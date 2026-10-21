//! Real SEG-02A cases: the store the off-stream research pipeline writes and the segment reads,
//! the owner's stream gate, and the corrections / right-of-reply log.
//!
//! ```text
//! <data>/cases/<case_id>/
//!   case.json          Case      (fictional: false; the map the segment loads)
//!   records.json       CaseFile  (fictional: false; the public records the bone-clerks read)
//!   meta.json          CaseMeta  (subject, category, primary record, status, owner approval,
//!                                 research provenance: every source fetched, PII-scrub hits,
//!                                 what the extraction refused)
//!   corrections.jsonl  CorrectionEntry, append-only
//! ```
//!
//! `<data>` is git-ignored: real case data is never committed.
//!
//! **The owner gate.** Research stages a case as `CALM_STAGED` with
//! `owner_approved_for_stream: false`. A live stream ([`StreamKind::Live`]) refuses a real case
//! until the owner approves it (`shadowrunr case approve <id>`), and an approval covers the exact
//! content it was given for: a case whose content hash changed since is unapproved again. A
//! private simulation ([`StreamKind::PrivateSim`]) may run an unapproved case; the simulator
//! marks such a run. A retracted case runs nowhere.
//!
//! Every write is durable: a temp file, flushed to disk, then renamed over the target.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shadowrunr_types::Case;

use crate::case::validate_case;
use crate::evidence::{ResearchAllowlist, SourceAllowlist};
use crate::research::CaseFile;
use crate::SegmentError;

pub const CASE_FILE: &str = "case.json";
pub const RECORDS_FILE: &str = "records.json";
pub const META_FILE: &str = "meta.json";
pub const CORRECTIONS_FILE: &str = "corrections.jsonl";

/// Who a case may name as its subject (digest section 4, guardrail 1). A private individual is
/// never a subject, and `COURT_RECORD_PUBLIC_FIGURE` is deliberately absent: a subject is a
/// company or a scheme named in a public enforcement record (gap section 10, item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubjectCategory {
    /// A company named in a regulator's or a court's public enforcement record.
    PublicEnforcementEntity,
    /// A scheme a regulator warned about or acted against in a public record.
    RegulatorWarnedScheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CaseStatus {
    /// Research staged the case off-stream; nothing has aired.
    CalmStaged,
    LiveActive,
    DossierSealed,
    /// Withdrawn: it runs nowhere, not even in a simulation.
    Retracted,
}

/// Who the stream is for. A live stream needs the owner's approval of a real case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    /// Goes out to an audience (the safe default: every gate applies).
    #[default]
    Live,
    /// A private simulation: nobody but the operator sees it.
    PrivateSim,
}

/// One page the research pipeline fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFetch {
    /// The publisher's URL.
    pub url: String,
    /// The Wayback capture fetched instead, when the publisher refused the fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// `YYYY-MM-DD` (UTC).
    pub retrieved: String,
    /// sha256 of the bytes retrieved.
    pub content_hash: String,
    pub bytes: u64,
    pub content_type: String,
    /// Its text reached the extraction (HTML or plain text; not a PDF).
    pub extracted: bool,
    /// At least one staged record cites it.
    pub used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A page the pipeline could not use, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFailure {
    pub url: String,
    pub why: String,
}

/// How often one PII-scrub rule fired before anything was stored. The scrubbed text itself is
/// never kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrubHit {
    pub kind: String,
    pub count: u32,
}

/// Something the extraction (or the pipeline's checks) refused to stage, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub what: String,
    pub why: String,
}

/// How a case was researched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchProvenance {
    /// What the operator asked for (`--subject`), scrubbed.
    pub subject_hint: String,
    pub researched_at_utc: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceFetch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<SourceFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pii_scrub: Vec<ScrubHit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<Refusal>,
    /// Where belief and record come apart: the case's reason to be watched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tension: Option<String>,
    /// What the records do not settle, each phrased as what may not be said on air.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknowns: Vec<String>,
    /// Engine calls the research made, one line each (`discovery: sonnet, 41 s`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
}

