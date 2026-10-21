//! `<data>/clips/<id>/candidate.json` and the files beside it, and the owner's state
//! transitions. Every write is durable (temp file, `sync_all`, rename) and every mutation
//! holds an exclusive lock on `<data>/clips/.lock`, so the CLI and the daemon can share the
//! store.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::Serialize;
use shadowrunr_types::clips::{
    ClipAct, ClipApproval, ClipCandidate, ClipFeedback, ClipFeedbackTag, ClipRejection, ClipStatus,
    ClipSummary, ClipTransition, ClipsListParams, ClipsPage, GateState,
};

use crate::{now_ms, ClipError, Result};

/// The largest page `clips.list` returns.
pub const MAX_LIMIT: u32 = 500;
/// The longest watched-feedback note.
pub const MAX_NOTE_CHARS: usize = 2_000;

#[derive(Debug, Clone)]
pub struct ClipStore {
    root: PathBuf,
}

/// Held while a mutation runs.
pub struct StoreLock {
    file: File,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Durable write: `<path>.tmp`, `sync_all`, rename over `path`.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |e: std::io::Error| ClipError::Io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = File::create(&tmp).map_err(io)?;
    f.write_all(bytes).map_err(io)?;
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(io)
}

pub fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let mut text = serde_json::to_string_pretty(v).map_err(|e| ClipError::Io(e.to_string()))?;
    text.push('\n');
    write_bytes(path, text.as_bytes())
}

