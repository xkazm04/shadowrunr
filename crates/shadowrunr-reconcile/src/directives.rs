//! The Lord's standing directives, persisted as a JSON array in `<data>/directives.json`.
//! Every stream receives the active ones until they are deactivated.
//!
//! A directive has three live states (registry technique `hitl-approval/gate-state-machines`):
//!
//! | state    | fields                            | written by                              |
//! |----------|-----------------------------------|-----------------------------------------|
//! | proposed | `active: false, proposed: true`   | the Reconciler (`lord.chat`), via [`DirectiveStore::propose`] |
//! | active   | `active: true`                    | the owner: [`DirectiveStore::add`]      |
//! | inactive | `active: false`                   | the owner: [`DirectiveStore::deactivate`] |
//!
//! Only the owner's paths (`directive.add`, `directive.deactivate`) leave `proposed`. A model
//! reply can only create proposals, never an active directive, and a proposal binds to its
//! exact content: the owner keeps it by adding the same kind, text and scope.

use std::path::{Path, PathBuf};

use shadowrunr_types::{SegmentId, TsMs};

use crate::types::{Directive, DirectiveKind};
use crate::{write_json_atomic, ReconcileError};

/// Longest directive text accepted, in characters.
pub const MAX_DIRECTIVE_CHARS: usize = 600;

#[derive(Debug, Clone)]
pub struct DirectiveStore {
    path: PathBuf,
}

impl DirectiveStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("directives.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All directives, oldest first. A missing file is an empty list.
    pub fn list(&self) -> Result<Vec<Directive>, ReconcileError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) if text.trim().is_empty() => Ok(Vec::new()),
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                ReconcileError::Io(format!(
                    "{}: corrupt directives file: {e}",
                    self.path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(ReconcileError::Io(format!("{}: {e}", self.path.display()))),
        }
    }

    pub fn active(&self) -> Result<Vec<Directive>, ReconcileError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|d| d.active && !d.proposed)
            .collect())
    }

    /// The owner adds a directive: it is active at once. When it repeats a pending proposal
    /// (same kind and scope, same text ignoring ASCII case) that proposal is kept instead:
    /// it becomes active under its own id, so no duplicate is stored.
    pub fn add(
        &self,
        kind: DirectiveKind,
        text: &str,
        scope: Option<SegmentId>,
        now_ms: TsMs,
    ) -> Result<Directive, ReconcileError> {
        let text = check_text(text)?;
        let mut all = self.list()?;
        if let Some(d) = all
            .iter_mut()
            .find(|d| d.proposed && same_content(d, kind, text, scope))
        {
            d.proposed = false;
            d.active = true;
            let out = d.clone();
            write_json_atomic(&self.path, &all)?;
            return Ok(out);
        }
        let d = Directive {
            id: next_id(&all, now_ms),
            kind,
            text: text.to_string(),
            scope,
            created_ms: now_ms,
            active: true,
            proposed: false,
        };
        all.push(d.clone());
        write_json_atomic(&self.path, &all)?;
        Ok(d)
    }

    /// A directive the Reconciler proposed: stored inactive until the owner keeps it.
    pub fn propose(
        &self,
        kind: DirectiveKind,
        text: &str,
        scope: Option<SegmentId>,
        now_ms: TsMs,
    ) -> Result<Directive, ReconcileError> {
        let text = check_text(text)?;
        let mut all = self.list()?;
        let d = Directive {
            id: next_id(&all, now_ms),
            kind,
            text: text.to_string(),
            scope,
            created_ms: now_ms,
            active: false,
            proposed: true,
        };
        all.push(d.clone());
        write_json_atomic(&self.path, &all)?;
        Ok(d)
    }

    /// Proposals still waiting for the owner, oldest first.
    pub fn proposed(&self) -> Result<Vec<Directive>, ReconcileError> {
        Ok(self.list()?.into_iter().filter(|d| d.proposed).collect())
    }

    /// Deactivate a directive, or drop a proposal (kept on disk for history either way).
    pub fn deactivate(&self, id: &str) -> Result<Directive, ReconcileError> {
        let mut all = self.list()?;
        let Some(d) = all.iter_mut().find(|d| d.id == id) else {
            return Err(ReconcileError::NotFound(format!("directive {id}")));
        };
        d.active = false;
        d.proposed = false;
        let out = d.clone();
        write_json_atomic(&self.path, &all)?;
        Ok(out)
    }
}

fn check_text(text: &str) -> Result<&str, ReconcileError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ReconcileError::InvalidInput(
            "directive text is empty".into(),
        ));
    }
    if text.chars().count() > MAX_DIRECTIVE_CHARS {
        return Err(ReconcileError::InvalidInput(format!(
            "directive text is longer than {MAX_DIRECTIVE_CHARS} characters"
        )));
    }
    Ok(text)
}

fn next_id(all: &[Directive], now_ms: TsMs) -> String {
    let mut seq = all.len() + 1;
    let mut id = format!("dir-{now_ms}-{seq}");
    while all.iter().any(|d| d.id == id) {
        seq += 1;
        id = format!("dir-{now_ms}-{seq}");
    }
    id
}

/// Same kind, scope and text (ASCII case ignored): the duplicate rule `lord.chat` uses too.
pub fn same_content(
    d: &Directive,
    kind: DirectiveKind,
    text: &str,
    scope: Option<SegmentId>,
) -> bool {
    d.kind == kind && d.scope == scope && d.text.trim().eq_ignore_ascii_case(text.trim())
}
