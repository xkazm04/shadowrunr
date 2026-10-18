//! `shadowrunr policy install-list <list-id> [--source <url|path>]`: download or copy a
//! moderation list into `<policy>/local/<list-id>.txt` (git-ignored, never committed) and
//! record its provenance in `<policy>/local/<list-id>.json`.
//!
//! The installed `.txt` is the brain's format: one term per line, optionally a tab and
//! comma-separated categories, `#` comment lines. Sources are converted on install:
//! - a JSON array of `{"word", "categories"}` objects (awdev1/better-profane-words) or of
//!   strings;
//! - a CSV with a `text` column and `category_*` columns (the Surge AI list as copied in
//!   NVIDIA/garak);
//! - plain text, one term per line.
//!
//! The brain compiles only the categories its placeholder rule names (docs/spec/contract.md
//! § Platform rules). Overlord never redistributes list data.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A list id the installer knows a default source for.
#[derive(Debug, Clone, Copy)]
pub struct KnownList {
    pub id: &'static str,
    pub url: &'static str,
    pub licence: &'static str,
}

/// Default sources for the list ids named by `policy/platform-rules.json`'s placeholders
/// (docs/research/06-moderation-and-monetisation.md § 5.2).
pub const KNOWN_LISTS: &[KnownList] = &[
    KnownList {
        id: "awdev1-better-profane-words",
        url: "https://raw.githubusercontent.com/awdev1/better-profane-words/main/words.json",
        licence: "GPL-3.0 (https://github.com/awdev1/better-profane-words)",
    },
    KnownList {
        id: "surge-ai-profanity-en",
        url: "https://raw.githubusercontent.com/NVIDIA/garak/main/garak/data/profanity_en.csv",
        licence: "MIT per a 2022 snapshot of the deleted Surge AI upstream (unconfirmed); \
                  copy in NVIDIA/garak (Apache-2.0 repo, no file notice)",
    },
];

/// Downloads larger than this are refused.
pub const MAX_SOURCE_BYTES: usize = 20 << 20;

/// `<policy>/local/<list-id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListMeta {
    pub list_id: String,
    /// URL or path the list came from.
    pub source: String,
    pub licence: String,
    /// sha256 of the installed `.txt`.
    pub sha256: String,
    /// sha256 of the bytes as downloaded or copied.
    pub source_sha256: String,
    pub terms: usize,
    /// UTC date of the install, `YYYY-MM-DD`.
    pub installed: String,
}

pub fn known(list_id: &str) -> Option<&'static KnownList> {
    KNOWN_LISTS.iter().find(|k| k.id == list_id)
}

pub fn local_dir(policy_dir: &Path) -> PathBuf {
    policy_dir.join("local")
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// UTC `YYYY-MM-DD` of a Unix time in seconds.
pub fn utc_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn clean_term(t: &str) -> Option<String> {
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty() && !t.starts_with('#')).then_some(t)
}

fn clean_cat(c: &str) -> Option<String> {
    let c = c.trim().replace([',', '\t', '\n', '\r'], " ");
    (!c.is_empty()).then_some(c)
}