/// `meta.json`: everything about a staged case that is not its map or its records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseMeta {
    pub case_id: String,
    pub title: String,
    /// The entity or scheme under audit, as the public record names it.
    pub subject: String,
    pub subject_category: SubjectCategory,
    /// The regulator's or court's record the case rests on.
    pub primary_public_record_url: String,
    pub status: CaseStatus,
    pub owner_approved_for_stream: bool,
    /// When the owner approved, and the content hash they approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at_utc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_content_sha256: Option<String>,
    pub created_at_utc: String,
    /// [`content_sha256`] of `case.json` and `records.json`.
    pub content_sha256: String,
    pub research: ResearchProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CorrectionType {
    FactualCorrection,
    RightOfReplyStatement,
    GradeDowngrade,
}

/// One line of `corrections.jsonl` (digest section 4, `corrections_and_reply_log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionEntry {
    pub entry_id: String,
    pub case_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex_id: Option<String>,
    pub entry_type: CorrectionType,
    pub public_note: String,
    /// An open entry blocks clip publishing for the case until the owner clears it.
    pub blocks_clip_publishing: bool,
    pub logged_at_utc: String,
}

/// A staged real case, loaded and validated.
#[derive(Debug, Clone)]
pub struct StagedCase {
    pub case: Case,
    pub file: CaseFile,
    pub meta: CaseMeta,
    /// Recomputed from the files on disk (equals `meta.content_sha256` for a sound case).
    pub content_sha256: String,
}

impl StagedCase {
    /// The owner approved exactly this content for a live stream.
    pub fn approved_for_stream(&self) -> bool {
        self.meta.owner_approved_for_stream
            && self.meta.approved_content_sha256.as_deref() == Some(self.content_sha256.as_str())
    }
}

/// sha256 over the canonical JSON of a case and its records: the case's identity in the
/// simulator's instrument hash and in the owner's approval.
pub fn content_sha256(case: &Case, file: &CaseFile) -> String {
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(case).unwrap_or_default());
    h.update(b"\n");
    h.update(serde_json::to_vec(file).unwrap_or_default());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A case id: 3 to 64 of `A-Z`, `0-9` and `-`, starting with a letter (`WOODBRIDGE-2019`).
pub fn check_case_id(id: &str) -> Result<(), SegmentError> {
    let ok = (3..=64).contains(&id.len())
        && id.as_bytes()[0].is_ascii_uppercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(SegmentError::Config {
            detail: format!("case id {id:?}: use 3-64 of A-Z, 0-9 and -, starting with a letter"),
        })
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a Unix time in milliseconds.
pub fn utc_iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Now, in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn io(path: &Path, e: impl std::fmt::Display) -> SegmentError {
    SegmentError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    }
}

/// Write `bytes` durably: a temp file, flushed to disk, then renamed over `path`.
pub fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), SegmentError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = std::fs::File::create(&tmp).map_err(|e| io(&tmp, e))?;
    f.write_all(bytes).map_err(|e| io(&tmp, e))?;
    f.sync_all().map_err(|e| io(&tmp, e))?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| io(path, e))
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), SegmentError> {
    let mut text = serde_json::to_string_pretty(v).map_err(|e| io(path, e))?;
    text.push('\n');
    write_durable(path, text.as_bytes())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, SegmentError> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    serde_json::from_str(&text).map_err(|e| SegmentError::Data {
        file: path.display().to_string(),
        detail: e.to_string(),
    })
}

/// The real-case store under `<data>/cases/`.
#[derive(Debug, Clone)]
pub struct CaseStore {
    root: PathBuf,
    allowlist: SourceAllowlist,
    research: ResearchAllowlist,
}

impl CaseStore {
    /// The store of `<data_dir>`, checked against the built-in allowlists.
    pub fn new(data_dir: &Path) -> Self {
        Self::with_allowlists(
            data_dir,
            SourceAllowlist::default(),
            ResearchAllowlist::default(),
        )
    }

