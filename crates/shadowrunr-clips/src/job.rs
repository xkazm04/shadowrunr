//! A re-render that never holds the store lock across the render.
//!
//! A render takes minutes (a Chromium recording, two ffmpeg encodes), and the store's one
//! exclusive lock (`<data>/clips/.lock`) guards every owner act. So `clips render` runs in
//! three phases:
//!
//! 1. **Snapshot** (store lock, milliseconds): load the candidate, apply the owner-act rule,
//!    take the per-clip render guard, note `(status, render.rendered_ms, history.len())` and
//!    copy its `clip.json` into a staging dir.
//! 2. **Render** (no store lock): the [`Renderer`] writes every output into the staging dir.
//!    The clip dir is not touched, so the media route only ever serves complete files.
//! 3. **Commit** (store lock, milliseconds): reload the candidate. Gone (a rescan removed the
//!    untouched draft) is `NotFound` and nothing is recreated; moved (the owner approved or
//!    rejected it, another act wrote history) is `Conflict` and the render is discarded.
//!    Otherwise the staged files are renamed over the live ones and the candidate is re-gated
//!    on its fresh copy (feedback given meanwhile is kept).
//!
//! A failed render never costs the previous output: a renderer `Err`, or a poster where the
//! clip already had a video, keeps the old render and its files and records a
//! `render failed: <reason>` note. A second render of the same clip is refused at once.
//!
//! The guard and the staging dir live under `<data>/clips/.render/`, outside every clip dir,
//! so a rescan can remove a draft while it renders and the media route cannot reach a staged
//! file.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use shadowrunr_types::clips::{ClipAct, ClipCandidate, ClipRender, ClipStatus, ClipTransition};
use shadowrunr_types::TsMs;

use crate::render::{self, RenderTools};
use crate::scan::{regate, stored_exclusions};
use crate::store::{self, ClipStore};
use crate::{now_ms, ClipError, Result};

/// Renders a candidate into a directory holding a copy of its `clip.json`.
pub trait Renderer {
    fn render(&self, staging: &Path, c: &ClipCandidate) -> Result<ClipRender>;
}

/// The real renderer: [`render::render`] with the configured tools.
pub struct ToolRenderer<'a>(pub Option<&'a RenderTools>);

impl Renderer for ToolRenderer<'_> {
    fn render(&self, staging: &Path, c: &ClipCandidate) -> Result<ClipRender> {
        render::render(staging, c, self.0)
    }
}

/// Outputs a render may leave in a clip dir; one the new render did not produce is removed
/// when the new render commits, so a stale cut never sits beside a fresh poster.
const OUTPUTS: [&str; 6] = [
    "draft.mp4",
    "poster.png",
    "cut-16x9.mp4",
    "cut-9x16.mp4",
    "cut-16x9.ass",
    "captions-9x16.ass",
];

fn render_root(store: &ClipStore) -> PathBuf {
    store.root().join(".render")
}

/// Where a render of `id` stages its files.
pub fn staging_dir(store: &ClipStore, id: &str) -> PathBuf {
    render_root(store).join(id)
}

/// Held while a render of one clip runs; a second render of it is refused.
struct RenderGuard {
    file: File,
}

impl Drop for RenderGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn guard(store: &ClipStore, id: &str) -> Result<RenderGuard> {
    let root = render_root(store);
    let io = |e: std::io::Error| ClipError::Io(format!("{}: {e}", root.display()));
    std::fs::create_dir_all(&root).map_err(io)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(format!("{id}.lock")))
        .map_err(io)?;
    if file.try_lock_exclusive().is_err() {
        return Err(ClipError::Conflict(format!(
            "clip {id}: render in progress (another render of this clip holds it)"
        )));
    }
    Ok(RenderGuard { file })
}

/// The staging dir, removed however the render ends.
struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the commit compares: the candidate moved if any of these did.
#[derive(PartialEq)]
struct Mark {
    status: ClipStatus,
    rendered_ms: Option<TsMs>,
    history: usize,
}

fn mark(c: &ClipCandidate) -> Mark {
    Mark {
        status: c.status,
        rendered_ms: c.render.as_ref().map(|r| r.rendered_ms),
        history: c.history.len(),
    }
}

fn is_video(r: Option<&ClipRender>) -> bool {
    r.is_some_and(|r| r.kind == "video")
}

