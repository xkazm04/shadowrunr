//! The memory store: markdown files are the truth, SQLite (FTS5) is a rebuildable index.
//!
//! Layout under `memory_dir`:
//! ```text
//! episodes/YYYY/MM/<id>.md            append-only
//! facts/<scope>/<key>.md              the live fact for (scope, key); viewer keys are
//!                                     `<viewer_id>` or `<viewer_id>/<topic>`
//! facts/_history/<id>.md              superseded, aged-out and forgotten facts
//! facts/_tombstones/<scope>/<key>.md  keys the sleep cycle may not re-derive
//! procedurals/<id>.md
//! cycles/<id>.md                      sleep-cycle reports
//! index.sqlite                        the index (delete it and call `rebuild_index`)
//! ```
//!
//! Forgetting is demotion, never deletion: a fact that leaves the live set keeps its file
//! (under `_history/`) with its final status.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use shadowrunr_types::{Episode, EpisodeKind, Fact, FactScope, TsMs};

use super::markdown::{key_path, read_doc, safe_segment, walk_md, write_doc, Doc};
use crate::util::{fnv1a64, fts_query, year_month};
use crate::BrainError;

pub const INDEX_FILE: &str = "index.sqlite";

const DAY_MS: i64 = 86_400_000;
/// 30 unrecalled days cost one importance point (floor 1).
pub const DECAY_AFTER_DAYS: i64 = 30;
/// A fact at the importance floor ages out after 90 days without recall...
pub const AGE_OUT_DAYS: i64 = 90;
/// ...or 365 days for viewer facts (regulars come back after long gaps).
pub const AGE_OUT_DAYS_VIEWER: i64 = 365;
/// Live facts per scope; the least important overflow is aged out.
pub const MAX_FACTS_PER_SCOPE: usize = 500;
/// Importance of a fact written by the sleep cycle.
pub const DEFAULT_FACT_IMPORTANCE: u8 = 3;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS mem_node (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, file_path TEXT NOT NULL,
  content_hash TEXT NOT NULL, importance INTEGER NOT NULL,
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS mem_fts USING fts5(
  id UNINDEXED, kind UNINDEXED, body, tags, tokenize = 'porter unicode61');
CREATE TABLE IF NOT EXISTS mem_episode (
  id TEXT PRIMARY KEY, ts_ms INTEGER NOT NULL, stream_id TEXT NOT NULL,
  kind TEXT NOT NULL, importance INTEGER NOT NULL, text TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS mem_episode_ts ON mem_episode(ts_ms);
CREATE TABLE IF NOT EXISTS mem_fact (
  id TEXT PRIMARY KEY, scope TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
  confidence REAL NOT NULL, sources_json TEXT NOT NULL, supersedes_id TEXT,
  viewer_id TEXT, status TEXT NOT NULL, importance INTEGER NOT NULL,
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL,
  last_recalled_ms INTEGER NOT NULL, last_decay_ms INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS mem_fact_key ON mem_fact(scope, key, status);
CREATE INDEX IF NOT EXISTS mem_fact_viewer ON mem_fact(viewer_id, status);
CREATE TABLE IF NOT EXISTS mem_fact_tombstone (
  scope TEXT NOT NULL, key TEXT NOT NULL, ts_ms INTEGER NOT NULL, reason TEXT NOT NULL,
  PRIMARY KEY (scope, key));
CREATE TABLE IF NOT EXISTS mem_procedural (
  id TEXT PRIMARY KEY, rule TEXT NOT NULL, sources_json TEXT NOT NULL,
  importance INTEGER NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS mem_cycle (
  id TEXT PRIMARY KEY, started_ms INTEGER NOT NULL, finished_ms INTEGER NOT NULL,
  status TEXT NOT NULL, stats_json TEXT NOT NULL);
";

impl From<rusqlite::Error> for BrainError {
    fn from(e: rusqlite::Error) -> Self {
        BrainError::Store(format!("sqlite: {e}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactStatus {
    Live,
    Superseded,
    AgedOut,
    Forgotten,
}

impl FactStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FactStatus::Live => "live",
            FactStatus::Superseded => "superseded",
            FactStatus::AgedOut => "aged_out",
            FactStatus::Forgotten => "forgotten",
        }
    }
    fn parse(s: &str) -> Result<Self, BrainError> {
        Ok(match s {
            "live" => FactStatus::Live,
            "superseded" => FactStatus::Superseded,
            "aged_out" => FactStatus::AgedOut,
            "forgotten" => FactStatus::Forgotten,
            other => return Err(BrainError::Store(format!("unknown fact status {other:?}"))),
        })
    }
}

/// A fact plus its index-side bookkeeping.
#[derive(Debug, Clone)]
pub struct StoredFact {
    pub fact: Fact,
    pub importance: u8,
    pub status: FactStatus,
    pub created_ms: TsMs,
    pub updated_ms: TsMs,
    pub last_recalled_ms: TsMs,
    pub last_decay_ms: TsMs,
    /// Set for `FactScope::Viewer`: the key's first segment.
    pub viewer_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Procedural {
    pub id: String,
    pub rule: String,
    pub sources: Vec<String>,
    pub importance: u8,
    pub created_ms: TsMs,
}

#[derive(Debug, Clone)]
pub struct CycleRecord {
    pub id: String,
    pub started_ms: TsMs,
    pub finished_ms: TsMs,
    /// "completed" or "failed".
    pub status: String,
    pub stats: serde_json::Value,
    pub report: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecayReport {
    pub decayed: usize,
    pub aged_out: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildReport {
    pub episodes: usize,
    pub facts: usize,
    pub tombstones: usize,
    pub procedurals: usize,
    pub cycles: usize,
}

pub fn scope_str(s: FactScope) -> &'static str {
    match s {
        FactScope::Viewer => "viewer",
        FactScope::Character => "character",
        FactScope::World => "world",
        FactScope::Case => "case",
    }
}

pub fn parse_scope(s: &str) -> Option<FactScope> {
    Some(match s {
        "viewer" => FactScope::Viewer,
        "character" => FactScope::Character,
        "world" => FactScope::World,
        "case" => FactScope::Case,
        _ => return None,
    })
}

fn kind_str(k: EpisodeKind) -> &'static str {
    match k {
        EpisodeKind::Stream => "stream",
        EpisodeKind::Job => "job",
        EpisodeKind::Reconcile => "reconcile",
        EpisodeKind::System => "system",
    }
}

fn parse_kind(s: &str) -> Result<EpisodeKind, BrainError> {
    Ok(match s {
        "stream" => EpisodeKind::Stream,
        "job" => EpisodeKind::Job,
        "reconcile" => EpisodeKind::Reconcile,
        "system" => EpisodeKind::System,
        other => return Err(BrainError::Store(format!("unknown episode kind {other:?}"))),
    })
}

fn viewer_of(scope: FactScope, key: &str) -> Option<String> {
    (scope == FactScope::Viewer).then(|| key.split('/').next().unwrap_or(key).to_string())
}

pub struct MemoryStore {
    root: PathBuf,
    conn: Mutex<Connection>,
    counter: AtomicU64,
    salt: u64,
    scope_cap: usize,
    /// Facts whose `last_recalled_ms` moved in SQL but not yet in markdown.
    dirty_recall: Mutex<BTreeSet<String>>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("root", &self.root)
            .finish()
    }
}

fn open_conn(root: &Path) -> Result<Connection, BrainError> {
    fs::create_dir_all(root).map_err(|e| BrainError::Store(format!("{}: {e}", root.display())))?;
    let conn = Connection::open(root.join(INDEX_FILE))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

impl MemoryStore {
    /// Open (or create) the store at `root`.
    pub fn open(root: &Path) -> Result<Self, BrainError> {
        let conn = open_conn(root)?;
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ fnv1a64(root.to_string_lossy().as_bytes());
        Ok(Self {
            root: root.to_path_buf(),
            conn: Mutex::new(conn),
            counter: AtomicU64::new(0),
            salt,
            scope_cap: MAX_FACTS_PER_SCOPE,
            dirty_recall: Mutex::new(BTreeSet::new()),
        })
    }

    /// Override the per-scope live-fact cap (default 500).
    pub fn with_scope_cap(mut self, cap: usize) -> Self {
        self.scope_cap = cap.max(1);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// A fresh id: `<prefix>_<ts>_<hash>`.
    pub fn new_id(&self, prefix: &str, ts_ms: TsMs) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let h = fnv1a64(format!("{}:{n}:{ts_ms}", self.salt).as_bytes());
        format!("{prefix}_{ts_ms}_{:08x}", h as u32)
    }

    // ── Episodes ────────────────────────────────────────────────────────

    fn episode_path(&self, ep: &Episode) -> PathBuf {
        let (y, m) = year_month(ep.ts_ms);
        self.root
            .join("episodes")
            .join(format!("{y:04}"))
            .join(format!("{m:02}"))
            .join(format!("{}.md", safe_segment(&ep.id)))
    }

    /// Append an episode (markdown first, then the index).
    pub fn write_episode(&self, ep: &Episode) -> Result<(), BrainError> {
        if ep.id.trim().is_empty() {
            return Err(BrainError::Store("episode without id".into()));
        }
        let mut ep = ep.clone();
        ep.importance = ep.importance.clamp(1, 5);
        let mut doc = Doc::new();
        doc.set("id", &ep.id)
            .set("ts_ms", ep.ts_ms)
            .set("stream_id", &ep.stream_id)
            .set("kind", kind_str(ep.kind))
            .set("importance", ep.importance);
        doc.body = ep.text.replace("\r\n", "\n");
        let path = self.episode_path(&ep);
        let hash = write_doc(&path, &doc)?;
        let rel = self.rel(&path);
        let conn = self.conn();
        index_episode(&conn, &ep, &rel, &hash)
    }

    pub fn episode_exists(&self, id: &str) -> Result<bool, BrainError> {
        let conn = self.conn();
        Ok(conn
            .query_row("SELECT 1 FROM mem_episode WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// The most recent `limit` non-system episodes, oldest first.
    pub fn recent_episodes(&self, limit: usize) -> Result<Vec<Episode>, BrainError> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode
             WHERE kind != 'system' ORDER BY ts_ms DESC, id DESC LIMIT ?1",
        )?;
        let mut v = st
            .query_map([limit as i64], row_episode)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        v.reverse();
        Ok(v)
    }

    /// Non-system episodes after `after_ms`, newest first, at most `limit`.
    pub fn episodes_since(&self, after_ms: TsMs, limit: usize) -> Result<Vec<Episode>, BrainError> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode
             WHERE kind != 'system' AND ts_ms > ?1 ORDER BY ts_ms DESC, id DESC LIMIT ?2",
        )?;
        let v = st
            .query_map(params![after_ms, limit as i64], row_episode)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Characters of non-system episode text after `after_ms` (sleep-cycle pressure).
    pub fn chars_since(&self, after_ms: TsMs) -> Result<usize, BrainError> {
        let conn = self.conn();
        let n: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(text)), 0) FROM mem_episode
             WHERE kind != 'system' AND ts_ms > ?1",
            [after_ms],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    // ── Facts ───────────────────────────────────────────────────────────

    fn fact_path(&self, scope: FactScope, key: &str) -> PathBuf {
        key_path(&self.root.join("facts").join(scope_str(scope)), key)
    }

    fn history_path(&self, id: &str) -> PathBuf {
        self.root
            .join("facts")
            .join("_history")
            .join(format!("{}.md", safe_segment(id)))
    }

    fn tombstone_path(&self, scope: FactScope, key: &str) -> PathBuf {
        key_path(
            &self
                .root
                .join("facts")
                .join("_tombstones")
                .join(scope_str(scope)),
            key,
        )
    }

    /// Write a fact. Rejected when it has no sources, cites an episode that does not exist,
    /// has an empty or multi-line key, or its (scope, key) is tombstoned. A live fact with
    /// the same (scope, key), and the fact named in `supersedes`, are superseded.
    pub fn write_fact(
        &self,
        fact: &Fact,
        importance: u8,
        now_ms: TsMs,
    ) -> Result<StoredFact, BrainError> {
        if fact.sources.is_empty() || fact.sources.iter().any(|s| s.trim().is_empty()) {
            return Err(BrainError::Rejected(format!(
                "fact {:?} has no source episode",
                fact.key
            )));
        }
        let key = fact.key.trim();
        if key.is_empty() || key.contains(['\n', '\r']) || key.split('/').all(str::is_empty) {
            return Err(BrainError::Rejected(format!(
                "fact key {:?} is invalid",
                fact.key
            )));
        }
        if fact.value.trim().is_empty() {
            return Err(BrainError::Rejected(format!(
                "fact {key:?} has an empty value"
            )));
        }
        for src in &fact.sources {
            if !self.episode_exists(src)? {
                return Err(BrainError::Rejected(format!(
                    "fact {key:?} cites unknown episode {src:?}"
                )));
            }
        }
        if self.is_tombstoned(fact.scope, key)? {
            return Err(BrainError::Rejected(format!(
                "fact {}/{key} is tombstoned",
                scope_str(fact.scope)
            )));
        }

        let mut to_supersede: Vec<String> = Vec::new();
        if let Some(old) = self.live_fact(fact.scope, key)? {
            to_supersede.push(old.fact.id);
        }
        let mut supersedes = fact.supersedes.clone();
        if let Some(sid) = &fact.supersedes {
            match self.fact(sid)? {
                Some(old) if old.status == FactStatus::Live => {
                    if !to_supersede.contains(sid) {
                        to_supersede.push(sid.clone());
                    }
                }
                _ => supersedes = None,
            }
        }
        if supersedes.is_none() {
            supersedes = to_supersede.first().cloned();
        }

        let id = if fact.id.trim().is_empty() || self.fact(&fact.id)?.is_some() {
            self.new_id("f", now_ms)
        } else {
            fact.id.clone()
        };
        for old in &to_supersede {
            self.retire(old, FactStatus::Superseded, now_ms)?;
        }

        let sf = StoredFact {
            fact: Fact {
                id,
                scope: fact.scope,
                key: key.to_string(),
                value: fact.value.trim().to_string(),
                confidence: if fact.confidence.is_finite() {
                    fact.confidence.clamp(0.0, 1.0)
                } else {
                    0.5
                },
                sources: fact.sources.clone(),
                supersedes,
            },
            importance: importance.clamp(1, 5),
            status: FactStatus::Live,
            created_ms: now_ms,
            updated_ms: now_ms,
            last_recalled_ms: now_ms,
            last_decay_ms: now_ms,
            viewer_id: viewer_of(fact.scope, key),
        };
        self.persist_fact(&sf, &self.fact_path(sf.fact.scope, &sf.fact.key))?;
        self.enforce_scope_cap(sf.fact.scope, now_ms)?;
        Ok(sf)
    }

    fn persist_fact(&self, sf: &StoredFact, path: &Path) -> Result<(), BrainError> {
        let doc = fact_doc(sf);
        let hash = write_doc(path, &doc)?;
        let rel = self.rel(path);
        let conn = self.conn();
        index_fact(&conn, sf, &rel, &hash)
    }

    /// Move a live fact out of the live set: new status, importance 0, file to `_history/`.
    fn retire(&self, id: &str, status: FactStatus, now_ms: TsMs) -> Result<(), BrainError> {
        let Some(mut sf) = self.fact(id)? else {
            return Ok(());
        };
        if sf.status != FactStatus::Live {
            return Ok(());
        }
        let old_path = self.fact_path(sf.fact.scope, &sf.fact.key);
        sf.status = status;
        sf.importance = 0;
        sf.updated_ms = now_ms;
        self.persist_fact(&sf, &self.history_path(&sf.fact.id))?;
        if old_path.is_file() {
            // Only remove the key file when it still holds this fact.
            if let Ok((doc, _)) = read_doc(&old_path) {
                if doc.get("id") == Some(sf.fact.id.as_str()) {
                    fs::remove_file(&old_path)
                        .map_err(|e| BrainError::Store(format!("{}: {e}", old_path.display())))?;
                }
            }
        }
        Ok(())
    }

    fn enforce_scope_cap(&self, scope: FactScope, now_ms: TsMs) -> Result<(), BrainError> {
        let excess: Vec<String> = {
            let conn = self.conn();
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM mem_fact WHERE scope = ?1 AND status = 'live'",
                [scope_str(scope)],
                |r| r.get(0),
            )?;
            let over = (n as usize).saturating_sub(self.scope_cap);
            if over == 0 {
                return Ok(());
            }
            let mut st = conn.prepare(
                "SELECT id FROM mem_fact WHERE scope = ?1 AND status = 'live'
                 ORDER BY importance ASC, last_recalled_ms ASC, updated_ms ASC, id ASC LIMIT ?2",
            )?;
            let v = st
                .query_map(params![scope_str(scope), over as i64], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            v
        };
        for id in excess {
            self.retire(&id, FactStatus::AgedOut, now_ms)?;
        }
        Ok(())
    }

    /// Forget a fact by (scope, key) and tombstone the key so the sleep cycle cannot
    /// re-derive it.
    pub fn forget_fact(
        &self,
        scope: FactScope,
        key: &str,
        reason: &str,
        now_ms: TsMs,
    ) -> Result<(), BrainError> {
        if let Some(sf) = self.live_fact(scope, key)? {
            self.retire(&sf.fact.id, FactStatus::Forgotten, now_ms)?;
        }
        let mut doc = Doc::new();
        doc.set("scope", scope_str(scope))
            .set("key", key)
            .set("ts_ms", now_ms)
            .set("reason", reason);
        let path = self.tombstone_path(scope, key);
        write_doc(&path, &doc)?;
        let conn = self.conn();
        index_tombstone(&conn, scope, key, now_ms, reason)
    }

    pub fn is_tombstoned(&self, scope: FactScope, key: &str) -> Result<bool, BrainError> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT 1 FROM mem_fact_tombstone WHERE scope = ?1 AND key = ?2",
                params![scope_str(scope), key],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn fact(&self, id: &str) -> Result<Option<StoredFact>, BrainError> {
        let conn = self.conn();
        let r = conn
            .query_row(
                &format!("SELECT {FACT_COLS} FROM mem_fact WHERE id = ?1"),
                [id],
                row_fact,
            )
            .optional()?;
        r.transpose()
    }

    pub fn live_fact(&self, scope: FactScope, key: &str) -> Result<Option<StoredFact>, BrainError> {
        let conn = self.conn();
        let r = conn
            .query_row(
                &format!(
                    "SELECT {FACT_COLS} FROM mem_fact
                     WHERE scope = ?1 AND key = ?2 AND status = 'live'"
                ),
                params![scope_str(scope), key],
                row_fact,
            )
            .optional()?;
        r.transpose()
    }

    /// Live facts about one viewer, most important first.
    pub fn viewer_facts(
        &self,
        viewer_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredFact>, BrainError> {
        self.query_facts(
            &format!(
                "SELECT {FACT_COLS} FROM mem_fact WHERE viewer_id = ?1 AND status = 'live'
                 ORDER BY importance DESC, confidence DESC, updated_ms DESC LIMIT ?2"
            ),
            params![viewer_id, limit as i64],
        )
    }

    /// Which of these viewers have at least one live fact (regulars).
    pub fn viewers_with_facts(&self, ids: &[String]) -> Result<HashSet<String>, BrainError> {
        let conn = self.conn();
        let mut st = conn
            .prepare("SELECT 1 FROM mem_fact WHERE viewer_id = ?1 AND status = 'live' LIMIT 1")?;
        let mut out = HashSet::new();
        for id in ids {
            if st.exists([id])? {
                out.insert(id.clone());
            }
        }
        Ok(out)
    }

    /// The always-included tier: top live facts of a scope.
    pub fn top_facts(&self, scope: FactScope, limit: usize) -> Result<Vec<StoredFact>, BrainError> {
        self.query_facts(
            &format!(
                "SELECT {FACT_COLS} FROM mem_fact WHERE scope = ?1 AND status = 'live'
                 ORDER BY importance DESC, confidence DESC, updated_ms DESC, id ASC LIMIT ?2"
            ),
            params![scope_str(scope), limit as i64],
        )
    }

    /// Every live fact (diagnostics, consolidation context).
    pub fn live_facts(&self) -> Result<Vec<StoredFact>, BrainError> {
        self.query_facts(
            &format!("SELECT {FACT_COLS} FROM mem_fact WHERE status = 'live' ORDER BY scope, key"),
            [],
        )
    }

    fn query_facts(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Vec<StoredFact>, BrainError> {
        let conn = self.conn();
        let mut st = conn.prepare(sql)?;
        let v = st
            .query_map(p, row_fact)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Record that these facts were put in front of the model.
    pub fn mark_recalled(&self, ids: &[String], now_ms: TsMs) -> Result<(), BrainError> {
        if ids.is_empty() {
            return Ok(());
        }
        {
            let conn = self.conn();
            let mut st = conn.prepare("UPDATE mem_fact SET last_recalled_ms = ?1 WHERE id = ?2")?;
            for id in ids {
                st.execute(params![now_ms, id])?;
            }
        }
        let mut dirty = self.dirty_recall.lock().unwrap_or_else(|p| p.into_inner());
        dirty.extend(ids.iter().cloned());
        Ok(())
    }

    /// Write recall marks back into the fact files so a rebuild keeps them.
    pub fn flush_recall_marks(&self) -> Result<usize, BrainError> {
        let ids: Vec<String> = {
            let mut dirty = self.dirty_recall.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::take(&mut *dirty).into_iter().collect()
        };
        let mut n = 0;
        for id in ids {
            if let Some(sf) = self.fact(&id)? {
                if sf.status == FactStatus::Live {
                    self.persist_fact(&sf, &self.fact_path(sf.fact.scope, &sf.fact.key))?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    // ── Procedurals ─────────────────────────────────────────────────────

    /// "When X, do Y" behaviour rules. Same citation rule as facts.
    pub fn write_procedural(
        &self,
        rule: &str,
        sources: &[String],
        now_ms: TsMs,
    ) -> Result<Procedural, BrainError> {
        if sources.is_empty() || sources.iter().any(|s| s.trim().is_empty()) {
            return Err(BrainError::Rejected(
                "procedural has no source episode".into(),
            ));
        }
        if rule.trim().is_empty() {
            return Err(BrainError::Rejected("procedural has an empty rule".into()));
        }
        for s in sources {
            if !self.episode_exists(s)? {
                return Err(BrainError::Rejected(format!(
                    "procedural cites unknown episode {s:?}"
                )));
            }
        }
        let p = Procedural {
            id: self.new_id("p", now_ms),
            rule: rule.trim().to_string(),
            sources: sources.to_vec(),
            importance: DEFAULT_FACT_IMPORTANCE,
            created_ms: now_ms,
        };
        let mut doc = Doc::new();
        doc.set("id", &p.id)
            .set("importance", p.importance)
            .set("created_ms", p.created_ms)
            .set("sources", json_list(&p.sources));
        doc.body = p.rule.clone();
        let path = self
            .root
            .join("procedurals")
            .join(format!("{}.md", safe_segment(&p.id)));
        let hash = write_doc(&path, &doc)?;
        let rel = self.rel(&path);
        let conn = self.conn();
        index_procedural(&conn, &p, &rel, &hash)?;
        Ok(p)
    }

    pub fn top_procedurals(&self, limit: usize) -> Result<Vec<Procedural>, BrainError> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, rule, sources_json, importance, created_ms FROM mem_procedural
             ORDER BY importance DESC, created_ms DESC, id ASC LIMIT ?1",
        )?;
        let v = st
            .query_map([limit as i64], |r| {
                Ok(Procedural {
                    id: r.get(0)?,
                    rule: r.get(1)?,
                    sources: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                    importance: r.get::<_, i64>(3)?.clamp(0, 5) as u8,
                    created_ms: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    // ── Search ──────────────────────────────────────────────────────────

    /// BM25 over live facts, procedurals and episodes. Returns (id, kind), best first.
    pub fn search(&self, text: &str, limit: usize) -> Result<Vec<(String, String)>, BrainError> {
        let Some(q) = fts_query(text, 16) else {
            return Ok(Vec::new());
        };
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, kind FROM mem_fts WHERE mem_fts MATCH ?1
             ORDER BY bm25(mem_fts), id LIMIT ?2",
        )?;
        let v = st
            .query_map(params![q, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// BM25 over live facts only.
    pub fn search_facts(&self, text: &str, limit: usize) -> Result<Vec<StoredFact>, BrainError> {
        let mut out = Vec::new();
        for (id, kind) in self.search(text, limit * 4 + 8)? {
            if kind != "fact" {
                continue;
            }
            if let Some(sf) = self.fact(&id)? {
                if sf.status == FactStatus::Live {
                    out.push(sf);
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// BM25 over non-system episodes.
    pub fn search_episodes(&self, text: &str, limit: usize) -> Result<Vec<Episode>, BrainError> {
        let ids: Vec<String> = self
            .search(text, limit * 4 + 8)?
            .into_iter()
            .filter(|(_, k)| k == "episode")
            .map(|(id, _)| id)
            .collect();
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode WHERE id = ?1",
        )?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(ep) = st.query_row([&id], row_episode).optional()? {
                let ep = ep?;
                if ep.kind != EpisodeKind::System {
                    out.push(ep);
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    // ── Decay ───────────────────────────────────────────────────────────

    /// 30 unrecalled days cost one importance point (floor 1); a fact at the floor ages out
    /// after 90 days without recall (365 for viewer facts).
    pub fn decay(&self, now_ms: TsMs) -> Result<DecayReport, BrainError> {
        let mut report = DecayReport::default();
        for mut sf in self.live_facts()? {
            let touched = sf.last_recalled_ms.max(sf.updated_ms);
            let idle = now_ms - touched.max(sf.last_decay_ms);
            if sf.importance > 1 && idle >= DECAY_AFTER_DAYS * DAY_MS {
                sf.importance -= 1;
                sf.last_decay_ms = now_ms;
                self.persist_fact(&sf, &self.fact_path(sf.fact.scope, &sf.fact.key))?;
                report.decayed += 1;
                continue;
            }
            let limit = if sf.fact.scope == FactScope::Viewer {
                AGE_OUT_DAYS_VIEWER
            } else {
                AGE_OUT_DAYS
            };
            if sf.importance <= 1 && now_ms - touched >= limit * DAY_MS {
                self.retire(&sf.fact.id, FactStatus::AgedOut, now_ms)?;
                report.aged_out += 1;
            }
        }
        Ok(report)
    }

    // ── Cycles ──────────────────────────────────────────────────────────

    pub fn write_cycle(&self, c: &CycleRecord) -> Result<(), BrainError> {
        let mut doc = Doc::new();
        doc.set("id", &c.id)
            .set("started_ms", c.started_ms)
            .set("finished_ms", c.finished_ms)
            .set("status", &c.status)
            .set("stats", c.stats.to_string());
        doc.body = c.report.clone();
        let path = self
            .root
            .join("cycles")
            .join(format!("{}.md", safe_segment(&c.id)));
        let hash = write_doc(&path, &doc)?;
        let rel = self.rel(&path);
        let conn = self.conn();
        index_cycle(&conn, c, &rel, &hash)
    }

    /// The most recent completed sleep cycle.
    pub fn last_completed_cycle(&self) -> Result<Option<CycleRecord>, BrainError> {
        let conn = self.conn();
        let r = conn
            .query_row(
                "SELECT id, started_ms, finished_ms, status, stats_json FROM mem_cycle
                 WHERE status = 'completed' ORDER BY finished_ms DESC, id DESC LIMIT 1",
                [],
                |r| {
                    Ok(CycleRecord {
                        id: r.get(0)?,
                        started_ms: r.get(1)?,
                        finished_ms: r.get(2)?,
                        status: r.get(3)?,
                        stats: serde_json::from_str(&r.get::<_, String>(4)?)
                            .unwrap_or(serde_json::Value::Null),
                        report: String::new(),
                    })
                },
            )
            .optional()?;
        Ok(r)
    }

    /// A deterministic dump of every index table, for tests and diagnostics.
    pub fn index_snapshot(&self) -> Result<String, BrainError> {
        let conn = self.conn();
        let mut out = String::new();
        for (name, sql) in [
            ("node", "SELECT id, kind, file_path, content_hash, importance, created_ms, updated_ms FROM mem_node ORDER BY id"),
            ("fts", "SELECT id, kind, body, tags FROM mem_fts ORDER BY id"),
            ("episode", "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode ORDER BY id"),
            ("fact", "SELECT id, scope, key, value, confidence, sources_json, supersedes_id, viewer_id, status, importance, created_ms, updated_ms, last_recalled_ms, last_decay_ms FROM mem_fact ORDER BY id"),
            ("tombstone", "SELECT scope, key, ts_ms, reason FROM mem_fact_tombstone ORDER BY scope, key"),
            ("procedural", "SELECT id, rule, sources_json, importance, created_ms FROM mem_procedural ORDER BY id"),
            ("cycle", "SELECT id, started_ms, finished_ms, status, stats_json FROM mem_cycle ORDER BY id"),
        ] {
            let mut st = conn.prepare(sql)?;
            let cols = st.column_count();
            let mut rows = st.query([])?;
            while let Some(row) = rows.next()? {
                out.push_str(name);
                for i in 0..cols {
                    let v: rusqlite::types::Value = row.get(i)?;
                    out.push_str(&format!(" | {v:?}"));
                }
                out.push('\n');
            }
        }
        Ok(out)
    }
}

// ── Row mapping and index writers (shared by live writes and rebuild) ────

const FACT_COLS: &str = "id, scope, key, value, confidence, sources_json, supersedes_id, \
    viewer_id, status, importance, created_ms, updated_ms, last_recalled_ms, last_decay_ms";

type RowResult<T> = rusqlite::Result<Result<T, BrainError>>;

fn row_episode(r: &rusqlite::Row<'_>) -> RowResult<Episode> {
    let id: String = r.get(0)?;
    let ts_ms: TsMs = r.get(1)?;
    let stream_id: String = r.get(2)?;
    let kind: String = r.get(3)?;
    let importance: i64 = r.get(4)?;
    let text: String = r.get(5)?;
    Ok(parse_kind(&kind).map(|kind| Episode {
        id,
        ts_ms,
        stream_id,
        kind,
        text,
        importance: importance.clamp(0, 5) as u8,
    }))
}

fn row_fact(r: &rusqlite::Row<'_>) -> RowResult<StoredFact> {
    let scope: String = r.get(1)?;
    let status: String = r.get(8)?;
    let sources: String = r.get(5)?;
    let Some(scope) = parse_scope(&scope) else {
        return Ok(Err(BrainError::Store(format!("unknown scope {scope:?}"))));
    };
    let status = match FactStatus::parse(&status) {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(StoredFact {
        fact: Fact {
            id: r.get(0)?,
            scope,
            key: r.get(2)?,
            value: r.get(3)?,
            confidence: r.get::<_, f64>(4)? as f32,
            sources: serde_json::from_str(&sources).unwrap_or_default(),
            supersedes: r.get(6)?,
        },
        viewer_id: r.get(7)?,
        status,
        importance: r.get::<_, i64>(9)?.clamp(0, 5) as u8,
        created_ms: r.get(10)?,
        updated_ms: r.get(11)?,
        last_recalled_ms: r.get(12)?,
        last_decay_ms: r.get(13)?,
    }))
}

fn json_list(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn fact_doc(sf: &StoredFact) -> Doc {
    let mut doc = Doc::new();
    doc.set("id", &sf.fact.id)
        .set("scope", scope_str(sf.fact.scope))
        .set("key", &sf.fact.key)
        .set("confidence", sf.fact.confidence)
        .set("importance", sf.importance)
        .set("status", sf.status.as_str())
        .set("created_ms", sf.created_ms)
        .set("updated_ms", sf.updated_ms)
        .set("last_recalled_ms", sf.last_recalled_ms)
        .set("last_decay_ms", sf.last_decay_ms)
        .set("sources", json_list(&sf.fact.sources));
    if let Some(s) = &sf.fact.supersedes {
        doc.set("supersedes", s);
    }
    doc.body = sf.fact.value.clone();
    doc
}

#[allow(clippy::too_many_arguments)]
fn upsert_node(
    conn: &Connection,
    id: &str,
    kind: &str,
    rel: &str,
    hash: &str,
    importance: i64,
    created: TsMs,
    updated: TsMs,
) -> Result<(), BrainError> {
    conn.execute(
        "INSERT INTO mem_node (id, kind, file_path, content_hash, importance, created_ms, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, file_path = excluded.file_path,
           content_hash = excluded.content_hash, importance = excluded.importance,
           updated_ms = excluded.updated_ms",
        params![id, kind, rel, hash, importance, created, updated],
    )?;
    Ok(())
}

fn set_fts(
    conn: &Connection,
    id: &str,
    kind: &str,
    body: Option<(&str, &str)>,
) -> Result<(), BrainError> {
    conn.execute("DELETE FROM mem_fts WHERE id = ?1", [id])?;
    if let Some((body, tags)) = body {
        conn.execute(
            "INSERT INTO mem_fts (id, kind, body, tags) VALUES (?1, ?2, ?3, ?4)",
            params![id, kind, body, tags],
        )?;
    }
    Ok(())
}

fn index_episode(conn: &Connection, ep: &Episode, rel: &str, hash: &str) -> Result<(), BrainError> {
    conn.execute(
        "INSERT OR REPLACE INTO mem_episode (id, ts_ms, stream_id, kind, importance, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ep.id,
            ep.ts_ms,
            ep.stream_id,
            kind_str(ep.kind),
            ep.importance,
            ep.text
        ],
    )?;
    upsert_node(
        conn,
        &ep.id,
        "episode",
        rel,
        hash,
        i64::from(ep.importance),
        ep.ts_ms,
        ep.ts_ms,
    )?;
    set_fts(conn, &ep.id, "episode", Some((&ep.text, kind_str(ep.kind))))
}

fn index_fact(conn: &Connection, sf: &StoredFact, rel: &str, hash: &str) -> Result<(), BrainError> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO mem_fact ({FACT_COLS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
        ),
        params![
            sf.fact.id,
            scope_str(sf.fact.scope),
            sf.fact.key,
            sf.fact.value,
            f64::from(sf.fact.confidence),
            json_list(&sf.fact.sources),
            sf.fact.supersedes,
            sf.viewer_id,
            sf.status.as_str(),
            sf.importance,
            sf.created_ms,
            sf.updated_ms,
            sf.last_recalled_ms,
            sf.last_decay_ms
        ],
    )?;
    upsert_node(
        conn,
        &sf.fact.id,
        "fact",
        rel,
        hash,
        i64::from(sf.importance),
        sf.created_ms,
        sf.updated_ms,
    )?;
    if sf.status == FactStatus::Live {
        let body = format!(
            "{} {}",
            sf.fact.key.replace(['/', '_', '-'], " "),
            sf.fact.value
        );
        let tags = format!(
            "{} {}",
            scope_str(sf.fact.scope),
            sf.viewer_id.as_deref().unwrap_or("")
        );
        set_fts(conn, &sf.fact.id, "fact", Some((&body, tags.trim())))
    } else {
        set_fts(conn, &sf.fact.id, "fact", None)
    }
}

fn index_tombstone(
    conn: &Connection,
    scope: FactScope,
    key: &str,
    ts: TsMs,
    reason: &str,
) -> Result<(), BrainError> {
    conn.execute(
        "INSERT OR REPLACE INTO mem_fact_tombstone (scope, key, ts_ms, reason) VALUES (?1, ?2, ?3, ?4)",
        params![scope_str(scope), key, ts, reason],
    )?;
    Ok(())
}

fn index_procedural(
    conn: &Connection,
    p: &Procedural,
    rel: &str,
    hash: &str,
) -> Result<(), BrainError> {
    conn.execute(
        "INSERT OR REPLACE INTO mem_procedural (id, rule, sources_json, importance, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            p.id,
            p.rule,
            json_list(&p.sources),
            p.importance,
            p.created_ms
        ],
    )?;
    upsert_node(
        conn,
        &p.id,
        "procedural",
        rel,
        hash,
        i64::from(p.importance),
        p.created_ms,
        p.created_ms,
    )?;
    set_fts(conn, &p.id, "procedural", Some((&p.rule, "procedural")))
}

fn index_cycle(
    conn: &Connection,
    c: &CycleRecord,
    rel: &str,
    hash: &str,
) -> Result<(), BrainError> {
    conn.execute(
        "INSERT OR REPLACE INTO mem_cycle (id, started_ms, finished_ms, status, stats_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            c.id,
            c.started_ms,
            c.finished_ms,
            c.status,
            c.stats.to_string()
        ],
    )?;
    upsert_node(
        conn,
        &c.id,
        "cycle",
        rel,
        hash,
        1,
        c.started_ms,
        c.finished_ms,
    )
}

/// Regenerate the SQLite index from the markdown truth under `memory_dir`. The old index
/// file is deleted first, so a corrupt index is never read.
pub fn rebuild_index(memory_dir: &Path) -> Result<RebuildReport, BrainError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let p = memory_dir.join(format!("{INDEX_FILE}{suffix}"));
        if p.exists() {
            fs::remove_file(&p).map_err(|e| BrainError::Store(format!("{}: {e}", p.display())))?;
        }
    }
    let mut conn = open_conn(memory_dir)?;
    let tx = conn.transaction()?;
    let mut report = RebuildReport::default();
    let rel = |p: &Path| -> String {
        p.strip_prefix(memory_dir)
            .unwrap_or(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };

    for path in walk_md(&memory_dir.join("episodes"))? {
        let (doc, hash) = read_doc(&path)?;
        let ep = Episode {
            id: doc.req("id", &path)?.to_string(),
            ts_ms: doc.num("ts_ms", &path)?,
            stream_id: doc.req("stream_id", &path)?.to_string(),
            kind: parse_kind(doc.req("kind", &path)?)?,
            importance: doc.num("importance", &path)?,
            text: doc.body.clone(),
        };
        index_episode(&tx, &ep, &rel(&path), &hash)?;
        report.episodes += 1;
    }

    let tomb_root = memory_dir.join("facts").join("_tombstones");
    for path in walk_md(&memory_dir.join("facts"))? {
        let (doc, hash) = read_doc(&path)?;
        if path.starts_with(&tomb_root) {
            let scope = parse_scope(doc.req("scope", &path)?)
                .ok_or_else(|| BrainError::Store(format!("{}: bad scope", path.display())))?;
            index_tombstone(
                &tx,
                scope,
                doc.req("key", &path)?,
                doc.num("ts_ms", &path)?,
                doc.get("reason").unwrap_or(""),
            )?;
            report.tombstones += 1;
            continue;
        }
        let scope = parse_scope(doc.req("scope", &path)?)
            .ok_or_else(|| BrainError::Store(format!("{}: bad scope", path.display())))?;
        let key = doc.req("key", &path)?.to_string();
        let sf = StoredFact {
            fact: Fact {
                id: doc.req("id", &path)?.to_string(),
                scope,
                key: key.clone(),
                value: doc.body.clone(),
                confidence: doc.num("confidence", &path)?,
                sources: serde_json::from_str(doc.req("sources", &path)?)
                    .map_err(|e| BrainError::Store(format!("{}: sources: {e}", path.display())))?,
                supersedes: doc.get("supersedes").map(str::to_string),
            },
            importance: doc.num("importance", &path)?,
            status: FactStatus::parse(doc.req("status", &path)?)?,
            created_ms: doc.num("created_ms", &path)?,
            updated_ms: doc.num("updated_ms", &path)?,
            last_recalled_ms: doc.num("last_recalled_ms", &path)?,
            last_decay_ms: doc.num("last_decay_ms", &path)?,
            viewer_id: viewer_of(scope, &key),
        };
        index_fact(&tx, &sf, &rel(&path), &hash)?;
        report.facts += 1;
    }

    for path in walk_md(&memory_dir.join("procedurals"))? {
        let (doc, hash) = read_doc(&path)?;
        let p = Procedural {
            id: doc.req("id", &path)?.to_string(),
            rule: doc.body.clone(),
            sources: serde_json::from_str(doc.req("sources", &path)?)
                .map_err(|e| BrainError::Store(format!("{}: sources: {e}", path.display())))?,
            importance: doc.num("importance", &path)?,
            created_ms: doc.num("created_ms", &path)?,
        };
        index_procedural(&tx, &p, &rel(&path), &hash)?;
        report.procedurals += 1;
    }

    for path in walk_md(&memory_dir.join("cycles"))? {
        let (doc, hash) = read_doc(&path)?;
        let c = CycleRecord {
            id: doc.req("id", &path)?.to_string(),
            started_ms: doc.num("started_ms", &path)?,
            finished_ms: doc.num("finished_ms", &path)?,
            status: doc.req("status", &path)?.to_string(),
            stats: serde_json::from_str(doc.get("stats").unwrap_or("null"))
                .unwrap_or(serde_json::Value::Null),
            report: doc.body.clone(),
        };
        index_cycle(&tx, &c, &rel(&path), &hash)?;
        report.cycles += 1;
    }

    tx.commit()?;
    Ok(report)
}
