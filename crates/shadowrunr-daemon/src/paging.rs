//! Cursor paging: `runs.page`, `tournaments.page` and `ledger.page`; plus `ledger.summary`, the
//! whole-source fold over `ledger.page`'s source grammar.
//!
//! Cursors are opaque to clients (hex of a versioned key). Runs and tournaments page by key
//! (`(tournament_id, run_id)` / `tournament_id`), so a run added while a client pages is
//! neither skipped nor repeated; ledgers are append-only and page by row offset. A page reads
//! at most `limit + 1` entries past the cursor, skipping directories before loading them.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use shadowrunr_types::rpc::*;
use shadowrunr_types::{Effort, EngineKind, ModelTier, Role};

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
                items.push(entry.with_health().row);
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

/// One ledger file in a `ledger.page` / `ledger.summary` source.
struct LedgerFile {
    path: PathBuf,
    run: Option<(String, String)>,
}

fn ledger_files(
    cfg: &Config,
    run_id: Option<&String>,
    tournament_id: Option<&String>,
) -> Result<Vec<LedgerFile>, RpcError> {
    let sim_root = cfg.paths.sim_runs();
    let per_run = |e: RunEntry| LedgerFile {
        path: e.dir.join("ledger.jsonl"),
        run: Some((e.row.run_id, e.row.tournament_id)),
    };
    Ok(match (run_id, tournament_id) {
        (Some(r), t) => vec![per_run(runs::find_run(
            &sim_root,
            r,
            t.map(String::as_str),
        )?)],
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
    'files: for file in ledger_files(cfg, p.run_id.as_ref(), p.tournament_id.as_ref())? {
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

/// A label's serde name (`claude_cli`), the order the console sorts labels in.
fn label<T: Serialize>(t: &T) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// Distinct values in label order.
fn sorted_labels<T: Serialize + Copy>(set: &[T]) -> Vec<T> {
    let mut by_label: Vec<(String, T)> = set.iter().map(|t| (label(t), *t)).collect();
    by_label.sort_by(|a, b| a.0.cmp(&b.0));
    by_label.dedup_by(|a, b| a.0 == b.0);
    by_label.into_iter().map(|(_, t)| t).collect()
}

fn add_once<T: PartialEq>(set: &mut Vec<T>, v: Option<T>) {
    if let Some(v) = v.filter(|v| !set.contains(v)) {
        set.push(v);
    }
}

#[derive(Default)]
struct RoleFold {
    calls: u64,
    errors: u64,
    fallbacks: u64,
    tiers: Vec<ModelTier>,
    efforts: Vec<Effort>,
    engines: Vec<EngineKind>,
    models: BTreeSet<String>,
    dur: Vec<u64>,
    first: Vec<u64>,
}

/// `ledger.summary`: every complete row of the source (the `ledger.page` grammar and its
/// `complete_lines` rule) folded once into totals, per-role routes and per-account rows, with
/// `summariseLedger`'s definitions (apps/console engineModel.ts): failed calls are left out of
/// the duration percentiles, percentiles are nearest-rank (the reconciler's `stats::percentile`),
/// and a row without a cost counts in `uncosted_calls` instead of as $0.
pub fn ledger_summary(cfg: &Config, p: LedgerSummaryParams) -> Result<LedgerSummary, RpcError> {
    use shadowrunr_reconcile::stats::percentile;
    let mut s = LedgerSummary {
        calls: 0,
        errors: 0,
        fallbacks: 0,
        cost_usd: 0.0,
        uncosted_calls: 0,
        cache_read_tokens: 0,
        unrouted: 0,
        routes: Vec::new(),
        accounts: Vec::new(),
    };
    let mut roles: Vec<(Role, RoleFold)> = Vec::new();
    let mut role_at: HashMap<Role, usize> = HashMap::new();
    let mut account_at: HashMap<(EngineKind, String), usize> = HashMap::new();
    for file in ledger_files(cfg, p.run_id.as_ref(), p.tournament_id.as_ref())? {
        for (i, line) in complete_lines(&file.path)?.iter().enumerate() {
            let l = parse_entry(line, &file, i + 1)?;
            let failed = l.error_kind.is_some();
            let fell_back = l.fallback_reason.is_some();
            s.calls += 1;
            s.errors += u64::from(failed);
            s.fallbacks += u64::from(fell_back);
            match l.cost_usd {
                Some(c) => s.cost_usd += c,
                None => s.uncosted_calls += 1,
            }
            s.cache_read_tokens += l.usage.cache_read_tokens;
            let k = *role_at.entry(l.role).or_insert_with(|| {
                roles.push((l.role, RoleFold::default()));
                roles.len() - 1
            });
            let r = &mut roles[k].1;
            r.calls += 1;
            r.errors += u64::from(failed);
            r.fallbacks += u64::from(fell_back);
            add_once(&mut r.tiers, l.tier);
            add_once(&mut r.efforts, l.effort);
            add_once(&mut r.engines, l.engine);
            if let Some(m) = &l.model {
                r.models.insert(m.clone());
            }
            if !failed {
                r.dur.push(l.duration_ms);
            }
            r.first.extend(l.first_text_ms);
            let (Some(engine), Some(account)) = (l.engine, l.account) else {
                s.unrouted += 1;
                continue;
            };
            let k = *account_at
                .entry((engine, account.clone()))
                .or_insert_with(|| {
                    s.accounts.push(AccountSummary {
                        engine,
                        account,
                        calls: 0,
                        errors: 0,
                        models: Vec::new(),
                        last_ts_ms: 0,
                        last_error: None,
                    });
                    s.accounts.len() - 1
                });
            let a = &mut s.accounts[k];
            a.calls += 1;
            a.errors += u64::from(failed);
            if let Some(m) = l.model {
                match a.models.iter_mut().find(|x| x.model == m) {
                    Some(x) => x.calls += 1,
                    None => a.models.push(ModelCalls { model: m, calls: 1 }),
                }
            }
            if l.ts_ms >= a.last_ts_ms {
                a.last_ts_ms = l.ts_ms;
                a.last_error = l.error_kind;
            }
        }
    }
    s.routes = roles
        .into_iter()
        .map(|(role, r)| RouteSummary {
            role,
            calls: r.calls,
            errors: r.errors,
            fallbacks: r.fallbacks,
            tiers: sorted_labels(&r.tiers),
            efforts: sorted_labels(&r.efforts),
            engines: sorted_labels(&r.engines),
            models: r.models.into_iter().collect(),
            duration_p50_ms: percentile(&r.dur, 50.0),
            duration_p95_ms: percentile(&r.dur, 95.0),
            first_text_p50_ms: percentile(&r.first, 50.0),
        })
        .collect();
    s.routes.sort_by_key(|r| std::cmp::Reverse(r.calls));
    for a in &mut s.accounts {
        a.models.sort_by_key(|m| std::cmp::Reverse(m.calls));
    }
    s.accounts.sort_by_key(|a| std::cmp::Reverse(a.calls));
    Ok(s)
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
