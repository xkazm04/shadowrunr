//! `<data>/prep/<prep_id>/prep.json` and the owner's transitions (staged -> approved ->
//! retired). Every write is durable (temp file, `sync_all`, rename) and every mutation holds an
//! exclusive lock on `<data>/prep/.lock`, as the clip store does, so the CLI and the daemon can
//! share the store.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use shadowrunr_types::prep::{PrepCheck, PrepStatus, PrepSummary, StreamPrep};
use shadowrunr_types::{ErrorCode, TsMs};

use super::checks;
use super::revise::{Revision, REVISION_FILE};
use crate::rpc::RpcError;

pub const PREP_FILE: &str = "prep.json";

#[derive(Debug, Clone)]
pub struct PrepStore {
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

fn conflict(m: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Conflict, m)
}

/// `prep-<yyyymmdd>-<hhmmss>`, optionally `-<n>` when two preps share a second.
pub fn check_id(id: &str) -> Result<(), RpcError> {
    let ok = id.strip_prefix("prep-").is_some_and(|rest| {
        let mut parts = rest.split('-');
        let date = parts.next().unwrap_or("");
        let time = parts.next().unwrap_or("");
        let n = parts.next();
        date.len() == 8
            && time.len() == 6
            && date.bytes().chain(time.bytes()).all(|b| b.is_ascii_digit())
            && n.is_none_or(|n| {
                !n.is_empty() && n.len() <= 4 && n.bytes().all(|b| b.is_ascii_digit())
            })
            && parts.next().is_none()
    });
    if ok {
        Ok(())
    } else {
        Err(RpcError::invalid(format!(
            "prep id {id:?} must be prep-<yyyymmdd>-<hhmmss>"
        )))
    }
}

/// `prep-<yyyymmdd>-<hhmmss>` for a UTC time.
pub fn id_for(ms: TsMs) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = crate::sandbox::civil_from_days(days);
    format!(
        "prep-{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn status_name(s: PrepStatus) -> &'static str {
    match s {
        PrepStatus::Staged => "staged",
        PrepStatus::Approved => "approved",
        PrepStatus::Retired => "retired",
    }
}

