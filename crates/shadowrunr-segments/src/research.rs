//! SEG-02A research: the case file (planted records of a fictional case, or the staged public
//! records of a real one), which hex a research brief is about, and the bone-clerks' report.
//!
//! A fictional case has nothing to find on the real web, so the bone-clerks used to come back
//! empty and Malakor looped on the fog. The demo case therefore ships a pack of planted
//! FICTIONAL records (`data/demo-case-records.json`) graded with the usual vocabulary (DOC,
//! REP, CLM; never SPC, which is Malakor's own speculation). A research brief that names a hex
//! returns that hex's records; some hexes carry none, so "nothing on file" still happens.
//!
//! A real case (`fictional: false`) carries the public records the off-stream research
//! pipeline staged (`shadowrunr case research`, see [`crate::cases`]): every record names the
//! URL it was published at, the day it was retrieved, the sha256 of the bytes retrieved and,
//! when the publisher's own site refused the fetch, the Wayback Machine capture it came
//! through. The bone-clerks then read those records, never the live web.
//!
//! The re-search cap itself (`max_searches_per_hex`, in the segment table's `[clock]`) is
//! enforced by [`crate::SegmentRuntime`]; [`report`] only words the outcome.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Case, EvidenceGrade};

use crate::command::normalize_hex;
use crate::evidence::{url_host, wayback_original, ResearchAllowlist, SourceAllowlist};
use crate::SegmentError;

/// The compiled-in planted records of the fictional demo case: the exact bytes of
/// `data/demo-case-records.json`.
pub const DEMO_RECORDS_JSON: &str = include_str!("../data/demo-case-records.json");

/// A report is cut to this many characters: the researcher's own limit, and the brain's cut
/// for a job summary.
const REPORT_CHARS: usize = 600;

/// Label words too common across the map to say which hex a brief means.
const STOP_WORDS: [&str; 10] = [
    "fictional",
    "wallet",
    "from",
    "with",
    "that",
    "this",
    "same",
    "cannot",
    "their",
    "they",
];

/// One record of a case file: planted (a fictional case) or staged from the public record (a
/// real case).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRecord {
    /// `DOC-01`, `REP-02`, `CLM-03`: the prefix is the grade.
    pub id: String,
    /// The hex this record is filed under.
    pub hex: String,
    pub grade: EvidenceGrade,
    pub title: String,
    /// Where the record was published: a reserved `*.example` URL in a fictional case, the
    /// publisher's own URL in a real one. A DOC record's source is a primary source.
    pub source: String,
    pub summary: String,
    /// Real cases: the UTC day the record was retrieved, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved: Option<String>,
    /// Real cases: sha256 (lowercase hex) of the bytes retrieved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Real cases: the Wayback Machine capture the record was retrieved through, when the
    /// publisher's site refused the fetch. Its original URL is `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

impl CaseRecord {
    /// The URL the record's bytes were actually fetched from.
    pub fn fetched_from(&self) -> &str {
        self.via.as_deref().unwrap_or(&self.source)
    }
}

/// The records of one case: planted for a fictional case, staged from the public record for a
/// real one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseFile {
    /// The case these records belong to.
    pub case_id: String,
    /// Must match the case: planted records for a fictional case, public records for a real one.
    pub fictional: bool,
    /// Human-readable notice: that everything is invented (fictional), or where the records
    /// come from and how they may be spoken (real).
    pub notice: String,
    pub records: Vec<CaseRecord>,
    /// The case's arc: hex ids per act, in act order (SEG-02A's `[arc]`). Empty: the runtime
    /// assigns acts by hex kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arc: Vec<Vec<String>>,
}

impl CaseFile {
    /// Parse a case file. Structural checks only; [`CaseFile::validate`] checks it against
    /// its case.
    pub fn parse(file: &str, text: &str) -> Result<Self, SegmentError> {
        serde_json::from_str(text).map_err(|e| SegmentError::Data {
            file: file.into(),
            detail: e.to_string(),
        })
    }