/// One CSV record split into fields (RFC 4180 quoting).
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Convert a downloaded or copied list into `(term, categories)` rows.
pub fn convert(bytes: &[u8]) -> Result<Vec<(String, Vec<String>)>, String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    if trimmed.starts_with('[') {
        let v: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("list JSON: {e}"))?;
        for item in v.as_array().into_iter().flatten() {
            let (term, cats) = match item {
                serde_json::Value::String(s) => (s.as_str(), Vec::new()),
                serde_json::Value::Object(o) => {
                    let Some(term) = ["word", "text", "term"]
                        .iter()
                        .find_map(|k| o.get(*k).and_then(|w| w.as_str()))
                    else {
                        continue;
                    };
                    let mut cats: Vec<String> = Vec::new();
                    if let Some(a) = o.get("categories").and_then(|c| c.as_array()) {
                        cats.extend(a.iter().filter_map(|c| c.as_str()).filter_map(clean_cat));
                    }
                    if let Some(c) = o.get("category").and_then(|c| c.as_str()) {
                        cats.extend(clean_cat(c));
                    }
                    (term, cats)
                }
                _ => continue,
            };
            if let Some(t) = clean_term(term) {
                rows.push((t, cats));
            }
        }
    } else {
        let mut lines = text.lines();
        let header = lines.next().unwrap_or("");
        let cols: Vec<String> = csv_fields(header)
            .iter()
            .map(|c| c.trim().to_ascii_lowercase())
            .collect();
        if header.contains(',') && cols.iter().any(|c| c == "text") {
            let ti = cols.iter().position(|c| c == "text").unwrap_or(0);
            let ci: Vec<usize> = cols
                .iter()
                .enumerate()
                .filter(|(_, c)| c.starts_with("category"))
                .map(|(i, _)| i)
                .collect();
            for line in lines {
                let f = csv_fields(line);
                let Some(t) = f.get(ti).and_then(|t| clean_term(t)) else {
                    continue;
                };
                let cats = ci
                    .iter()
                    .filter_map(|&i| f.get(i).and_then(|c| clean_cat(c)))
                    .collect();
                rows.push((t, cats));
            }
        } else {
            for line in text.lines() {
                let (t, c) = line.split_once('\t').unwrap_or((line, ""));
                if let Some(t) = clean_term(t) {
                    rows.push((t, c.split(',').filter_map(clean_cat).collect()));
                }
            }
        }
    }
    rows.sort();
    rows.dedup();
    if rows.is_empty() {
        return Err("the source holds no terms".into());
    }
    Ok(rows)
}

/// Render rows in the brain's `.txt` format.
pub fn render(
    list_id: &str,
    source: &str,
    licence: &str,
    date: &str,
    rows: &[(String, Vec<String>)],
) -> String {
    let mut s = format!(
        "# {list_id}: installed {date} from {source}\n# Licence: {licence}\n# Local operator install; never commit or redistribute.\n"
    );
    for (t, cats) in rows {
        s.push_str(t);
        if !cats.is_empty() {
            s.push('\t');
            s.push_str(&cats.join(","));
        }
        s.push('\n');
    }
    s
}

