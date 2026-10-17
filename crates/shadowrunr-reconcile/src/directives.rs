//! The Lord's standing directives, persisted as a JSON array in `<data>/directives.json`.
//! Every stream receives the active ones until they are deactivated.

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
        Ok(self.list()?.into_iter().filter(|d| d.active).collect())
    }

    pub fn add(
        &self,
        kind: DirectiveKind,
        text: &str,
        scope: Option<SegmentId>,
        now_ms: TsMs,
    ) -> Result<Directive, ReconcileError> {
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
        let mut all = self.list()?;
        let mut seq = all.len() + 1;
        let mut id = format!("dir-{now_ms}-{seq}");
        while all.iter().any(|d| d.id == id) {
            seq += 1;
            id = format!("dir-{now_ms}-{seq}");
        }
        let d = Directive {
            id,
            kind,
            text: text.to_string(),
            scope,
            created_ms: now_ms,
            active: true,
        };
        all.push(d.clone());
        write_json_atomic(&self.path, &all)?;
        Ok(d)
    }

    /// Deactivate a directive (kept on disk for history).
    pub fn deactivate(&self, id: &str) -> Result<Directive, ReconcileError> {
        let mut all = self.list()?;
        let Some(d) = all.iter_mut().find(|d| d.id == id) else {
            return Err(ReconcileError::NotFound(format!("directive {id}")));
        };
        d.active = false;
        let out = d.clone();
        write_json_atomic(&self.path, &all)?;
        Ok(out)
    }
}