    pub fn load(path: &Path) -> Result<Self, SegmentError> {
        let text = std::fs::read_to_string(path).map_err(|e| SegmentError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        Self::parse(&path.display().to_string(), &text)
    }

    /// The planted records of the fictional demo case.
    pub fn demo() -> Self {
        Self::parse("demo-case-records.json", DEMO_RECORDS_JSON)
            .expect("built-in demo case records are valid (tested)")
    }

    /// The file fits `case`: same id and the same `fictional` flag, unique ids whose prefix is
    /// the grade, no SPC, every record on a hex of the case, non-empty text, and every DOC
    /// record backed by a primary source (so finding it makes a DOC reveal possible under the
    /// existing rule). A fictional file's sources sit on reserved `*.example` domains. A real
    /// file's records each carry their retrieval day and content hash, were fetched from the
    /// research allowlist (`via` must be a Wayback capture of `source`), and carry no email
    /// address or phone number. The arc, when present, names each hex at most once.
    pub fn validate(&self, case: &Case, allowlist: &SourceAllowlist) -> Result<(), SegmentError> {
        self.validate_with(case, allowlist, ResearchAllowlist::builtin())
    }

    /// [`CaseFile::validate`] with the research allowlist the case was staged under.
    pub fn validate_with(
        &self,
        case: &Case,
        allowlist: &SourceAllowlist,
        research: &ResearchAllowlist,
    ) -> Result<(), SegmentError> {
        let bad = |detail: String| SegmentError::Config {
            detail: format!("case file {}: {detail}", self.case_id),
        };
        if self.case_id != case.id {
            return Err(bad(format!("belongs to another case than {}", case.id)));
        }
        if self.fictional != case.fictional {
            return Err(bad(if self.fictional {
                "planted records are only allowed in a fictional case".into()
            } else {
                "public records belong to a real case, not a fictional one".into()
            }));
        }
        let hexes: BTreeSet<&str> = case.hexes.iter().map(|h| h.id.as_str()).collect();
        let mut ids = BTreeSet::new();
        for r in &self.records {
            if !ids.insert(r.id.as_str()) {
                return Err(bad(format!("duplicate record id {}", r.id)));
            }
            if r.grade == EvidenceGrade::Spc {
                return Err(bad(format!("{}: SPC is speculation, not a record", r.id)));
            }
            if !r.id.starts_with(&format!("{}-", grade_name(r.grade))) {
                return Err(bad(format!("{}: the id prefix must be its grade", r.id)));
            }
            if !hexes.contains(r.hex.as_str()) {
                return Err(bad(format!("{}: unknown hex {}", r.id, r.hex)));
            }
            if r.title.trim().is_empty() || r.summary.trim().is_empty() {
                return Err(bad(format!("{}: empty title or summary", r.id)));
            }
            if self.fictional {
                if !url_host(&r.source).is_some_and(|h| h.ends_with(".example")) {
                    return Err(bad(format!(
                        "{}: source must be on a reserved *.example domain",
                        r.id
                    )));
                }
            } else {
                real_record_ok(r, research).map_err(|why| bad(format!("{}: {why}", r.id)))?;
            }
            if r.grade == EvidenceGrade::Doc && !allowlist.is_primary(&r.source, self.fictional) {
                return Err(bad(format!(
                    "{}: a DOC record needs a primary source",
                    r.id
                )));
            }
        }
        let mut staged = BTreeSet::new();
        for id in self.arc.iter().flatten() {
            if !hexes.contains(id.as_str()) {
                return Err(bad(format!("arc names unknown hex {id}")));
            }
            if !staged.insert(id.as_str()) {
                return Err(bad(format!("arc names {id} twice")));
            }
        }
        Ok(())
    }

    /// The records filed under `hex`, in file order.
    pub fn records_for<'a>(&'a self, hex: &'a str) -> impl Iterator<Item = &'a CaseRecord> + 'a {
        self.records.iter().filter(move |r| r.hex == hex)
    }
}

