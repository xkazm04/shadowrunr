//! Segment directions (docs/guide/characters.md § Segment directions): the owner's notes on
//! how a character runs one segment, one file per segment, `segments/<SEG-ID>.md` in the
//! package. The brain reads it into that segment's per-turn section while it is live.
//!
//! The daemon reads and writes that file only, the way the Tone Lab writes `directions.md`:
//! the character comes through [`tone::package_dir`], the segment is a typed [`SegmentId`] (an
//! unknown one never parses), every target is checked to resolve inside `paths.characters`, the
//! write goes through a temp file and a rename, then the package is validated and a write that
//! adds an error is undone and refused. The owner commits what it writes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use shadowrunr_brain::character::{segment_directions_file, SEGMENT_DIRECTIONS_DIR};
use shadowrunr_types::segment_directions::{
    SegmentDirections, SegmentDirectionsGetParams, SegmentDirectionsSetParams,
};
use shadowrunr_types::{SegmentId, TsMs};

use crate::config::Config;
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::tone::{self, contained, read_text, write_validated, WRITE_LOCK};

/// The longest segment directions file the console writes, in characters (as `directions.md`).
pub const MAX_SEGMENT_DIRECTIONS_CHARS: usize = tone::MAX_DIRECTIONS_CHARS;

/// The package dir of `character` and the path of `segment`'s directions file in it, checked
/// to stay inside the characters directory where it already exists.
fn target(
    cfg: &Config,
    character: &str,
    segment: SegmentId,
) -> Result<(PathBuf, PathBuf), RpcError> {
    let dir = tone::package_dir(cfg, character)?;
    let seg_dir = dir.join(SEGMENT_DIRECTIONS_DIR);
    if seg_dir.exists() {
        contained(&cfg.paths.characters, &seg_dir)?;
    }
    let path = dir.join(segment_directions_file(segment));
    if path.exists() {
        contained(&cfg.paths.characters, &path)?;
    }
    Ok((dir, path))
}

/// The file's last write in ms since the epoch; `None` without the file.
fn updated_ms(path: &Path) -> Option<TsMs> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let ms = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    TsMs::try_from(ms).ok()
}

pub fn get(cfg: &Config, p: &SegmentDirectionsGetParams) -> Result<SegmentDirections, RpcError> {
    let (_, path) = target(cfg, &p.character, p.segment_id)?;
    Ok(SegmentDirections {
        character: p.character.clone(),
        segment_id: p.segment_id,
        text: read_text(&path)?.unwrap_or_default().replace("\r\n", "\n"),
        updated_ms: updated_ms(&path),
    })
}

/// Replace `segments/<SEG-ID>.md`: line ends as LF, one trailing newline; an empty text
/// writes an empty file, which the brain ignores.
pub fn set(cfg: &Config, p: &SegmentDirectionsSetParams) -> Result<SegmentDirections, RpcError> {
    let (dir, path) = target(cfg, &p.character, p.segment_id)?;
    if p.text.chars().count() > MAX_SEGMENT_DIRECTIONS_CHARS {
        return Err(RpcError::invalid(format!(
            "segment directions are longer than {MAX_SEGMENT_DIRECTIONS_CHARS} characters"
        )));
    }
    let body = p.text.replace("\r\n", "\n");
    let body = body.trim_end();
    let text = if body.is_empty() {
        String::new()
    } else {
        format!("{body}\n")
    };
    let file = segment_directions_file(p.segment_id);
    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    write_validated(&dir, &path, &file, &text)?;
    Ok(SegmentDirections {
        character: p.character.clone(),
        segment_id: p.segment_id,
        text,
        updated_ms: updated_ms(&path),
    })
}

pub async fn rpc_get(
    state: &Arc<AppState>,
    p: SegmentDirectionsGetParams,
) -> Result<SegmentDirections, RpcError> {
    let cfg = state.cfg.clone();
    tone::blocking(move || get(&cfg, &p)).await
}

pub async fn rpc_set(
    state: &Arc<AppState>,
    p: SegmentDirectionsSetParams,
) -> Result<SegmentDirections, RpcError> {
    let cfg = state.cfg.clone();
    tone::blocking(move || set(&cfg, &p)).await
}

#[cfg(test)]
mod tests {
    use shadowrunr_types::ErrorCode;

    use super::*;
    use crate::e2e::{copy_dir, repo, test_config};

