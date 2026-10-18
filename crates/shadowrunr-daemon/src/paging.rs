//! Cursor paging: `runs.page`, `tournaments.page` and `ledger.page`.
//!
//! Cursors are opaque to clients (hex of a versioned key). Runs and tournaments page by key
//! (`(tournament_id, run_id)` / `tournament_id`), so a run added while a client pages is
//! neither skipped nor repeated; ledgers are append-only and page by row offset. A page reads
//! at most `limit + 1` entries past the cursor, skipping directories before loading them.

use std::path::{Path, PathBuf};

use shadowrunr_types::rpc::*;

use crate::config::Config;
use crate::rpc::RpcError;
use crate::runs::{self, RunEntry};

const CURSOR_VERSION: &str = "v1";
const SEP: char = '\u{1f}';

/// Encode a cursor key.
pub fn encode_cursor(parts: &[&str]) -> String {
    let mut key = String::from(CURSOR_VERSION);
    for p in parts {
        key.push(SEP);
        key.push_str(p);
    }
    key.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Decode a cursor made by [`encode_cursor`] with `n` parts.
pub fn decode_cursor(cursor: &str, n: usize) -> Result<Vec<String>, RpcError> {
    let bad = || RpcError::invalid(format!("invalid cursor {cursor:?}"));
    if !cursor.len().is_multiple_of(2) || cursor.len() > 4096 {
        return Err(bad());
    }
    let bytes = (0..cursor.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(cursor.get(i..i + 2).ok_or_else(bad)?, 16).map_err(|_| bad()))
        .collect::<Result<Vec<u8>, RpcError>>()?;
    let key = String::from_utf8(bytes).map_err(|_| bad())?;
    let mut parts = key.split(SEP);
    if parts.next() != Some(CURSOR_VERSION) {
        return Err(bad());
    }
    let parts: Vec<String> = parts.map(String::from).collect();
    if parts.len() != n {
        return Err(bad());
    }
    Ok(parts)
}

/// `limit` in 1..=MAX_LIMIT (larger values are capped).
pub fn page_limit(limit: u32) -> Result<usize, RpcError> {
    if limit == 0 {
        return Err(RpcError::invalid("limit must be at least 1"));
    }
    Ok(limit.min(MAX_LIMIT) as usize)
}

fn matches(f: &RunFilter, e: &RunEntry) -> bool {
    let s = &e.row.spec;
    f.tone_variant.as_ref().is_none_or(|t| *t == s.tone_variant)
        && f.core_tier.is_none_or(|t| t == s.core_tier)
        && f.segment.is_none_or(|g| g == s.segment)
        && f.audience_size.is_none_or(|a| a == s.audience_size)
        && f.has_verdict.is_none_or(|v| v == e.row.has_verdict)
        && f.text.as_ref().is_none_or(|t| {
            e.row
                .run_id
                .to_ascii_lowercase()
                .contains(&t.to_ascii_lowercase())
        })
}

fn tournament_ids(sim_root: &Path, only: Option<&str>) -> Result<Vec<String>, RpcError> {
    match only {
        Some(t) => {
            runs::check_id("tournament_id", t)?;
            if !sim_root.join(t).is_dir() {
                return Err(RpcError::not_found(format!("tournament {t}")));
            }
            Ok(vec![t.to_string()])
        }
        None => Ok(runs::subdirs(sim_root)
            .iter()
            .map(|d| runs::name_of(d))
            .collect()),
    }
}

/// `runs.page`: runs ordered by `(tournament_id, run_id)`, optionally filtered.
pub fn runs_page(sim_root: &Path, p: RunsPageParams) -> Result<RunsPage, RpcError> {
    let limit = page_limit(p.limit)?;
    let after = match &p.cursor {
        Some(c) => {
            let k = decode_cursor(c, 2)?;
            Some((k[0].clone(), k[1].clone()))
        }
        None => None,
    };
    let filter = p.filter.unwrap_or_default();
    let mut items: Vec<RunRow> = Vec::with_capacity(limit + 1);
    'outer: for tid in tournament_ids(sim_root, p.tournament_id.as_deref())? {
        if after.as_ref().is_some_and(|(t, _)| tid < *t) {
            continue;
        }
        for dir in runs::subdirs(&sim_root.join(&tid)) {
            let name = runs::name_of(&dir);
            if after
                .as_ref()
                .is_some_and(|(t, r)| (tid.as_str(), name.as_str()) <= (t.as_str(), r.as_str()))
            {
                continue;
            }
            let Some(entry) = runs::load_entry(&dir, &tid) else {
                continue;
            };
            if matches(&filter, &entry) {
                items.push(entry.row);
                if items.len() > limit {
                    break 'outer;
                }
            }
        }
    }
    let next_cursor = (items.len() > limit).then(|| {
        items.truncate(limit);
        let last = &items[limit - 1];
        encode_cursor(&[&last.tournament_id, &last.run_id])
    });
    Ok(RunsPage { items, next_cursor })
}

/// `tournaments.page`: tournaments ordered by id.
pub fn tournaments_page(
    sim_root: &Path,
    p: TournamentsPageParams,
) -> Result<TournamentsPage, RpcError> {
    let limit = page_limit(p.limit)?;
    let after = match &p.cursor {
        Some(c) => Some(decode_cursor(c, 1)?.remove(0)),
        None => None,
    };
    let mut items = Vec::with_capacity(limit + 1);
    for dir in runs::subdirs(sim_root) {
        let tid = runs::name_of(&dir);
        if after.as_ref().is_some_and(|a| tid <= *a) {
            continue;
        }
        if let Some(row) = runs::tournament_row(sim_root, &tid) {
            items.push(row);
            if items.len() > limit {
                break;
            }
        }
    }
    let next_cursor = (items.len() > limit).then(|| {
        items.truncate(limit);
        encode_cursor(&[&items[limit - 1].tournament_id])
    });
    Ok(TournamentsPage { items, next_cursor })
}