/// The provenance checks of one real record (see [`CaseFile::validate`]).
fn real_record_ok(r: &CaseRecord, research: &ResearchAllowlist) -> Result<(), String> {
    let source = r.source.trim().to_ascii_lowercase();
    if !(source.starts_with("https://") || source.starts_with("http://"))
        || url_host(&r.source).is_none()
    {
        return Err("source must be an absolute http(s) URL".into());
    }
    if let Some(via) = &r.via {
        if wayback_original(via).as_deref() != Some(r.source.as_str()) {
            return Err("via must be a Wayback capture of the source".into());
        }
    }
    if !research.allows(r.fetched_from()) {
        return Err(format!(
            "{} is not on the research allowlist",
            r.fetched_from()
        ));
    }
    let day_ok = r.retrieved.as_deref().is_some_and(|d| {
        let b = d.as_bytes();
        b.len() == 10
            && b.iter().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    *c == b'-'
                } else {
                    c.is_ascii_digit()
                }
            })
    });
    if !day_ok {
        return Err("a real record needs its retrieval day (YYYY-MM-DD)".into());
    }
    if !r
        .content_hash
        .as_deref()
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("a real record needs the sha256 of the bytes retrieved".into());
    }
    for text in [&r.title, &r.summary] {
        if let Some(what) = contact_detail(text) {
            return Err(format!("{what} in the text (scrub it before staging)"));
        }
    }
    Ok(())
}

/// The first contact detail in `text`: an email address, or a phone number (digits in 3-3-4
/// groups, or a `+` followed by 10 or more digits). A last mechanical check on staged text;
/// the research pipeline's scrubber is the real one.
pub fn contact_detail(text: &str) -> Option<&'static str> {
    for w in text.split_whitespace() {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        if let Some((user, domain)) = w.split_once('@') {
            if !user.is_empty() && domain.contains('.') && !domain.starts_with('.') {
                return Some("an email address");
            }
        }
    }
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_digit() || b[i] == b'+' || b[i] == b'(') {
            i += 1;
            continue;
        }
        // One run of digits and phone punctuation.
        let plus = b[i] == b'+';
        let mut digits = 0usize;
        let mut groups: Vec<usize> = Vec::new();
        let mut cur = 0usize;
        while i < b.len()
            && (b[i].is_ascii_digit() || matches!(b[i], b'+' | b'(' | b')' | b'-' | b'.' | b' '))
        {
            if b[i].is_ascii_digit() {
                digits += 1;
                cur += 1;
            } else if cur > 0 {
                groups.push(cur);
                cur = 0;
            }
            i += 1;
        }
        if cur > 0 {
            groups.push(cur);
        }
        if groups.windows(3).any(|g| g == [3, 3, 4]) || (plus && digits >= 10) {
            return Some("a phone number");
        }
    }
    None
}

/// `DOC`, `REP`, `CLM`, `SPC`.
pub fn grade_name(g: EvidenceGrade) -> &'static str {
    match g {
        EvidenceGrade::Doc => "DOC",
        EvidenceGrade::Rep => "REP",
        EvidenceGrade::Clm => "CLM",
        EvidenceGrade::Spc => "SPC",
    }
}