pub fn summary(p: &StreamPrep) -> PrepSummary {
    PrepSummary {
        prep_id: p.prep_id.clone(),
        created_ms: p.created_ms,
        status: p.status,
        title: p.cold_open.title.clone(),
        bits: p.bits.len() as u32,
        failed_checks: checks::failed(&p.checks).len() as u32,
    }
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> Result<(), RpcError> {
    use std::io::Write;
    let io = |e: std::io::Error| RpcError::io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut text = serde_json::to_string_pretty(v)
        .map_err(|e| RpcError::internal(format!("serialize prep: {e}")))?;
    text.push('\n');
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = File::create(&tmp).map_err(io)?;
    f.write_all(text.as_bytes()).map_err(io)?;
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(io)
}

impl PrepStore {
    /// The store under `<data>/prep`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("prep"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn lock(&self) -> Result<StoreLock, RpcError> {
        let io = |e: std::io::Error| RpcError::io(format!("{}: {e}", self.root.display()));
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

    fn path(&self, id: &str) -> PathBuf {
        self.root.join(id).join(PREP_FILE)
    }

    pub fn load(&self, id: &str) -> Result<StreamPrep, RpcError> {
        check_id(id)?;
        let path = self.path(id);
        let text = std::fs::read_to_string(&path)
            .map_err(|_| RpcError::not_found(format!("prep {id}")))?;
        serde_json::from_str(&text).map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
    }

    fn save(&self, p: &StreamPrep) -> Result<(), RpcError> {
        check_id(&p.prep_id)?;
        write_json(&self.path(&p.prep_id), p)
    }

    /// Every stored prep, newest first. A malformed `prep.json` is an error naming the file,
    /// never a silent gap in the list.
    pub fn all(&self) -> Result<Vec<StreamPrep>, RpcError> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            if check_id(&name).is_err() || !e.path().join(PREP_FILE).is_file() {
                continue;
            }
            out.push(self.load(&name)?);
        }
        out.sort_by(|a, b| {
            b.created_ms
                .cmp(&a.created_ms)
                .then_with(|| b.prep_id.cmp(&a.prep_id))
        });
        Ok(out)
    }

    /// `prep.list`: summaries, newest first.
    pub fn list(&self) -> Result<Vec<PrepSummary>, RpcError> {
        Ok(self.all()?.iter().map(summary).collect())
    }

    /// The newest approved prep for `character`, if any.
    pub fn newest_approved(&self, character: &str) -> Result<Option<StreamPrep>, RpcError> {
        Ok(self
            .all()?
            .into_iter()
            .find(|p| p.status == PrepStatus::Approved && p.character == character))
    }

    /// Store a freshly written prep as `staged` under a new id from its `created_ms`
    /// (`-2`, `-3` ... when that second is taken).
    pub fn stage(&self, p: StreamPrep) -> Result<StreamPrep, RpcError> {
        self.stage_with(p, None)
    }

    /// Stage a revised prep with its `revision.json` beside it (written first, so a listed
    /// revised prep always has its lineage). The base is not touched.
    pub fn stage_revision(&self, p: StreamPrep, rev: &Revision) -> Result<StreamPrep, RpcError> {
        self.stage_with(p, Some(rev))
    }

    fn stage_with(
        &self,
        mut p: StreamPrep,
        rev: Option<&Revision>,
    ) -> Result<StreamPrep, RpcError> {
        let _lock = self.lock()?;
        let base = id_for(p.created_ms);
        let mut id = base.clone();
        let mut n = 2;
        while self.root.join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        p.prep_id = id;
        p.status = PrepStatus::Staged;
        p.approved_by = None;
        p.approved_ms = None;
        if let Some(rev) = rev {
            write_json(&self.root.join(&p.prep_id).join(REVISION_FILE), rev)?;
        }
        self.save(&p)?;
        Ok(p)
    }

    /// `revision.json` of `id`: `None` for a prep that was not revised from another.
    pub fn revision(&self, id: &str) -> Result<Option<Revision>, RpcError> {
        check_id(id)?;
        let path = self.root.join(id).join(REVISION_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
    }

    /// The owner approves a staged prep. Refused (`conflict`) when it is not staged, when a
    /// check failed at generation, or when re-running the checks over the material as it is
    /// stored now fails one (the new results are kept, so the owner sees why). A failed check
    /// has no override: revise the prep (a new staged prep, checked again) or generate one.
    pub fn approve(
        &self,
        id: &str,
        signature: &str,
        now: TsMs,
        recheck: impl Fn(&StreamPrep) -> Vec<PrepCheck>,
    ) -> Result<StreamPrep, RpcError> {
        let signature = signature.trim();
        if signature.is_empty() {
            return Err(RpcError::invalid("an approval needs the owner's signature"));
        }
        let _lock = self.lock()?;
        let mut p = self.load(id)?;
        if p.status != PrepStatus::Staged {
            return Err(conflict(format!(
                "prep {id} is {}, not staged",
                status_name(p.status)
            )));
        }
        let failed: Vec<String> = checks::failed(&p.checks)
            .into_iter()
            .map(String::from)
            .collect();
        if !failed.is_empty() {
            return Err(conflict(format!(
                "prep {id} failed {}: a failed check holds and no override applies; revise it (`prep revise`) or generate a new prep",
                failed.join(", ")
            ))
            .with_detail(serde_json::json!({ "failed": failed })));
        }
        if p.checks.is_empty() {
            return Err(conflict(format!("prep {id} was never checked")));
        }
        let now_checks = recheck(&p);
        let now_failed: Vec<String> = checks::failed(&now_checks)
            .into_iter()
            .map(String::from)
            .collect();
        if !now_failed.is_empty() {
            p.checks = now_checks;
            self.save(&p)?;
            return Err(conflict(format!(
                "prep {id} now fails {}: the stored material changed or the rules did",
                now_failed.join(", ")
            ))
            .with_detail(serde_json::json!({ "failed": now_failed })));
        }
        p.checks = now_checks;
        p.status = PrepStatus::Approved;
        p.approved_by = Some(signature.to_string());
        p.approved_ms = Some(now);
        self.save(&p)?;
        Ok(p)
    }

    /// Retire a staged or approved prep: no stream uses it again.
    pub fn retire(&self, id: &str) -> Result<StreamPrep, RpcError> {
        let _lock = self.lock()?;
        let mut p = self.load(id)?;
        if p.status == PrepStatus::Retired {
            return Err(conflict(format!("prep {id} is already retired")));
        }
        p.status = PrepStatus::Retired;
        self.save(&p)?;
        Ok(p)
    }
}