async fn fetch(source: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(source)
            .header("User-Agent", "shadowrunr-policy-install")
            .send()
            .await
            .map_err(|e| format!("{source}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("{source}: HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| format!("{source}: {e}"))?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(format!("{source}: {} bytes exceeds the cap", bytes.len()));
        }
        Ok(bytes.to_vec())
    } else {
        let p = Path::new(source);
        let meta = std::fs::metadata(p).map_err(|e| format!("{source}: {e}"))?;
        if meta.len() as usize > MAX_SOURCE_BYTES {
            return Err(format!("{source}: {} bytes exceeds the cap", meta.len()));
        }
        std::fs::read(p).map_err(|e| format!("{source}: {e}"))
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Install `list_id` from `source` (default: the known source for the id) into
/// `<policy_dir>/local/`. Returns the provenance record written beside the list.
pub async fn install(
    policy_dir: &Path,
    list_id: &str,
    source: Option<&str>,
    now_secs: i64,
) -> Result<ListMeta, String> {
    let safe = !list_id.is_empty()
        && list_id.len() <= 100
        && list_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !safe {
        return Err(format!("list id {list_id:?} must be [A-Za-z0-9_-]"));
    }
    let k = known(list_id);
    let source = match (source, k) {
        (Some(s), _) => s.to_string(),
        (None, Some(k)) => k.url.to_string(),
        (None, None) => {
            return Err(format!(
                "no default source for list {list_id:?}; pass --source <url|path> (known: {})",
                KNOWN_LISTS
                    .iter()
                    .map(|k| k.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let licence = k
        .map(|k| k.licence.to_string())
        .unwrap_or_else(|| "unknown (operator-supplied source)".to_string());
    let bytes = fetch(&source).await?;
    let rows = convert(&bytes)?;
    let date = utc_date(now_secs);
    let txt = render(list_id, &source, &licence, &date, &rows);
    let dir = local_dir(policy_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let meta = ListMeta {
        list_id: list_id.to_string(),
        source,
        licence,
        sha256: sha_hex(txt.as_bytes()),
        source_sha256: sha_hex(&bytes),
        terms: rows.len(),
        installed: date,
    };
    write_atomic(&dir.join(format!("{list_id}.txt")), txt.as_bytes())?;
    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    write_atomic(
        &dir.join(format!("{list_id}.json")),
        format!("{json}\n").as_bytes(),
    )?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invented tokens only: no real slur ever appears in a fixture.
    const JSON_FIXTURE: &str = r#"[
      {"word": "zorblax", "categories": ["slur_racial"], "intensity": 5},
      {"word": "quexfin", "categories": ["slur_gender", "insult"], "intensity": 4},
      {"word": "wibblet", "categories": ["insult"], "intensity": 1},
      "glimsnorp"
    ]"#;

    const CSV_FIXTURE: &str = "text,canonical_form_1,category_1,category_2,severity_rating\n\
        zorblax,zorblax,racial / ethnic slurs,,2.8\n\
        \"glim, snorp\",glim,sexual orientation / gender,other / general insult,2.2\n\
        wibblet,wibblet,bodily fluids / excrement,,1.0\n";

    #[test]
    fn converts_better_profane_words_json() {
        let rows = convert(JSON_FIXTURE.as_bytes()).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows.contains(&("zorblax".into(), vec!["slur_racial".into()])));
        assert!(rows.contains(&(
            "quexfin".into(),
            vec!["slur_gender".into(), "insult".into()]
        )));
        assert!(rows.contains(&("glimsnorp".into(), vec![])));
    }

    #[test]
    fn converts_the_surge_csv_and_plain_text() {
        let rows = convert(CSV_FIXTURE.as_bytes()).unwrap();
        assert!(rows.contains(&("zorblax".into(), vec!["racial / ethnic slurs".into()])));
        assert!(rows.contains(&(
            "glim, snorp".into(),
            vec![
                "sexual orientation / gender".into(),
                "other / general insult".into()
            ]
        )));
        let rows = convert(b"# comment\nzorblax\nquexfin\tslur_gender\n\n").unwrap();
        assert_eq!(
            rows,
            vec![
                ("quexfin".to_string(), vec!["slur_gender".to_string()]),
                ("zorblax".to_string(), vec![]),
            ]
        );
        assert!(convert(b"\n# nothing\n").is_err());
    }

    #[test]
    fn dates_are_utc_calendar_days() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_792_281_600), "2026-10-18");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn the_default_source_for_better_profane_words_is_its_raw_list() {
        let k = known("awdev1-better-profane-words").unwrap();
        assert!(k
            .url
            .starts_with("https://raw.githubusercontent.com/awdev1/better-profane-words/"));
        assert!(k.licence.starts_with("GPL-3.0"));
        assert!(known("surge-ai-profanity-en").is_some());
    }

    #[tokio::test]
    async fn install_from_a_path_writes_the_list_and_its_provenance() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("words.json");
        std::fs::write(&src, JSON_FIXTURE).unwrap();
        let policy = d.path().join("policy");
        let meta = install(
            &policy,
            "awdev1-better-profane-words",
            Some(src.to_str().unwrap()),
            1_792_281_600,
        )
        .await
        .unwrap();
        assert_eq!(meta.terms, 4);
        assert_eq!(meta.installed, "2026-10-18");
        assert!(meta.licence.starts_with("GPL-3.0"));
        let txt =
            std::fs::read_to_string(policy.join("local/awdev1-better-profane-words.txt")).unwrap();
        assert!(txt.contains("zorblax\tslur_racial\n"));
        assert_eq!(meta.sha256, sha_hex(txt.as_bytes()));
        let back: ListMeta = serde_json::from_str(
            &std::fs::read_to_string(policy.join("local/awdev1-better-profane-words.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(back, meta);

        // The brain compiles it for the placeholder's categories.
        let terms = shadowrunr_brain::moderation::parse_installed_list(&txt);
        let (re, n) = shadowrunr_brain::moderation::compile_installed(
            &terms,
            &["slur_racial".into(), "slur_gender".into()],
        )
        .unwrap();
        assert_eq!(n, 3, "zorblax, quexfin and the untagged glimsnorp");
        assert!(re.is_match("z0rblax"));
        assert!(!re.is_match("wibblet"));
    }

    #[tokio::test]
    async fn unknown_ids_need_a_source_and_bad_ids_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let e = install(d.path(), "my-list", None, 0).await.unwrap_err();
        assert!(e.contains("--source"));
        let e = install(d.path(), "../evil", Some("x"), 0)
            .await
            .unwrap_err();
        assert!(e.contains("must be"));
        assert!(!d.path().join("local").exists());
    }
}