/// Re-render `id` with `renderer`: snapshot, render, commit (see the module docs).
pub fn rerender_with(
    store: &ClipStore,
    id: &str,
    renderer: &dyn Renderer,
) -> Result<ClipCandidate> {
    // 1. Snapshot. The guard outlives the staging dir (locals drop in reverse order), so a
    // second render can never start while this one's staging dir still exists.
    let lock = store.lock()?;
    let snapshot = store.load(id)?;
    store::refuse_blocked(&snapshot, ClipAct::Render)?;
    let _guard = guard(store, id)?;
    let staging = Staging(staging_dir(store, id));
    let io = |e: std::io::Error| ClipError::Io(format!("{}: {e}", staging.0.display()));
    let _ = std::fs::remove_dir_all(&staging.0);
    std::fs::create_dir_all(&staging.0).map_err(io)?;
    let input = store.dir(id).join("clip.json");
    std::fs::copy(&input, staging.0.join("clip.json"))
        .map_err(|e| ClipError::Io(format!("{}: {e}", input.display())))?;
    drop(lock);
    let before = mark(&snapshot);

    // 2. Render, unlocked, into the staging dir.
    let rendered = renderer.render(&staging.0, &snapshot);

    // 3. Commit.
    let _lock = store.lock()?;
    let mut c = match store.load(id) {
        Ok(c) => c,
        Err(ClipError::NotFound(_)) => {
            return Err(ClipError::NotFound(format!(
                "clip {id} was removed while it rendered (a rescan replaced it); the render is discarded"
            )))
        }
        Err(e) => return Err(e),
    };
    if mark(&c) != before {
        return Err(ClipError::Conflict(match &c.rejection {
            Some(r) if snapshot.rejection.is_none() => format!(
                "clip {id} was rejected while it rendered ({}); the render is discarded",
                r.reason
            ),
            _ => format!(
                "clip {id} changed while it rendered ({} -> {}); the render is discarded",
                snapshot.status, c.status
            ),
        }));
    }
    let failure = match rendered {
        Err(e) => Some(e.to_string()),
        Ok(r) if is_video(c.render.as_ref()) && !is_video(Some(&r)) => Some(
            r.fallback_reason
                .filter(|why| !why.is_empty())
                .unwrap_or_else(|| "no video was made".into()),
        ),
        Ok(r) => {
            publish(&staging.0, &store.dir(id))?;
            commit(store, &mut c, r);
            store.save(&c)?;
            return Ok(c.with_acts());
        }
    };
    let why = failure.unwrap_or_default();
    let kept = if is_video(c.render.as_ref()) {
        "the previous video is kept"
    } else {
        "the previous render is kept"
    };
    c.history.push(ClipTransition {
        status: c.status,
        at_ms: now_ms(),
        by: "render".into(),
        note: Some(format!("render failed: {why}; {kept}")),
    });
    store.save(&c)?;
    Err(ClipError::Io(format!(
        "clip {id}: render failed: {why}; {kept}"
    )))
}

/// Rename every staged file over its live one, then remove the outputs this render did not
/// make. Each rename is atomic, so a reader sees the old file or the new one.
fn publish(staging: &Path, live: &Path) -> Result<()> {
    let io = |e: std::io::Error| ClipError::Io(format!("{}: {e}", staging.display()));
    let mut made = Vec::new();
    for e in std::fs::read_dir(staging).map_err(io)? {
        let e = e.map_err(io)?;
        let name = e.file_name().to_string_lossy().into_owned();
        if !e.file_type().map_err(io)?.is_file() || name.ends_with(".tmp") {
            continue;
        }
        std::fs::rename(e.path(), live.join(&name))
            .map_err(|err| ClipError::Io(format!("{}: {err}", live.join(&name).display())))?;
        made.push(name);
    }
    for f in OUTPUTS {
        if !made.iter().any(|m| m == f) {
            let _ = std::fs::remove_file(live.join(f));
        }
    }
    Ok(())
}

/// The render's record, its history, and the gate re-run on it.
fn commit(store: &ClipStore, c: &mut ClipCandidate, r: ClipRender) {
    let note = match &r.fallback_reason {
        Some(why) => format!("poster frame only: {why}"),
        None => format!("video {:.1} s, {}x{}", r.duration_s, r.width, r.height),
    };
    c.history.push(ClipTransition {
        status: ClipStatus::RenderedDraft,
        at_ms: r.rendered_ms,
        by: "render".into(),
        note: Some(note),
    });
    c.render = Some(r);
    let exclusions = stored_exclusions(c);
    regate(store, c, exclusions);
    c.status = ClipStatus::PendingOwnerGate;
    c.history.push(ClipTransition {
        status: ClipStatus::PendingOwnerGate,
        at_ms: c.gate.evaluated_ms,
        by: "gate".into(),
        note: Some(if c.gate.held {
            format!("held: {}", c.gate.hold_reasons.len())
        } else {
            "every check passed; waits for the owner's signature".into()
        }),
    });
}