/// Lowercase words of `s`: runs of letters, digits, `-` and `.`, trimmed of edge `-`/`.` so
/// identifiers such as `demo-0001` and `glimmerquay-yield.example` stay whole.
fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '.'))
        .map(|w| w.trim_matches(|c| c == '-' || c == '.'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// An identifier-like word (a wallet id, a domain) says much more than a plain word.
fn is_identifier(w: &str) -> bool {
    w.contains(['-', '.']) || w.bytes().any(|b| b.is_ascii_digit())
}

/// The hex a research brief is about, as an index into `case.hexes`.
///
/// An explicit hex id wins: the first `HEX-07`, `hex7` or `hex 7` that names a hex on the map.
/// Otherwise the hex whose label the brief echoes best (an identifier-like word counts three,
/// a plain word of four letters or more one; words common across the map do not count). No
/// match, or a tie at the top, is `None`: the brief targets no single hex.
pub fn target_hex(case: &Case, brief: &str) -> Option<usize> {
    let ws = words(brief);
    for (k, w) in ws.iter().enumerate() {
        let candidate = if w == "hex" {
            ws.get(k + 1).map(|n| format!("hex{n}"))
        } else if w.starts_with("hex") {
            Some(w.clone())
        } else {
            None
        };
        if let Some(id) = candidate.as_deref().and_then(normalize_hex) {
            if let Some(i) = case.hexes.iter().position(|h| h.id == id) {
                return Some(i);
            }
        }
    }
    let said: BTreeSet<&str> = ws.iter().map(String::as_str).collect();
    let mut best: Option<(usize, u32)> = None;
    let mut tied = false;
    for (i, h) in case.hexes.iter().enumerate() {
        let label: BTreeSet<String> = words(&h.label).into_iter().collect();
        let score: u32 = label
            .iter()
            .filter(|w| said.contains(w.as_str()) && !STOP_WORDS.contains(&w.as_str()))
            .map(|w| match (is_identifier(w), w.chars().count() >= 4) {
                (true, _) => 3,
                (false, true) => 1,
                (false, false) => 0,
            })
            .sum();
        match best {
            _ if score == 0 => {}
            Some((_, s)) if score < s => {}
            Some((_, s)) if score == s => tied = true,
            _ => {
                best = Some((i, score));
                tied = false;
            }
        }
    }
    best.filter(|_| !tied).map(|(i, _)| i)
}

/// The researchers' report on `brief`: what a research job returns for a case with a planted
/// case file. `prior_searches` is how many searches the brief's hex already had this stream
/// and `cap` the table's `max_searches_per_hex`; at the cap the report says the next move is
/// the Plebiscite's instead of repeating the file. `researchers` is the persona's word for
/// them (`{researchers}`, e.g. "bone-clerks").
pub fn report(
    case: &Case,
    file: &CaseFile,
    brief: &str,
    prior_searches: u32,
    cap: u32,
    researchers: &str,
) -> String {
    let Some(i) = target_hex(case, brief) else {
        return format!(
            "The {researchers} could not tell which hex this brief means. Name exactly one \
             hex from the map (\"HEX-04: ...\")."
        );
    };
    let hex = &case.hexes[i];
    if prior_searches >= cap {
        return format!(
            "{}: already searched {cap} times this stream. The {researchers} will not search it \
             again; the next move on {} goes to the Plebiscite.",
            hex.id, hex.id
        );
    }
    // Titles stay out (the state summary carries them) so a full hex fits the job summary.
    let found: Vec<String> = file
        .records_for(&hex.id)
        .map(|r| {
            format!(
                "{} [{}] {} ({})",
                r.id,
                grade_name(r.grade),
                r.summary,
                r.source
            )
        })
        .collect();
    if found.is_empty() {
        return if file.fictional {
            format!(
                "{} ({}): the {researchers} searched the case file and found nothing on record. \
                 PLANTED FICTIONAL DEMO CASE.",
                hex.id, hex.label
            )
        } else {
            format!(
                "{} ({}): the {researchers} searched the staged public record and found nothing \
                 filed under this hex. Do not fill the gap: it stays unproven.",
                hex.id, hex.label
            )
        };
    }
    let kind = if file.fictional {
        "planted FICTIONAL demo record(s)"
    } else {
        "public record(s)"
    };
    format!(
        "{} ({}): {} {kind} on file. {}",
        hex.id,
        hex.label,
        found.len(),
        found.join(" | ")
    )
    .chars()
    .take(REPORT_CHARS)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::demo_case;

    fn idx(case: &Case, id: &str) -> usize {
        case.hexes.iter().position(|h| h.id == id).unwrap()
    }

    #[test]
    fn demo_records_fit_the_demo_case() {
        let case = demo_case();
        let file = CaseFile::demo();
        file.validate(&case, SourceAllowlist::builtin()).unwrap();
        assert!(file.fictional);
        assert!(file.notice.contains("FICTIONAL"));
        assert!(file.records.len() >= 10, "{}", file.records.len());
        for r in &file.records {
            assert!(r.title.contains("(fictional)"), "{}", r.id);
        }
        // Some hexes stay empty so "nothing found" still happens.
        let empty: Vec<&str> = case
            .hexes
            .iter()
            .filter(|h| file.records_for(&h.id).next().is_none())
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(empty, ["HEX-08", "HEX-10", "HEX-12", "HEX-14", "HEX-16"]);
        // Every grade but SPC is planted.
        for g in [EvidenceGrade::Doc, EvidenceGrade::Rep, EvidenceGrade::Clm] {
            assert!(file.records.iter().any(|r| r.grade == g), "{g:?}");
        }
    }

    #[test]
    fn bad_case_files_rejected() {
        let case = demo_case();
        let list = SourceAllowlist::builtin();
        let reject = |text: String| {
            let f = CaseFile::parse("x", &text).unwrap();
            assert!(f.validate(&case, list).is_err(), "{text}");
        };
        reject(DEMO_RECORDS_JSON.replace("\"fictional\": true", "\"fictional\": false"));
        reject(DEMO_RECORDS_JSON.replace("DEMO-C01-GLIMMERQUAY", "OTHER-CASE"));
        reject(DEMO_RECORDS_JSON.replace("\"hex\": \"HEX-15\"", "\"hex\": \"HEX-99\""));
        reject(DEMO_RECORDS_JSON.replace("\"id\": \"REP-02\"", "\"id\": \"REP-01\""));
        reject(DEMO_RECORDS_JSON.replace("\"id\": \"REP-02\"", "\"id\": \"DOC-09\""));
        reject(DEMO_RECORDS_JSON.replace(
            "https://news.example/pebblecourt-processing",
            "https://news.real.com/pebblecourt",
        ));
        // A DOC record must sit on a primary source.
        reject(DEMO_RECORDS_JSON.replace(
            "https://records.example/morrowgate-fco/warning-list-2026",
            "https://news.example/warning-list",
        ));
        // A real case never takes planted records.
        let mut real = demo_case();
        real.fictional = false;
        assert!(CaseFile::demo().validate(&real, list).is_err());
        // SPC and unknown fields do not parse into a valid file.
        let spc = DEMO_RECORDS_JSON
            .replace("\"id\": \"CLM-05\"", "\"id\": \"SPC-01\"")
            .replace(
                "\"hex\": \"HEX-11\", \"grade\": \"CLM\"",
                "\"hex\": \"HEX-11\", \"grade\": \"SPC\"",
            );
        reject(spc);
        assert!(CaseFile::parse("x", "{\"case_id\": \"x\"}").is_err());
    }

    #[test]
    fn briefs_resolve_to_hexes() {
        let case = demo_case();
        let at = |brief: &str| target_hex(&case, brief).map(|i| case.hexes[i].id.as_str());
        // Explicit ids, in every spelling the chat parser takes.
        assert_eq!(at("HEX-04: trace the treasury wallet"), Some("HEX-04"));
        assert_eq!(at("dig into hex9 please"), Some("HEX-09"));
        assert_eq!(at("what about hex 13?"), Some("HEX-13"));
        // An id off the map falls through to the label.
        assert_eq!(
            at("HEX-77 or the Ashpetal Holdings registry"),
            Some("HEX-03")
        );
        // Labels: identifiers weigh most.
        assert_eq!(
            at("Archive snapshot of glimmerquay-yield.example"),
            Some("HEX-02")
        );
        assert_eq!(
            at("Trace the hops out of treasury wallet demo-0001"),
            Some("HEX-04")
        );
        assert_eq!(
            at("Who runs the Glimmerquay Yield Syndicate?"),
            Some("HEX-01")
        );
        assert_eq!(at("Registry extract for Ashpetal Holdings"), Some("HEX-03"));
        // Nothing, only common words, or a tie: no single hex.
        assert_eq!(at("map the shell chain"), None);
        assert_eq!(at("follow every wallet"), None);
        assert_eq!(at("Mirecrown and Ashpetal"), None);
        assert_eq!(at(""), None);
    }

    #[test]
    fn report_returns_records_for_seeded_hexes() {
        let case = demo_case();
        let file = CaseFile::demo();
        let r = report(
            &case,
            &file,
            "HEX-04: trace the treasury wallet",
            0,
            2,
            "bone-clerks",
        );
        assert!(r.starts_with("HEX-04 (Treasury wallet demo-0001): 2 planted FICTIONAL"));
        assert!(r.contains("DOC-03 [DOC]"), "{r}");
        assert!(r.contains("CLM-02 [CLM]"), "{r}");
        assert!(r.contains("https://records.example/morrowgate-fco/freeze-0003"));
        assert_eq!(idx(&case, "HEX-04"), target_hex(&case, "hex4").unwrap());
    }

    #[test]
    fn report_says_nothing_found_for_empty_hexes() {
        let case = demo_case();
        let file = CaseFile::demo();
        let r = report(
            &case,
            &file,
            "HEX-08: who controls Mirecrown Trust",
            1,
            2,
            "bone-clerks",
        );
        assert!(r.contains("found nothing on record"), "{r}");
        assert!(!r.contains("[DOC]") && !r.contains("[REP]") && !r.contains("[CLM]"));
    }

    #[test]
    fn report_at_the_cap_points_to_the_plebiscite() {
        let case = demo_case();
        let file = CaseFile::demo();
        let r = report(&case, &file, "HEX-07 again", 2, 2, "bone-clerks");
        assert!(r.contains("will not search it again"), "{r}");
        assert!(r.contains("Plebiscite"), "{r}");
        assert!(!r.contains("CLM-03"), "the file is not repeated: {r}");
        let r = report(&case, &file, "the usual suspects", 5, 2, "bone-clerks");
        assert!(r.contains("Name exactly one hex"), "{r}");
    }

    #[test]
    fn every_report_fits_a_job_summary_whole() {
        let case = demo_case();
        let file = CaseFile::demo();
        for h in &case.hexes {
            let r = report(
                &case,
                &file,
                &format!("{}: everything", h.id),
                0,
                2,
                "bone-clerks",
            );
            assert!(r.chars().count() < REPORT_CHARS, "{} is cut: {r}", h.id);
            for rec in file.records_for(&h.id) {
                assert!(r.contains(&rec.source), "{}", rec.id);
            }
        }
    }

    #[test]
    fn a_real_case_reports_its_public_records() {
        let (case, file) = crate::cases::tests::real_case();
        file.validate(&case, SourceAllowlist::builtin()).unwrap();
        let r = report(&case, &file, "HEX-01: who runs it", 0, 2, "bone-clerks");
        assert!(
            r.starts_with("HEX-01 (Test Holdings LLC): 1 public record(s) on file."),
            "{r}"
        );
        assert!(r.contains("DOC-01 [DOC]"), "{r}");
        assert!(!r.contains("FICTIONAL"), "{r}");
        let r = report(&case, &file, "HEX-03: the fund", 0, 2, "bone-clerks");
        assert!(r.contains("found nothing filed under this hex"), "{r}");
        assert!(!r.contains("FICTIONAL"), "{r}");
        // A DOC through a Wayback capture keeps the publisher's URL as its source.
        let doc = file.records.iter().find(|r| r.id == "DOC-02").unwrap();
        assert!(doc
            .fetched_from()
            .starts_with("https://web.archive.org/web/"));
        assert!(SourceAllowlist::builtin().is_primary(&doc.source, false));
    }

    #[test]
    fn a_real_file_never_fits_a_fictional_case_and_back() {
        let (case, file) = crate::cases::tests::real_case();
        let mut fictional = case.clone();
        fictional.fictional = true;
        assert!(file
            .validate(&fictional, SourceAllowlist::builtin())
            .is_err());
        // Real records need their provenance.
        let mut bare = file.clone();
        bare.records[0].content_hash = None;
        assert!(bare.validate(&case, SourceAllowlist::builtin()).is_err());
    }

    #[test]
    fn report_is_deterministic() {
        let case = demo_case();
        let file = CaseFile::demo();
        for h in &case.hexes {
            let brief = format!("{}: everything", h.id);
            assert_eq!(
                report(&case, &file, &brief, 0, 2, "bone-clerks"),
                report(&case, &file, &brief, 0, 2, "bone-clerks")
            );
        }
    }
}