/// One ledger file in a `ledger.page` source.
struct LedgerFile {
    path: PathBuf,
    run: Option<(String, String)>,
}

fn ledger_files(cfg: &Config, p: &LedgerPageParams) -> Result<Vec<LedgerFile>, RpcError> {
    let sim_root = cfg.paths.sim_runs();
    let per_run = |e: RunEntry| LedgerFile {
        path: e.dir.join("ledger.jsonl"),
        run: Some((e.row.run_id, e.row.tournament_id)),
    };
    Ok(match (&p.run_id, &p.tournament_id) {
        (Some(r), t) => vec![per_run(runs::find_run(&sim_root, r, t.as_deref())?)],
        (None, Some(t)) => runs::list_runs(&sim_root, Some(t))?
            .into_iter()
            .map(per_run)
            .collect(),
        (None, None) => vec![LedgerFile {
            path: cfg.engine.ledger_path(),
            run: None,
        }],
    })
}

/// Complete lines of a JSONL file (a trailing line still being written is left out). A
/// missing file has no lines.
fn complete_lines(path: &Path) -> Result<Vec<String>, RpcError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(RpcError::io(format!("{}: {e}", path.display()))),
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop(); // the text after the last '\n': empty, or a partial row
    Ok(lines
        .into_iter()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect())
}

fn sim_entry(row: shadowrunr_sim::ledger::LedgerRow, run: &(String, String)) -> LedgerEntry {
    LedgerEntry {
        ts_ms: row.ts_unix_ms,
        role: row.role,
        tier: Some(row.tier),
        effort: Some(row.effort),
        engine: row.engine,
        account: row.account,
        model: row.model,
        first_text_ms: row.first_text_ms,
        duration_ms: row.duration_ms,
        usage: row.usage,
        cost_usd: row.cost_usd,
        fallback_reason: row.fallback_reason,
        correlation_id: row.correlation_id,
        error_kind: row.error_kind,
        warm: row.session,
        run_id: Some(run.0.clone()),
        tournament_id: Some(run.1.clone()),
    }
}

fn engine_entry(row: shadowrunr_engine::ledger::LedgerRow) -> LedgerEntry {
    LedgerEntry {
        ts_ms: row.ts_ms,
        role: row.role,
        tier: None,
        effort: None,
        engine: Some(row.engine),
        account: row.account,
        model: Some(row.model),
        first_text_ms: row.first_text_ms,
        duration_ms: row.duration_ms,
        usage: row.usage,
        cost_usd: row.cost_usd,
        fallback_reason: row.fallback_reason,
        correlation_id: row.correlation_id,
        error_kind: row.error_kind,
        warm: row.warm,
        run_id: None,
        tournament_id: None,
    }
}

fn parse_entry(line: &str, file: &LedgerFile, n: usize) -> Result<LedgerEntry, RpcError> {
    let bad = |e: serde_json::Error| RpcError::io(format!("{}:{n}: {e}", file.path.display()));
    match &file.run {
        Some(run) => Ok(sim_entry(serde_json::from_str(line).map_err(bad)?, run)),
        None => Ok(engine_entry(serde_json::from_str(line).map_err(bad)?)),
    }
}

/// `ledger.page`: engine calls of one run, of a tournament's runs in run order, or of the
/// daemon's process-wide ledger.
pub fn ledger_page(cfg: &Config, p: LedgerPageParams) -> Result<LedgerPage, RpcError> {
    let limit = page_limit(p.limit)?;
    let offset: u64 = match &p.cursor {
        Some(c) => decode_cursor(c, 1)?[0]
            .parse()
            .map_err(|_| RpcError::invalid(format!("invalid cursor {c:?}")))?,
        None => 0,
    };
    let mut skip = offset;
    let mut items = Vec::with_capacity(limit + 1);
    'files: for file in ledger_files(cfg, &p)? {
        let lines = complete_lines(&file.path)?;
        if skip >= lines.len() as u64 {
            skip -= lines.len() as u64;
            continue;
        }
        for (i, line) in lines.iter().enumerate().skip(skip as usize) {
            items.push(parse_entry(line, &file, i + 1)?);
            if items.len() > limit {
                break 'files;
            }
        }
        skip = 0;
    }
    let next_cursor = (items.len() > limit).then(|| {
        items.truncate(limit);
        encode_cursor(&[&(offset + limit as u64).to_string()])
    });
    Ok(LedgerPage { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let c = encode_cursor(&["t-1", "run.a"]);
        assert_eq!(decode_cursor(&c, 2).unwrap(), ["t-1", "run.a"]);
        assert!(decode_cursor(&c, 1).is_err());
        for bad in ["", "zz", "abc", "7631"] {
            assert!(decode_cursor(bad, 1).is_err(), "{bad}");
        }
    }

    #[test]
    fn limit_is_capped_and_never_zero() {
        assert_eq!(page_limit(10_000).unwrap(), 500);
        assert_eq!(page_limit(7).unwrap(), 7);
        assert!(page_limit(0).is_err());
    }
}