    pub fn with_allowlists(
        data_dir: &Path,
        allowlist: SourceAllowlist,
        research: ResearchAllowlist,
    ) -> Self {
        Self {
            root: data_dir.join("cases"),
            allowlist,
            research,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn research_allowlist(&self) -> &ResearchAllowlist {
        &self.research
    }

    /// `<data>/cases/<id>`, for a valid id.
    pub fn dir(&self, id: &str) -> Result<PathBuf, SegmentError> {
        check_case_id(id)?;
        Ok(self.root.join(id))
    }

    pub fn exists(&self, id: &str) -> bool {
        self.dir(id).is_ok_and(|d| d.join(META_FILE).is_file())
    }

    /// Ids of every staged case, sorted.
    pub fn ids(&self) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = rd
            .filter_map(Result::ok)
            .filter(|e| e.path().join(META_FILE).is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|id| check_case_id(id).is_ok())
            .collect();
        ids.sort();
        ids
    }

    /// The meta of every staged case, sorted by id. A case whose meta does not parse is an
    /// error, never skipped.
    pub fn list(&self) -> Result<Vec<CaseMeta>, SegmentError> {
        self.ids().iter().map(|id| self.meta(id)).collect()
    }

    pub fn meta(&self, id: &str) -> Result<CaseMeta, SegmentError> {
        read_json(&self.dir(id)?.join(META_FILE))
    }

    /// Load and validate a staged case: the map, its records against the allowlists, and the
    /// meta's content hash against the files.
    pub fn load(&self, id: &str) -> Result<StagedCase, SegmentError> {
        let dir = self.dir(id)?;
        if !dir.join(META_FILE).is_file() {
            return Err(SegmentError::Config {
                detail: format!("no staged case {id} under {}", self.root.display()),
            });
        }
        let case: Case = read_json(&dir.join(CASE_FILE))?;
        let file: CaseFile = read_json(&dir.join(RECORDS_FILE))?;
        let meta: CaseMeta = read_json(&dir.join(META_FILE))?;
        let bad = |detail: String| SegmentError::Config {
            detail: format!("case {id}: {detail}"),
        };
        if case.id != id || meta.case_id != id {
            return Err(bad("the files name another case id".into()));
        }
        if case.fictional || file.fictional {
            return Err(bad("a staged case is a real case (fictional: false)".into()));
        }
        validate_case(&case)?;
        file.validate_with(&case, &self.allowlist, &self.research)?;
        if !self
            .allowlist
            .is_primary(&meta.primary_public_record_url, false)
        {
            return Err(bad(format!(
                "primary_public_record_url {} is not a primary source",
                meta.primary_public_record_url
            )));
        }
        let content_sha256 = content_sha256(&case, &file);
        if content_sha256 != meta.content_sha256 {
            return Err(bad(
                "case.json or records.json changed after staging (content hash mismatch)".into(),
            ));
        }
        Ok(StagedCase {
            case,
            file,
            meta,
            content_sha256,
        })
    }

    /// A case for a stream of `kind`. A live stream refuses a case the owner has not approved
    /// (or whose content changed since the approval); a retracted case is refused everywhere.
    pub fn open_for_stream(&self, id: &str, kind: StreamKind) -> Result<StagedCase, SegmentError> {
        let staged = self.load(id)?;
        if staged.meta.status == CaseStatus::Retracted {
            return Err(SegmentError::Config {
                detail: format!("case {id} is retracted"),
            });
        }
        if kind == StreamKind::Live && !staged.approved_for_stream() {
            return Err(SegmentError::NotApproved {
                case_id: id.to_string(),
            });
        }
        Ok(staged)
    }

    /// Stage a researched case: writes `case.json`, `records.json` and `meta.json` (status
    /// `CALM_STAGED`, not approved, `content_sha256` computed here). Refuses an invalid case
    /// and refuses to overwrite a case the owner approved.
    pub fn stage(
        &self,
        case: &Case,
        file: &CaseFile,
        mut meta: CaseMeta,
    ) -> Result<PathBuf, SegmentError> {
        let dir = self.dir(&case.id)?;
        if case.fictional || file.fictional {
            return Err(SegmentError::Config {
                detail: "only real cases are staged in the case store".into(),
            });
        }
        validate_case(case)?;
        file.validate_with(case, &self.allowlist, &self.research)?;
        if self.exists(&case.id) && self.meta(&case.id)?.owner_approved_for_stream {
            return Err(SegmentError::Config {
                detail: format!(
                    "case {} is approved for stream; retract it or pick another id before \
                     re-staging",
                    case.id
                ),
            });
        }
        meta.case_id = case.id.clone();
        meta.status = CaseStatus::CalmStaged;
        meta.owner_approved_for_stream = false;
        meta.approved_at_utc = None;
        meta.approved_content_sha256 = None;
        meta.content_sha256 = content_sha256(case, file);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        write_json(&dir.join(CASE_FILE), case)?;
        write_json(&dir.join(RECORDS_FILE), file)?;
        // The meta last: a case without one is not listed.
        write_json(&dir.join(META_FILE), &meta)?;
        Ok(dir)
    }

    /// The owner approves the case, as it is now, for a live stream.
    pub fn approve(&self, id: &str, now_ms: i64) -> Result<CaseMeta, SegmentError> {
        let staged = self.load(id)?;
        if staged.meta.status == CaseStatus::Retracted {
            return Err(SegmentError::Config {
                detail: format!("case {id} is retracted and cannot be approved"),
            });
        }
        let mut meta = staged.meta;
        meta.owner_approved_for_stream = true;
        meta.approved_at_utc = Some(utc_iso(now_ms));
        meta.approved_content_sha256 = Some(staged.content_sha256);
        write_json(&self.dir(id)?.join(META_FILE), &meta)?;
        Ok(meta)
    }

    /// Withdraw a case: it runs nowhere and its approval is void.
    pub fn retract(&self, id: &str) -> Result<CaseMeta, SegmentError> {
        let mut meta = self.meta(id)?;
        meta.status = CaseStatus::Retracted;
        meta.owner_approved_for_stream = false;
        meta.approved_content_sha256 = None;
        write_json(&self.dir(id)?.join(META_FILE), &meta)?;
        Ok(meta)
    }

    /// The corrections / right-of-reply log, oldest first. A malformed line is an error.
    pub fn corrections(&self, id: &str) -> Result<Vec<CorrectionEntry>, SegmentError> {
        let path = self.dir(id)?.join(CORRECTIONS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&path, e)),
        };
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(n, l)| {
                serde_json::from_str(l).map_err(|e| SegmentError::Data {
                    file: format!("{}:{}", path.display(), n + 1),
                    detail: e.to_string(),
                })
            })
            .collect()
    }

    /// Append one entry to the log (flushed to disk before returning). The note is public text:
    /// it must name no contact detail.
    pub fn append_correction(
        &self,
        id: &str,
        hex_id: Option<&str>,
        entry_type: CorrectionType,
        public_note: &str,
        now_ms: i64,
    ) -> Result<CorrectionEntry, SegmentError> {
        let staged = self.load(id)?;
        let bad = |detail: String| SegmentError::Config {
            detail: format!("case {id}: {detail}"),
        };
        let note = public_note.trim();
        if note.is_empty() {
            return Err(bad("an empty public note".into()));
        }
        if let Some(what) = crate::research::contact_detail(note) {
            return Err(bad(format!("{what} in the public note")));
        }
        let hex_id = match hex_id {
            Some(h) => {
                let canon = crate::normalize_hex(h).unwrap_or_else(|| h.to_string());
                if !staged.case.hexes.iter().any(|x| x.id == canon) {
                    return Err(bad(format!("unknown hex {h}")));
                }
                Some(canon)
            }
            None => None,
        };
        let n = self.corrections(id)?.len() + 1;
        let entry = CorrectionEntry {
            entry_id: format!("{id}-C{n:03}"),
            case_id: id.to_string(),
            hex_id,
            entry_type,
            public_note: note.to_string(),
            blocks_clip_publishing: true,
            logged_at_utc: utc_iso(now_ms),
        };
        let path = self.dir(id)?.join(CORRECTIONS_FILE);
        let mut line = serde_json::to_string(&entry).map_err(|e| io(&path, e))?;
        line.push('\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        f.write_all(line.as_bytes()).map_err(|e| io(&path, e))?;
        f.sync_all().map_err(|e| io(&path, e))?;
        Ok(entry)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::research::CaseRecord;
    use shadowrunr_types::{EvidenceGrade, Hex, HexKind};

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn record(id: &str, hex: &str, grade: EvidenceGrade, source: &str) -> CaseRecord {
        CaseRecord {
            id: id.into(),
            hex: hex.into(),
            grade,
            title: format!("Record {id}"),
            source: source.into(),
            summary: format!("The regulator's filing alleges item {id}."),
            retrieved: Some("2026-10-19".into()),
            content_hash: Some(HASH.into()),
            via: None,
            date: None,
        }
    }

    /// A small, well-formed real case (test data: the URLs need not resolve).
    pub(crate) fn real_case() -> (Case, CaseFile) {
        let hex = |id: &str, q: i32, r: i32, kind: HexKind, label: &str, src: &str| Hex {
            id: id.into(),
            q,
            r,
            kind,
            label: label.into(),
            grade: None,
            fog: true,
            sources: vec![src.into()],
            role: None,
            date: None,
        };
        let case = Case {
            id: "TEST-REAL-01".into(),
            title: "Test Holdings LLC (test fixture)".into(),
            fictional: false,
            hexes: vec![
                hex(
                    "HEX-01",
                    0,
                    0,
                    HexKind::Entity,
                    "Test Holdings LLC",
                    "https://www.sec.gov/litigation/complaints/2019/comp-1.pdf",
                ),
                hex(
                    "HEX-02",
                    1,
                    0,
                    HexKind::Claim,
                    "Promised 7% fixed returns",
                    "https://www.sec.gov/news/press-release/2019-1",
                ),
                hex(
                    "HEX-03",
                    1,
                    -1,
                    HexKind::ShellCompany,
                    "Test Fund 1 LLC",
                    "https://opencorporates.com/companies/us_de/1",
                ),
                hex(
                    "HEX-04",
                    0,
                    -1,
                    HexKind::Domain,
                    "testholdings.example",
                    "https://web.archive.org/web/2018id_/https://testholdings.example/",
                ),
                hex(
                    "HEX-05",
                    -1,
                    0,
                    HexKind::Entity,
                    "Final judgment",
                    "https://www.sec.gov/litigation/litreleases/2019/lr1.htm",
                ),
            ],
            links: vec![("HEX-01".into(), "HEX-03".into())],
            thesis: None,
            acts: Vec::new(),
        };
        let mut wayback = record(
            "CLM-01",
            "HEX-04",
            EvidenceGrade::Clm,
            "https://testholdings.example/",
        );
        wayback.via = Some(
            "https://web.archive.org/web/20180101000000id_/https://testholdings.example/".into(),
        );
        let mut archived_doc = record(
            "DOC-02",
            "HEX-05",
            EvidenceGrade::Doc,
            "https://www.sec.gov/litigation/litreleases/2019/lr1.htm",
        );
        archived_doc.via = Some(
            "https://web.archive.org/web/20190301000000id_/https://www.sec.gov/litigation/litreleases/2019/lr1.htm"
                .into(),
        );
        let file = CaseFile {
            case_id: case.id.clone(),
            fictional: false,
            notice: "Public records staged off-stream.".into(),
            records: vec![
                record(
                    "DOC-01",
                    "HEX-01",
                    EvidenceGrade::Doc,
                    "https://www.sec.gov/news/press-release/2019-1",
                ),
                record(
                    "REP-01",
                    "HEX-02",
                    EvidenceGrade::Rep,
                    "https://opencorporates.com/companies/us_de/1",
                ),
                wayback,
                archived_doc,
            ],
            arc: vec![
                vec!["HEX-01".into(), "HEX-02".into()],
                vec!["HEX-03".into(), "HEX-04".into()],
                vec!["HEX-05".into()],
            ],
        };
        (case, file)
    }

    pub(crate) fn meta_for(case: &Case) -> CaseMeta {
        CaseMeta {
            case_id: case.id.clone(),
            title: case.title.clone(),
            subject: "Test Holdings LLC".into(),
            subject_category: SubjectCategory::PublicEnforcementEntity,
            primary_public_record_url: "https://www.sec.gov/news/press-release/2019-1".into(),
            status: CaseStatus::CalmStaged,
            owner_approved_for_stream: true, // stage() resets it
            approved_at_utc: None,
            approved_content_sha256: None,
            created_at_utc: utc_iso(0),
            content_sha256: String::new(),
            research: ResearchProvenance::default(),
        }
    }

    pub(crate) fn staged_store(dir: &Path) -> CaseStore {
        let store = CaseStore::new(dir);
        let (case, file) = real_case();
        store.stage(&case, &file, meta_for(&case)).unwrap();
        store
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(utc_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_iso(1_792_415_751_000), "2026-10-19T13:15:51Z");
        assert_eq!(utc_iso(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn case_ids() {
        for ok in ["WOODBRIDGE-2019", "ABC", "S01-C03-X"] {
            assert!(check_case_id(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "ab",
            "lower-case",
            "1ABC",
            "A/B",
            "../X",
            "WITH SPACE",
            "A_B",
        ] {
            assert!(check_case_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn staging_writes_an_unapproved_calm_case() {
        let d = tempfile::tempdir().unwrap();
        let store = staged_store(d.path());
        assert_eq!(store.ids(), ["TEST-REAL-01"]);
        let staged = store.load("TEST-REAL-01").unwrap();
        assert_eq!(staged.meta.status, CaseStatus::CalmStaged);
        assert!(!staged.meta.owner_approved_for_stream);
        assert!(!staged.approved_for_stream());
        assert_eq!(staged.content_sha256, staged.meta.content_sha256);
        assert_eq!(staged.content_sha256.len(), 64);
        let text = std::fs::read_to_string(d.path().join("cases/TEST-REAL-01/meta.json")).unwrap();
        assert!(
            text.contains("\"subject_category\": \"PUBLIC_ENFORCEMENT_ENTITY\""),
            "{text}"
        );
        assert!(text.contains("\"status\": \"CALM_STAGED\""), "{text}");
        assert!(!d.path().join("cases/TEST-REAL-01/meta.json.tmp").exists());
    }

    #[test]
    fn a_live_stream_needs_the_owners_approval_of_this_content() {
        let d = tempfile::tempdir().unwrap();
        let store = staged_store(d.path());
        let id = "TEST-REAL-01";
        assert!(matches!(
            store.open_for_stream(id, StreamKind::Live),
            Err(SegmentError::NotApproved { .. })
        ));
        // A private sim may run it.
        assert!(store.open_for_stream(id, StreamKind::PrivateSim).is_ok());
        let meta = store.approve(id, 1_792_415_751_000).unwrap();
        assert!(meta.owner_approved_for_stream);
        assert_eq!(
            meta.approved_at_utc.as_deref(),
            Some("2026-10-19T13:15:51Z")
        );
        assert!(store.open_for_stream(id, StreamKind::Live).is_ok());
        // Approved content cannot be re-staged in place.
        let (case, file) = real_case();
        assert!(store.stage(&case, &file, meta_for(&case)).is_err());
        // Content edited after approval: the files no longer match the meta.
        let path = d.path().join("cases/TEST-REAL-01/records.json");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("item DOC-01", "item DOC-01 (edited)")).unwrap();
        assert!(store.open_for_stream(id, StreamKind::Live).is_err());
        assert!(store.open_for_stream(id, StreamKind::PrivateSim).is_err());
        std::fs::write(&path, text).unwrap();
        // Retracted: nowhere, and no approval.
        store.retract(id).unwrap();
        assert!(store.open_for_stream(id, StreamKind::PrivateSim).is_err());
        assert!(store.approve(id, 0).is_err());
    }

    #[test]
    fn stage_refuses_unsound_cases() {
        let d = tempfile::tempdir().unwrap();
        let store = CaseStore::new(d.path());
        let (case, file) = real_case();
        let try_stage = |case: &Case, file: &CaseFile| store.stage(case, file, meta_for(case));
        let mut fictional = case.clone();
        fictional.fictional = true;
        assert!(try_stage(&fictional, &file).is_err());
        // A record fetched off the research allowlist.
        let mut f = file.clone();
        f.records[1].source = "https://en.wikipedia.org/wiki/X".into();
        assert!(try_stage(&case, &f).is_err());
        // A DOC on a research (not primary) source.
        let mut f = file.clone();
        f.records[1].grade = EvidenceGrade::Doc;
        f.records[1].id = "DOC-09".into();
        assert!(try_stage(&case, &f).is_err());
        // A Wayback `via` that is not a capture of the source.
        let mut f = file.clone();
        f.records[2].via =
            Some("https://web.archive.org/web/2018id_/https://other.example/".into());
        assert!(try_stage(&case, &f).is_err());
        // No retrieval day or content hash.
        let mut f = file.clone();
        f.records[0].retrieved = None;
        assert!(try_stage(&case, &f).is_err());
        let mut f = file.clone();
        f.records[0].content_hash = Some("abc".into());
        assert!(try_stage(&case, &f).is_err());
        // Contact details left in the text.
        let mut f = file.clone();
        f.records[0].summary = "Write to investor.relations@test.example for refunds.".into();
        assert!(try_stage(&case, &f).is_err());
        let mut f = file.clone();
        f.records[0].summary = "Call (555) 010-4477 for a refund.".into();
        assert!(try_stage(&case, &f).is_err());
        // An arc naming a hex twice.
        let mut f = file.clone();
        f.arc[2].push("HEX-01".into());
        assert!(try_stage(&case, &f).is_err());
        // The sound case stages.
        assert!(try_stage(&case, &file).is_ok());
    }

    #[test]
    fn corrections_append_and_read_back() {
        let d = tempfile::tempdir().unwrap();
        let store = staged_store(d.path());
        let id = "TEST-REAL-01";
        assert!(store.corrections(id).unwrap().is_empty());
        let a = store
            .append_correction(
                id,
                Some("hex2"),
                CorrectionType::GradeDowngrade,
                "REP, not DOC.",
                0,
            )
            .unwrap();
        assert_eq!(a.entry_id, "TEST-REAL-01-C001");
        assert_eq!(a.hex_id.as_deref(), Some("HEX-02"));
        assert!(a.blocks_clip_publishing);
        let b = store
            .append_correction(
                id,
                None,
                CorrectionType::RightOfReplyStatement,
                "The company replied.",
                0,
            )
            .unwrap();
        assert_eq!(b.entry_id, "TEST-REAL-01-C002");
        assert_eq!(store.corrections(id).unwrap(), vec![a, b]);
        assert!(store
            .append_correction(
                id,
                Some("HEX-99"),
                CorrectionType::FactualCorrection,
                "x",
                0
            )
            .is_err());
        assert!(store
            .append_correction(id, None, CorrectionType::FactualCorrection, "  ", 0)
            .is_err());
        assert!(store
            .append_correction(
                id,
                None,
                CorrectionType::FactualCorrection,
                "mail ceo@test.example",
                0
            )
            .is_err());
    }

    #[test]
    fn contact_details_are_spotted() {
        use crate::research::contact_detail;
        assert_eq!(contact_detail("reach a@b.co now"), Some("an email address"));
        assert_eq!(contact_detail("call 555-010-4477"), Some("a phone number"));
        assert_eq!(
            contact_detail("call +44 20 7946 0958"),
            Some("a phone number")
        );
        for clean in [
            "The SEC obtained a $1.2 billion judgment in 2019.",
            "Case 1:17-cv-24624 (S.D. Fla.)",
            "Release No. 34-82354; 275 companies",
            "@handle on a forum",
        ] {
            assert_eq!(contact_detail(clean), None, "{clean}");
        }
    }
}