/// `clip-` and 16 lowercase hex digits.
pub fn check_id(id: &str) -> Result<()> {
    let ok = id.len() == 21
        && id.starts_with("clip-")
        && id[5..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    if ok {
        Ok(())
    } else {
        Err(ClipError::Invalid(format!(
            "clip id {id:?} must be clip-<16 hex>"
        )))
    }
}

pub fn summary(c: &ClipCandidate) -> ClipSummary {
    ClipSummary {
        id: c.id.clone(),
        status: c.status,
        score: c.score,
        rank: c.rank,
        lane: c.lane,
        segment: c.segment,
        title: c.title.clone(),
        tournament_id: c.source.tournament_id.clone(),
        run_id: c.source.run_id.clone(),
        duration_s: c.window.duration_s,
        held: c.gate.held,
        requires_mandatory_gate: c.gate.requires_mandatory_gate,
        render: c.render.as_ref().map(|r| r.kind.clone()),
        created_ms: c.created_ms,
    }
}

/// Refuse `act` on `c` when the contract's owner-act rule blocks it; the error text is the
/// block's own refusal.
pub fn refuse_blocked(c: &ClipCandidate, act: ClipAct) -> Result<()> {
    match c.act_block(act) {
        None => Ok(()),
        Some(b) => Err(ClipError::Conflict(format!(
            "clip {} {}",
            c.id,
            b.refusal(act)
        ))),
    }
}

impl ClipStore {
    /// The store under `<data>/clips`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("clips"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<data>`, whose case store holds the corrections logs the gate reads.
    pub fn data_dir(&self) -> PathBuf {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    pub fn lock(&self) -> Result<StoreLock> {
        let io = |e: std::io::Error| ClipError::Io(format!("{}: {e}", self.root.display()));
        std::fs::create_dir_all(&self.root).map_err(io)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(".lock"))
            .map_err(io)?;
        file.lock_exclusive().map_err(io)?;
        Ok(StoreLock { file })
    }

    pub fn exists(&self, id: &str) -> bool {
        self.dir(id).join("candidate.json").is_file()
    }

    /// A stored candidate, with its `acts` derived from its present state.
    pub fn load(&self, id: &str) -> Result<ClipCandidate> {
        check_id(id)?;
        let path = self.dir(id).join("candidate.json");
        let text = std::fs::read_to_string(&path)
            .map_err(|_| ClipError::NotFound(format!("clip {id}")))?;
        serde_json::from_str::<ClipCandidate>(&text)
            .map(ClipCandidate::with_acts)
            .map_err(|e| ClipError::Io(format!("{}: {e}", path.display())))
    }

    /// Writes the candidate without `acts` (derived on every read, never stored).
    pub fn save(&self, c: &ClipCandidate) -> Result<()> {
        check_id(&c.id)?;
        let path = self.dir(&c.id).join("candidate.json");
        if c.acts.is_none() {
            return write_json(&path, c);
        }
        let mut stored = c.clone();
        stored.acts = None;
        write_json(&path, &stored)
    }

    /// Every stored candidate. A malformed `candidate.json` is an error naming the file,
    /// never a silent gap in the list.
    pub fn all(&self) -> Result<Vec<ClipCandidate>> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            if check_id(&name).is_err() || !e.path().join("candidate.json").is_file() {
                continue;
            }
            out.push(self.load(&name)?);
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// `clips.list`: by score, best first, with counts by status over the whole store.
    pub fn page(&self, p: &ClipsListParams) -> Result<ClipsPage> {
        if p.limit == 0 {
            return Err(ClipError::Invalid("limit must be at least 1".into()));
        }
        let limit = p.limit.min(MAX_LIMIT) as usize;
        let offset: usize = match &p.cursor {
            Some(c) => c
                .strip_prefix("o")
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| ClipError::Invalid(format!("bad cursor {c:?}")))?,
            None => 0,
        };
        let all = self.all()?;
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for c in &all {
            *counts.entry(c.status.as_str().to_string()).or_insert(0) += 1;
        }
        let rows: Vec<&ClipCandidate> = all
            .iter()
            .filter(|c| p.status.is_none_or(|s| c.status == s))
            .filter(|c| p.run_id.as_ref().is_none_or(|r| &c.source.run_id == r))
            .filter(|c| {
                p.tournament_id
                    .as_ref()
                    .is_none_or(|t| c.source.tournament_id.as_ref() == Some(t))
            })
            .collect();
        let total = rows.len() as u32;
        let items: Vec<ClipSummary> = rows
            .iter()
            .skip(offset)
            .take(limit)
            .map(|c| summary(c))
            .collect();
        let next = offset + items.len();
        Ok(ClipsPage {
            items,
            counts,
            held: all
                .iter()
                .filter(|c| c.gate.held && c.status == ClipStatus::PendingOwnerGate)
                .count() as u32,
            total,
            next_cursor: (next < rows.len()).then(|| format!("o{next}")),
        })
    }

    /// The owner approves: refused unless the candidate waits at the gate, no check failed,
    /// and the window needs no re-authoring. An unmeasured check needs `override_reason`,
    /// recorded with the hold reasons it overrode.
    pub fn approve(
        &self,
        id: &str,
        signature: &str,
        override_reason: Option<&str>,
        note: Option<&str>,
    ) -> Result<ClipCandidate> {
        let signature = signature.trim();
        if signature.is_empty() {
            return Err(ClipError::Invalid(
                "an approval needs the owner's signature".into(),
            ));
        }
        let _lock = self.lock()?;
        let mut c = self.load(id)?;
        refuse_blocked(&c, ClipAct::Approve)?;
        let unmeasured: Vec<String> = c
            .gate
            .checks
            .iter()
            .filter(|k| k.state == GateState::Unmeasured)
            .map(|k| format!("{}: {}", k.id, k.detail))
            .collect();
        let reason = override_reason.map(str::trim).filter(|r| !r.is_empty());
        if !unmeasured.is_empty() && reason.is_none() {
            return Err(ClipError::Conflict(format!(
                "clip {id} has unmeasured checks ({}); approving it needs an override reason",
                unmeasured.join("; ")
            )));
        }
        let at = now_ms();
        c.approval = Some(ClipApproval {
            signature: signature.to_string(),
            at_ms: at,
            override_reason: if unmeasured.is_empty() {
                None
            } else {
                reason.map(String::from)
            },
            overridden: unmeasured,
            note: note
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(String::from),
        });
        c.status = ClipStatus::ApprovedForUpload;
        c.history.push(ClipTransition {
            status: ClipStatus::ApprovedForUpload,
            at_ms: at,
            by: signature.to_string(),
            note: Some("owner signed: the cut keeps its context and the captions claim no more than it shows".into()),
        });
        self.save(&c)?;
        Ok(c.with_acts())
    }

    /// The owner rejects: from any state before `published`, with a reason kept beside the
    /// gate's own hold reasons.
    pub fn reject(&self, id: &str, reason: &str, by: Option<&str>) -> Result<ClipCandidate> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(ClipError::Invalid("a rejection needs a reason".into()));
        }
        let _lock = self.lock()?;
        let mut c = self.load(id)?;
        refuse_blocked(&c, ClipAct::Reject)?;
        let by = by
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .unwrap_or("owner")
            .to_string();
        let at = now_ms();
        c.rejection = Some(ClipRejection {
            reason: reason.to_string(),
            at_ms: at,
            by: by.clone(),
        });
        c.status = ClipStatus::Rejected;
        c.history.push(ClipTransition {
            status: ClipStatus::Rejected,
            at_ms: at,
            by,
            note: Some(reason.to_string()),
        });
        self.save(&c)?;
        Ok(c.with_acts())
    }

    /// Watched feedback: replaces the clip's tags and note, in any state, and changes nothing
    /// else. Tags are kept once each, in the order given; a blank note is dropped. No tags and
    /// no note clear the feedback.
    pub fn feedback(
        &self,
        id: &str,
        tags: &[ClipFeedbackTag],
        note: Option<&str>,
    ) -> Result<ClipCandidate> {
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        if note.is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS) {
            return Err(ClipError::Invalid(format!(
                "a feedback note is at most {MAX_NOTE_CHARS} characters"
            )));
        }
        let _lock = self.lock()?;
        let mut c = self.load(id)?;
        let mut kept: Vec<ClipFeedbackTag> = Vec::new();
        for t in tags {
            if !kept.contains(t) {
                kept.push(*t);
            }
        }
        c.feedback = (!kept.is_empty() || note.is_some()).then(|| ClipFeedback {
            tags: kept,
            note: note.map(String::from),
            at_ms: now_ms(),
        });
        self.save(&c)?;
        Ok(c)
    }
}
