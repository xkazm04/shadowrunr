//! The operator's feedback log, `<data>/review/feedback.jsonl`: append-only `LineFeedback`
//! rows. Each append takes an exclusive file lock, writes the whole row and its newline in one
//! write and syncs before unlocking, so concurrent sets (in this daemon or another process)
//! never interleave bytes. Folding keeps the last row per `<session>#<line>` in file order and
//! drops lines whose last row clears them (no tags, no note). A malformed row is skipped and
//! counted; it never costs the rest of the log.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use shadowrunr_types::review::LineFeedback;

pub const FEEDBACK_FILE: &str = "feedback.jsonl";

/// `<data>/review/feedback.jsonl`.
pub fn feedback_path(review_dir: &Path) -> PathBuf {
    review_dir.join(FEEDBACK_FILE)
}

/// A row clears its line: no tags and no note.
pub fn clears(f: &LineFeedback) -> bool {
    f.tags.is_empty() && f.note.is_none()
}

/// Append one row durably: lock, write, sync, unlock. Creates the directory and the file.
pub fn append(path: &Path, row: &LineFeedback) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_vec(row).map_err(std::io::Error::other)?;
    line.push(b'\n');
    // `read` as well: Windows refuses a lock on a handle opened for appending alone.
    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    f.lock_exclusive()?;
    let written = f.write_all(&line).and_then(|()| f.sync_all());
    let unlocked = FileExt::unlock(&f);
    written.and(unlocked)
}

/// The folded log, and how many rows were skipped as malformed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Folded {
    /// Ordered by session, then line.
    pub lines: Vec<LineFeedback>,
    pub skipped: usize,
}

/// Fold `feedback.jsonl`: the last row per line wins, cleared lines are dropped. A missing
/// file folds to nothing.
pub fn fold(path: &Path) -> std::io::Result<Folded> {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Folded::default()),
        Err(e) => return Err(e),
    };
    // Shared, so a read never sees half of an append (and, on Windows, never fails on the
    // appender's lock).
    f.lock_shared()?;
    let mut bytes = Vec::new();
    let read = f.read_to_end(&mut bytes);
    let unlocked = FileExt::unlock(&f);
    read.and(unlocked)?;
    // Lossy: a row with broken bytes fails to parse and is skipped, the rest still fold.
    let text = String::from_utf8_lossy(&bytes);
    let mut latest: BTreeMap<(String, u64), LineFeedback> = BTreeMap::new();
    let mut skipped = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<LineFeedback>(line) {
            Ok(row) => {
                latest.insert((row.session_id.clone(), row.line), row);
            }
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!(
            "review: {}: skipped {skipped} malformed rows",
            path.display()
        );
    }
    Ok(Folded {
        lines: latest.into_values().filter(|f| !clears(f)).collect(),
        skipped,
    })
}