    /// A config whose characters dir is a temp copy of `malakor` (never the repo's files).
    fn setup(tmp: &Path) -> (Config, PathBuf) {
        let chars = tmp.join("characters");
        copy_dir(&repo().join("characters/malakor"), &chars.join("malakor"));
        let cfg = test_config(tmp, chars.clone());
        (cfg, chars.join("malakor"))
    }

    fn set_p(character: &str, segment_id: SegmentId, text: &str) -> SegmentDirectionsSetParams {
        SegmentDirectionsSetParams {
            character: character.into(),
            segment_id,
            text: text.into(),
        }
    }

    fn get_p(character: &str, segment_id: SegmentId) -> SegmentDirectionsGetParams {
        SegmentDirectionsGetParams {
            character: character.into(),
            segment_id,
        }
    }

    #[test]
    fn segment_directions_round_trip_and_reach_the_loaded_package() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let seg = SegmentId::Seg02aHexmap;
        let none = get(&cfg, &get_p("malakor", seg)).unwrap();
        assert_eq!(none.text, "");
        assert_eq!(none.updated_ms, None);
        assert_eq!(
            serde_json::to_value(&none).unwrap(),
            serde_json::json!({"character": "malakor", "segment_id": "SEG-02A", "text": ""}),
            "an absent updated_ms is omitted"
        );

        let w = set(
            &cfg,
            &set_p(
                "malakor",
                seg,
                "- Pin a record first.\r\n\r\n> L12 \"x\"\r\n\r\n",
            ),
        )
        .unwrap();
        assert_eq!(w.text, "- Pin a record first.\n\n> L12 \"x\"\n");
        assert!(w.updated_ms.is_some());
        let path = dir.join("segments").join("SEG-02A.md");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), w.text);
        assert_eq!(get(&cfg, &get_p("malakor", seg)).unwrap().text, w.text);
        let pkg = shadowrunr_brain::load_character(&dir, "").unwrap();
        assert_eq!(pkg.segment_directions.get(&seg), Some(&w.text));
        assert!(!pkg
            .segment_directions
            .contains_key(&SegmentId::Seg01ImprovisedRoom));

        // Emptied: an empty file, which the brain ignores.
        let e = set(&cfg, &set_p("malakor", seg, "  \n")).unwrap();
        assert_eq!(e.text, "");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let pkg = shadowrunr_brain::load_character(&dir, "").unwrap();
        assert!(pkg.segment_directions.is_empty());
        let leftovers: Vec<_> = std::fs::read_dir(dir.join("segments"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("tone-lab"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn an_unknown_segment_or_an_oversized_text_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let e = serde_json::from_value::<SegmentDirectionsSetParams>(serde_json::json!({
            "character": "malakor", "segment_id": "SEG-99", "text": "x"
        }));
        assert!(e.is_err(), "an unknown segment never parses");
        let e = set(
            &cfg,
            &set_p(
                "malakor",
                SegmentId::Seg01ImprovisedRoom,
                &"x".repeat(MAX_SEGMENT_DIRECTIONS_CHARS + 1),
            ),
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams);
        assert!(!dir.join("segments").exists());
    }

    #[test]
    fn segment_directions_writes_stay_inside_the_characters_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, _) = setup(tmp.path());
        let snapshot = || {
            let mut v = Vec::new();
            fn walk(p: &Path, v: &mut Vec<PathBuf>) {
                for e in std::fs::read_dir(p).unwrap().filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p, v);
                    } else {
                        v.push(p);
                    }
                }
            }
            walk(tmp.path(), &mut v);
            v.sort();
            v
        };
        let files = snapshot();
        for character in [
            "..",
            "../characters/malakor",
            "malakor/..",
            "",
            "..\\malakor",
            "/etc",
        ] {
            let e = set(&cfg, &set_p(character, SegmentId::Seg01ImprovisedRoom, "x")).unwrap_err();
            assert!(
                matches!(e.code, ErrorCode::InvalidParams | ErrorCode::NotFound),
                "{character}: {e}"
            );
            let e = get(&cfg, &get_p(character, SegmentId::Seg01ImprovisedRoom)).unwrap_err();
            assert!(
                matches!(e.code, ErrorCode::InvalidParams | ErrorCode::NotFound),
                "{character}: {e}"
            );
        }
        assert_eq!(snapshot(), files, "no file was written or removed");
    }
}
