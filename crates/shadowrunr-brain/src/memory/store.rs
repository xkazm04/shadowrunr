//! The memory store: markdown files are the truth, SQLite (FTS5) is a rebuildable index.
//!
//! Layout under `memory_dir`:
//! ```text
//! episodes/YYYY/MM/<id>.md            append-only
//! facts/<scope>/<key>.md              the live fact for (scope, key); viewer keys are
//!                                     `<viewer_id>` or `<viewer_id>/<topic>`
//! facts/_history/<id>.md              superseded, aged-out and forgotten facts
//! facts/_tombstones/<scope>/<key>.md  keys the sleep cycle may not re-derive
//! procedurals/<key>.md                the live habit for a key (older stores: `<id>.md`)
//! procedurals/_history/<id>.md        superseded and archived habits
//! cycles/<id>.md                      sleep-cycle reports
//! index.sqlite                        the index (delete it and call `rebuild_index`)
//! ```
//!
//! Forgetting is demotion, never deletion: a fact or habit that leaves the live set keeps its
//! file (under `_history/`) with its final status.
//!
//! Aging reads evidence, never exposure: an item ages from its last substantive update
//! (`updated_ms`: a write, a supersession by fresh evidence) or its last use (`last_used_ms`: a
//! NOTE naming it, [`MemoryStore::mark_used`]). Being put in front of the model
//! (`last_recalled_ms`) is recorded for diagnostics and never keeps an item alive: the
//! always-included tier is put there every beat, so recall alone would lock it in forever.
//!
//! Recall reads only what the markdown still backs: an item whose own file, or any cited
//! episode's file, is gone, or whose key is tombstoned, is never recalled, whatever the index
//! says ([`MemoryStore::fact_recallable`]).

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use shadowrunr_types::{Episode, EpisodeKind, Fact, FactScope, TsMs};

use super::markdown::{key_path, read_doc, safe_segment, walk_md, write_doc, Doc};
use crate::util::{fnv1a64, fts_query, normalize, year_month};
use crate::BrainError;

pub const INDEX_FILE: &str = "index.sqlite";

const DAY_MS: i64 = 86_400_000;
/// 30 unrecalled days cost one importance point (floor 1).
pub const DECAY_AFTER_DAYS: i64 = 30;
/// A fact at the importance floor ages out after 90 days without an update or a use...
pub const AGE_OUT_DAYS: i64 = 90;
/// ...or 365 days for viewer facts (regulars come back after long gaps).
pub const AGE_OUT_DAYS_VIEWER: i64 = 365;
/// Live facts per scope; the least important overflow is aged out.
pub const MAX_FACTS_PER_SCOPE: usize = 500;
/// Importance of a fact written by the sleep cycle.
pub const DEFAULT_FACT_IMPORTANCE: u8 = 3;
/// A habit at the importance floor is archived (recoverably, `_history/`) after this many days
/// without an update or a use: twice a fact's horizon, so a habit whose situation is merely
/// rare is never archived because a few streams lacked it.
pub const HABIT_ARCHIVE_DAYS: i64 = 180;

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
  last_recalled_ms INTEGER NOT NULL, last_decay_ms INTEGER NOT NULL,
  last_used_ms INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS mem_fact_key ON mem_fact(scope, key, status);
CREATE INDEX IF NOT EXISTS mem_fact_viewer ON mem_fact(viewer_id, status);
CREATE TABLE IF NOT EXISTS mem_fact_tombstone (
  scope TEXT NOT NULL, key TEXT NOT NULL, ts_ms INTEGER NOT NULL, reason TEXT NOT NULL,
  PRIMARY KEY (scope, key));
CREATE TABLE IF NOT EXISTS mem_procedural (
  id TEXT PRIMARY KEY, key TEXT NOT NULL, rule TEXT NOT NULL, sources_json TEXT NOT NULL,
  supersedes_id TEXT, status TEXT NOT NULL, importance INTEGER NOT NULL,
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL, last_used_ms INTEGER NOT NULL,
  last_decay_ms INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS mem_procedural_key ON mem_procedural(key, status);
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
    /// When the fact was last put in front of the model. Diagnostic only: never an aging input.
    pub last_recalled_ms: TsMs,
    pub last_decay_ms: TsMs,
    /// When the fact was last used (a NOTE named it); 0 when never. Aging reads it.
    pub last_used_ms: TsMs,
    /// Set for `FactScope::Viewer`: the key's first segment.
    pub viewer_id: Option<String>,
}

impl StoredFact {
    /// The last evidence the fact still matters: its last substantive update or use.
    pub fn touched_ms(&self) -> TsMs {
        self.updated_ms.max(self.last_used_ms)
    }
}

/// Where a habit stands: the fact lifecycle's statuses, with `archived` for one that aged out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HabitStatus {
    Live,
    Superseded,
    Archived,
}

impl HabitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HabitStatus::Live => "live",
            HabitStatus::Superseded => "superseded",
            HabitStatus::Archived => "archived",
        }
    }
    fn parse(s: &str) -> Result<Self, BrainError> {
        Ok(match s {
            "live" => HabitStatus::Live,
            "superseded" => HabitStatus::Superseded,
            "archived" => HabitStatus::Archived,
            other => return Err(BrainError::Store(format!("unknown habit status {other:?}"))),
        })
    }
}

/// A "When X, do Y" habit the sleep cycle learned, with the fact lifecycle: a stable key (one
/// live habit per key), supersession, decay under [`HABIT_ARCHIVE_DAYS`], recoverable archive.
#[derive(Debug, Clone)]
pub struct Procedural {
    pub id: String,
    /// Stable across re-derivations: the consolidator's key, else one derived from the rule's
    /// words ([`habit_key`]), so the same habit learned again supersedes, never duplicates.
    pub key: String,
    pub rule: String,
    pub sources: Vec<String>,
    pub supersedes: Option<String>,
    pub status: HabitStatus,
    pub importance: u8,
    pub created_ms: TsMs,
    pub updated_ms: TsMs,
    /// When a NOTE last named the habit; 0 when never.
    pub last_used_ms: TsMs,
    pub last_decay_ms: TsMs,
}

impl Procedural {
    pub fn touched_ms(&self) -> TsMs {
        self.updated_ms.max(self.last_used_ms)
    }
}

/// A habit to write: what the sleep cycle hands [`MemoryStore::write_habit`].
#[derive(Debug, Clone, Default)]
pub struct NewHabit {
    /// `None` derives one from the rule's words ([`habit_key`]).
    pub key: Option<String>,
    pub rule: String,
    pub sources: Vec<String>,
    /// A live habit's id (or key) this one replaces.
    pub supersedes: Option<String>,
}

/// The stable key of a habit with no key of its own: its normalised words, hashed. The same
/// rule re-derived gets the same key, so it supersedes instead of piling up.
pub fn habit_key(rule: &str) -> String {
    format!("h-{:08x}", fnv1a64(normalize(rule).as_bytes()) as u32)
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

/// A position in the episode stream: `(ts_ms, id)` order, so episodes sharing a
/// timestamp (one `end_stream` batch) stay individually addressable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeMark {
    pub ts_ms: TsMs,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecayReport {
    pub decayed: usize,
    pub aged_out: usize,
    pub habits_decayed: usize,
    pub habits_archived: usize,
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

/// `path` under `root`, `/`-separated: the `file_path` the index records for a markdown file.
fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn open_conn(root: &Path) -> Result<Connection, BrainError> {
    fs::create_dir_all(root).map_err(|e| BrainError::Store(format!("{}: {e}", root.display())))?;
    let conn = Connection::open(root.join(INDEX_FILE))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>, BrainError> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// An index written before use-based aging and the habit lifecycle has those tables without
/// their columns. The index is derived, so it is rebuilt from the markdown rather than migrated
/// in place.
fn index_is_stale(root: &Path) -> Result<bool, BrainError> {
    let path = root.join(INDEX_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    let conn = Connection::open(&path)?;
    let stale = |table: &str, column: &str| -> Result<bool, BrainError> {
        let cols = columns(&conn, table)?;
        Ok(!cols.is_empty() && !cols.iter().any(|c| c == column))
    };
    Ok(stale("mem_fact", "last_used_ms")? || stale("mem_procedural", "status")?)
}

impl MemoryStore {
    /// Open (or create) the store at `root`.
    pub fn open(root: &Path) -> Result<Self, BrainError> {
        if index_is_stale(root)? {
            rebuild_index(root)?;
        }
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
        rel_path(&self.root, path)
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
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        index_episode(&tx, &ep, &rel, &hash)?;
        tx.commit()?;
        Ok(())
    }

    /// Whether episode `id` is indexed and its markdown file is still on disk. The markdown is
    /// the truth: an index row whose file is gone cites nothing.
    pub fn episode_exists(&self, id: &str) -> Result<bool, BrainError> {
        let path: Option<String> = {
            let conn = self.conn();
            conn.query_row(
                "SELECT n.file_path FROM mem_episode e JOIN mem_node n ON n.id = e.id
                 WHERE e.id = ?1 AND n.kind = 'episode'",
                [id],
                |r| r.get(0),
            )
            .optional()?
        };
        Ok(path.is_some_and(|p| self.root.join(p).is_file()))
    }

    /// The markdown file the index records for node `id`, if any.
    fn node_path(&self, id: &str) -> Result<Option<PathBuf>, BrainError> {
        let conn = self.conn();
        let p: Option<String> = conn
            .query_row("SELECT file_path FROM mem_node WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(p.map(|p| self.root.join(p)))
    }

    fn node_file_live(&self, id: &str) -> Result<bool, BrainError> {
        Ok(self.node_path(id)?.is_some_and(|p| p.is_file()))
    }

    fn sources_live(&self, sources: &[String]) -> Result<bool, BrainError> {
        for s in sources {
            if !self.episode_exists(s)? {
                return Ok(false);
            }
        }
        Ok(!sources.is_empty())
    }

    /// Whether a fact may be recalled: live, its own markdown file on disk, its key not
    /// tombstoned, and every episode it cites still on disk. The index alone proves nothing (a
    /// row survives a deleted file until the next rebuild), and when provenance is uncertain the
    /// fact stays out of the prompt.
    pub fn fact_recallable(&self, sf: &StoredFact) -> Result<bool, BrainError> {
        Ok(sf.status == FactStatus::Live
            && self.node_file_live(&sf.fact.id)?
            && !self.is_tombstoned(sf.fact.scope, &sf.fact.key)?
            && self.sources_live(&sf.fact.sources)?)
    }

    /// [`MemoryStore::fact_recallable`] for a habit.
    pub fn habit_recallable(&self, p: &Procedural) -> Result<bool, BrainError> {
        Ok(p.status == HabitStatus::Live
            && self.node_file_live(&p.id)?
            && self.sources_live(&p.sources)?)
    }

    fn recallable(&self, v: Vec<StoredFact>, limit: usize) -> Result<Vec<StoredFact>, BrainError> {
        let mut out = Vec::new();
        for sf in v {
            if out.len() >= limit {
                break;
            }
            if self.fact_recallable(&sf)? {
                out.push(sf);
            }
        }
        Ok(out)
    }

    /// The most recent `limit` non-system episodes whose markdown is still on disk, oldest
    /// first.
    pub fn recent_episodes(&self, limit: usize) -> Result<Vec<Episode>, BrainError> {
        let v = {
            let conn = self.conn();
            let mut st = conn.prepare(
                "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode
                 WHERE kind != 'system' ORDER BY ts_ms DESC, id DESC LIMIT ?1",
            )?;
            let v = st
                .query_map([(limit * 2 + 4) as i64], row_episode)?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            v
        };
        let mut out = Vec::new();
        for ep in v {
            if out.len() >= limit {
                break;
            }
            if self.episode_exists(&ep.id)? {
                out.push(ep);
            }
        }
        out.reverse();
        Ok(out)
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

    /// Non-system episodes strictly after `lo` and (if given) strictly before `hi` in
    /// `(ts_ms, id)` order, at most `limit`; newest first or oldest first.
    pub fn episodes_between(
        &self,
        lo: &EpisodeMark,
        hi: Option<&EpisodeMark>,
        limit: usize,
        newest_first: bool,
    ) -> Result<Vec<Episode>, BrainError> {
        let conn = self.conn();
        let dir = if newest_first { "DESC" } else { "ASC" };
        let mut st = conn.prepare(&format!(
            "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode
             WHERE kind != 'system'
               AND (ts_ms > ?1 OR (ts_ms = ?1 AND id > ?2))
               AND (?3 IS NULL OR ts_ms < ?3 OR (ts_ms = ?3 AND id < ?4))
             ORDER BY ts_ms {dir}, id {dir} LIMIT ?5"
        ))?;
        let v = st
            .query_map(
                params![
                    lo.ts_ms,
                    lo.id,
                    hi.map(|h| h.ts_ms),
                    hi.map(|h| h.id.as_str()).unwrap_or(""),
                    limit as i64
                ],
                row_episode,
            )?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Characters of non-system episode text in the same range as `episodes_between`.
    pub fn chars_between(
        &self,
        lo: &EpisodeMark,
        hi: Option<&EpisodeMark>,
    ) -> Result<usize, BrainError> {
        let conn = self.conn();
        let n: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(text)), 0) FROM mem_episode
             WHERE kind != 'system'
               AND (ts_ms > ?1 OR (ts_ms = ?1 AND id > ?2))
               AND (?3 IS NULL OR ts_ms < ?3 OR (ts_ms = ?3 AND id < ?4))",
            params![
                lo.ts_ms,
                lo.id,
                hi.map(|h| h.ts_ms),
                hi.map(|h| h.id.as_str()).unwrap_or("")
            ],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
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
            last_used_ms: 0,
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
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        index_fact(&tx, sf, &rel, &hash)?;
        tx.commit()?;
        Ok(())
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
                 ORDER BY importance ASC, MAX(updated_ms, last_used_ms) ASC, updated_ms ASC, id ASC
                 LIMIT ?2",
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

    /// Recallable live facts about one viewer, most important first.
    pub fn viewer_facts(
        &self,
        viewer_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredFact>, BrainError> {
        let v = self.query_facts(
            &format!(
                "SELECT {FACT_COLS} FROM mem_fact WHERE viewer_id = ?1 AND status = 'live'
                 ORDER BY importance DESC, confidence DESC, updated_ms DESC LIMIT ?2"
            ),
            params![viewer_id, (limit * 2 + 8) as i64],
        )?;
        self.recallable(v, limit)
    }

    /// Which of these viewers have at least one recallable live fact (regulars).
    pub fn viewers_with_facts(&self, ids: &[String]) -> Result<HashSet<String>, BrainError> {
        let mut out = HashSet::new();
        for id in ids {
            if !self.viewer_facts(id, 1)?.is_empty() {
                out.insert(id.clone());
            }
        }
        Ok(out)
    }

    /// The always-included tier: the top recallable live facts of a scope.
    pub fn top_facts(&self, scope: FactScope, limit: usize) -> Result<Vec<StoredFact>, BrainError> {
        let v = self.query_facts(
            &format!(
                "SELECT {FACT_COLS} FROM mem_fact WHERE scope = ?1 AND status = 'live'
                 ORDER BY importance DESC, confidence DESC, updated_ms DESC, id ASC LIMIT ?2"
            ),
            params![scope_str(scope), (limit * 2 + 8) as i64],
        )?;
        self.recallable(v, limit)
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
            // One commit per beat, not one per fact: each autocommit is its own fsync.
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            {
                let mut st =
                    tx.prepare("UPDATE mem_fact SET last_recalled_ms = ?1 WHERE id = ?2")?;
                for id in ids {
                    st.execute(params![now_ms, id])?;
                }
            }
            tx.commit()?;
        }
        let mut dirty = self.dirty_recall.lock().unwrap_or_else(|p| p.into_inner());
        dirty.extend(ids.iter().cloned());
        Ok(())
    }

    /// Record that these facts or habits were used: a NOTE named them. Use is evidence (unlike
    /// recall), so it resets aging; it is written to the markdown at once (a rare event). Returns
    /// how many live items it marked.
    pub fn mark_used(&self, ids: &[String], now_ms: TsMs) -> Result<usize, BrainError> {
        let mut n = 0;
        for id in ids {
            if let Some(mut sf) = self.fact(id)? {
                if sf.status == FactStatus::Live && sf.last_used_ms < now_ms {
                    sf.last_used_ms = now_ms;
                    self.persist_fact(&sf, &self.fact_path(sf.fact.scope, &sf.fact.key))?;
                    n += 1;
                }
                continue;
            }
            if let Some(mut p) = self.habit(id)? {
                if p.status == HabitStatus::Live && p.last_used_ms < now_ms {
                    p.last_used_ms = now_ms;
                    self.persist_habit(&p, &self.habit_path(&p.key))?;
                    n += 1;
                }
            }
        }
        Ok(n)
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

    // ── Procedurals (habits) ────────────────────────────────────────────

    fn habit_path(&self, key: &str) -> PathBuf {
        key_path(&self.root.join("procedurals"), key)
    }

    fn habit_history_path(&self, id: &str) -> PathBuf {
        self.root
            .join("procedurals")
            .join("_history")
            .join(format!("{}.md", safe_segment(id)))
    }

    fn persist_habit(&self, p: &Procedural, path: &Path) -> Result<(), BrainError> {
        let hash = write_doc(path, &habit_doc(p))?;
        let rel = self.rel(path);
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        index_procedural(&tx, p, &rel, &hash)?;
        tx.commit()?;
        Ok(())
    }

    /// "When X, do Y" behaviour rules, keyed by their words ([`habit_key`]). Same citation
    /// rule as facts.
    pub fn write_procedural(
        &self,
        rule: &str,
        sources: &[String],
        now_ms: TsMs,
    ) -> Result<Procedural, BrainError> {
        self.write_habit(
            &NewHabit {
                key: None,
                rule: rule.to_string(),
                sources: sources.to_vec(),
                supersedes: None,
            },
            now_ms,
        )
    }

    /// Write a habit. Rejected when it has no sources, cites an episode whose markdown is gone,
    /// or has an empty rule or a bad key. The live habit with the same key, and the one named in
    /// `supersedes` (by id or key), are superseded (to `_history/`).
    pub fn write_habit(&self, h: &NewHabit, now_ms: TsMs) -> Result<Procedural, BrainError> {
        if h.sources.is_empty() || h.sources.iter().any(|s| s.trim().is_empty()) {
            return Err(BrainError::Rejected(
                "procedural has no source episode".into(),
            ));
        }
        let rule = h.rule.trim();
        if rule.is_empty() {
            return Err(BrainError::Rejected("procedural has an empty rule".into()));
        }
        for s in &h.sources {
            if !self.episode_exists(s)? {
                return Err(BrainError::Rejected(format!(
                    "procedural cites unknown episode {s:?}"
                )));
            }
        }
        let key = match h.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) if k.contains(['\n', '\r']) || k.split('/').all(str::is_empty) => {
                return Err(BrainError::Rejected(format!("habit key {k:?} is invalid")));
            }
            Some(k) => k.to_string(),
            None => habit_key(rule),
        };
        let mut to_supersede: Vec<String> = Vec::new();
        if let Some(old) = self.live_habit(&key)? {
            to_supersede.push(old.id);
        }
        let mut supersedes = None;
        if let Some(s) = h.supersedes.as_deref().filter(|s| !s.trim().is_empty()) {
            let target = match self.habit(s)? {
                Some(p) if p.status == HabitStatus::Live => Some(p),
                _ => self.live_habit(s)?,
            };
            if let Some(p) = target {
                if !to_supersede.contains(&p.id) {
                    to_supersede.push(p.id.clone());
                }
                supersedes = Some(p.id);
            }
        }
        if supersedes.is_none() {
            supersedes = to_supersede.first().cloned();
        }
        for old in &to_supersede {
            self.retire_habit(old, HabitStatus::Superseded, now_ms)?;
        }
        let p = Procedural {
            id: self.new_id("p", now_ms),
            key,
            rule: rule.to_string(),
            sources: h.sources.clone(),
            supersedes,
            status: HabitStatus::Live,
            importance: DEFAULT_FACT_IMPORTANCE,
            created_ms: now_ms,
            updated_ms: now_ms,
            last_used_ms: 0,
            last_decay_ms: now_ms,
        };
        self.persist_habit(&p, &self.habit_path(&p.key))?;
        Ok(p)
    }

    /// Move a live habit out of the live set: new status, importance 0, file to `_history/`.
    fn retire_habit(&self, id: &str, status: HabitStatus, now_ms: TsMs) -> Result<(), BrainError> {
        let Some(mut p) = self.habit(id)? else {
            return Ok(());
        };
        if p.status != HabitStatus::Live {
            return Ok(());
        }
        let old = self.node_path(id)?;
        p.status = status;
        p.importance = 0;
        p.updated_ms = now_ms;
        let hist = self.habit_history_path(&p.id);
        self.persist_habit(&p, &hist)?;
        if let Some(old) = old.filter(|o| *o != hist && o.is_file()) {
            // Only remove the key file when it still holds this habit.
            if let Ok((doc, _)) = read_doc(&old) {
                if doc.get("id") == Some(p.id.as_str()) {
                    fs::remove_file(&old)
                        .map_err(|e| BrainError::Store(format!("{}: {e}", old.display())))?;
                }
            }
        }
        Ok(())
    }

    /// Bring an archived or superseded habit back (the archive is recoverable). Rejected while
    /// another habit holds its key.
    pub fn restore_habit(&self, id: &str, now_ms: TsMs) -> Result<Procedural, BrainError> {
        let Some(mut p) = self.habit(id)? else {
            return Err(BrainError::Rejected(format!("no habit {id:?}")));
        };
        if p.status == HabitStatus::Live {
            return Ok(p);
        }
        if let Some(other) = self.live_habit(&p.key)? {
            return Err(BrainError::Rejected(format!(
                "habit key {:?} is held by {}",
                p.key, other.id
            )));
        }
        let hist = self.node_path(id)?;
        p.status = HabitStatus::Live;
        p.importance = DEFAULT_FACT_IMPORTANCE;
        p.updated_ms = now_ms;
        p.last_decay_ms = now_ms;
        self.persist_habit(&p, &self.habit_path(&p.key))?;
        if let Some(h) = hist.filter(|h| h.is_file()) {
            fs::remove_file(&h).map_err(|e| BrainError::Store(format!("{}: {e}", h.display())))?;
        }
        Ok(p)
    }

    pub fn habit(&self, id: &str) -> Result<Option<Procedural>, BrainError> {
        let conn = self.conn();
        let r = conn
            .query_row(
                &format!("SELECT {HABIT_COLS} FROM mem_procedural WHERE id = ?1"),
                [id],
                row_habit,
            )
            .optional()?;
        r.transpose()
    }

    pub fn live_habit(&self, key: &str) -> Result<Option<Procedural>, BrainError> {
        let conn = self.conn();
        let r = conn
            .query_row(
                &format!(
                    "SELECT {HABIT_COLS} FROM mem_procedural WHERE key = ?1 AND status = 'live'"
                ),
                [key],
                row_habit,
            )
            .optional()?;
        r.transpose()
    }

    fn query_habits(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Vec<Procedural>, BrainError> {
        let conn = self.conn();
        let mut st = conn.prepare(sql)?;
        let v = st
            .query_map(p, row_habit)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Every live habit (diagnostics, decay).
    pub fn live_habits(&self) -> Result<Vec<Procedural>, BrainError> {
        self.query_habits(
            &format!("SELECT {HABIT_COLS} FROM mem_procedural WHERE status = 'live' ORDER BY key"),
            [],
        )
    }

    /// The most important live habits (diagnostics; recall selects by context,
    /// [`MemoryStore::search_habits`]).
    pub fn top_procedurals(&self, limit: usize) -> Result<Vec<Procedural>, BrainError> {
        self.query_habits(
            &format!(
                "SELECT {HABIT_COLS} FROM mem_procedural WHERE status = 'live'
                 ORDER BY importance DESC, updated_ms DESC, id ASC LIMIT ?1"
            ),
            [limit as i64],
        )
    }

    /// BM25 over live habits for the live query: recallable ones only, best first.
    pub fn search_habits(&self, text: &str, limit: usize) -> Result<Vec<Procedural>, BrainError> {
        let Some(q) = fts_query(text, 32) else {
            return Ok(Vec::new());
        };
        let ids: Vec<String> = {
            let conn = self.conn();
            let mut st = conn.prepare(
                "SELECT id FROM mem_fts WHERE mem_fts MATCH ?1 AND kind = 'procedural'
                 ORDER BY bm25(mem_fts), id LIMIT ?2",
            )?;
            let v = st
                .query_map(params![q, (limit * 2 + 8) as i64], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            v
        };
        let mut out = Vec::new();
        for id in ids {
            if out.len() >= limit {
                break;
            }
            if let Some(p) = self.habit(&id)? {
                if self.habit_recallable(&p)? {
                    out.push(p);
                }
            }
        }
        Ok(out)
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

    /// BM25 over recallable live facts only.
    pub fn search_facts(&self, text: &str, limit: usize) -> Result<Vec<StoredFact>, BrainError> {
        let mut out = Vec::new();
        for (id, kind) in self.search(text, limit * 4 + 8)? {
            if kind != "fact" {
                continue;
            }
            if let Some(sf) = self.fact(&id)? {
                if self.fact_recallable(&sf)? {
                    out.push(sf);
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// BM25 over non-system episodes whose markdown is still on disk.
    pub fn search_episodes(&self, text: &str, limit: usize) -> Result<Vec<Episode>, BrainError> {
        let ids: Vec<String> = self
            .search(text, limit * 4 + 8)?
            .into_iter()
            .filter(|(_, k)| k == "episode")
            .map(|(id, _)| id)
            .collect();
        let mut out = Vec::new();
        for id in ids {
            let ep = self
                .conn()
                .query_row(
                    "SELECT id, ts_ms, stream_id, kind, importance, text FROM mem_episode
                     WHERE id = ?1",
                    [&id],
                    row_episode,
                )
                .optional()?;
            if let Some(ep) = ep {
                let ep = ep?;
                if ep.kind != EpisodeKind::System && self.episode_exists(&ep.id)? {
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

    /// 30 days without an update or a use cost one importance point (floor 1); a fact at the
    /// floor ages out 90 days after its last update or use (365 for viewer facts), a habit is
    /// archived after [`HABIT_ARCHIVE_DAYS`]. Being recalled is not a use: the always-included
    /// tier is recalled every beat.
    pub fn decay(&self, now_ms: TsMs) -> Result<DecayReport, BrainError> {
        let mut report = DecayReport::default();
        for mut p in self.live_habits()? {
            let touched = p.touched_ms();
            let idle = now_ms - touched.max(p.last_decay_ms);
            if p.importance > 1 && idle >= DECAY_AFTER_DAYS * DAY_MS {
                p.importance -= 1;
                p.last_decay_ms = now_ms;
                self.persist_habit(&p, &self.habit_path(&p.key))?;
                report.habits_decayed += 1;
                continue;
            }
            if p.importance <= 1 && now_ms - touched >= HABIT_ARCHIVE_DAYS * DAY_MS {
                self.retire_habit(&p.id, HabitStatus::Archived, now_ms)?;
                report.habits_archived += 1;
            }
        }
        for mut sf in self.live_facts()? {
            let touched = sf.touched_ms();
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
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        index_cycle(&tx, c, &rel, &hash)?;
        tx.commit()?;
        Ok(())
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
            ("fact", "SELECT id, scope, key, value, confidence, sources_json, supersedes_id, viewer_id, status, importance, created_ms, updated_ms, last_recalled_ms, last_decay_ms, last_used_ms FROM mem_fact ORDER BY id"),
            ("tombstone", "SELECT scope, key, ts_ms, reason FROM mem_fact_tombstone ORDER BY scope, key"),
            ("procedural", "SELECT id, key, rule, sources_json, supersedes_id, status, importance, created_ms, updated_ms, last_used_ms, last_decay_ms FROM mem_procedural ORDER BY id"),
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
    viewer_id, status, importance, created_ms, updated_ms, last_recalled_ms, last_decay_ms, \
    last_used_ms";

const HABIT_COLS: &str = "id, key, rule, sources_json, supersedes_id, status, importance, \
    created_ms, updated_ms, last_used_ms, last_decay_ms";

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
        last_used_ms: r.get(14)?,
    }))
}

fn row_habit(r: &rusqlite::Row<'_>) -> RowResult<Procedural> {
    let status: String = r.get(5)?;
    let status = match HabitStatus::parse(&status) {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(Procedural {
        id: r.get(0)?,
        key: r.get(1)?,
        rule: r.get(2)?,
        sources: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
        supersedes: r.get(4)?,
        status,
        importance: r.get::<_, i64>(6)?.clamp(0, 5) as u8,
        created_ms: r.get(7)?,
        updated_ms: r.get(8)?,
        last_used_ms: r.get(9)?,
        last_decay_ms: r.get(10)?,
    }))
}

fn habit_doc(p: &Procedural) -> Doc {
    let mut doc = Doc::new();
    doc.set("id", &p.id)
        .set("key", &p.key)
        .set("status", p.status.as_str())
        .set("importance", p.importance)
        .set("created_ms", p.created_ms)
        .set("updated_ms", p.updated_ms)
        .set("last_used_ms", p.last_used_ms)
        .set("last_decay_ms", p.last_decay_ms)
        .set("sources", json_list(&p.sources));
    if let Some(s) = &p.supersedes {
        doc.set("supersedes", s);
    }
    doc.body = p.rule.clone();
    doc
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
        .set("last_used_ms", sf.last_used_ms)
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
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
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
            sf.last_decay_ms,
            sf.last_used_ms
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
        &format!(
            "INSERT OR REPLACE INTO mem_procedural ({HABIT_COLS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ),
        params![
            p.id,
            p.key,
            p.rule,
            json_list(&p.sources),
            p.supersedes,
            p.status.as_str(),
            p.importance,
            p.created_ms,
            p.updated_ms,
            p.last_used_ms,
            p.last_decay_ms
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
        p.updated_ms,
    )?;
    if p.status == HabitStatus::Live {
        set_fts(conn, &p.id, "procedural", Some((&p.rule, "procedural")))
    } else {
        set_fts(conn, &p.id, "procedural", None)
    }
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

/// An optional numeric frontmatter field: absent is `None`, present but not a number is an error.
fn opt_num(doc: &Doc, key: &str, path: &Path) -> Result<Option<TsMs>, BrainError> {
    match doc.get(key) {
        None => Ok(None),
        Some(_) => doc.num(key, path).map(Some),
    }
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
    let rel = |p: &Path| rel_path(memory_dir, p);

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
            // Absent in files written before use-based aging: never used.
            last_used_ms: opt_num(&doc, "last_used_ms", &path)?.unwrap_or(0),
            viewer_id: viewer_of(scope, &key),
        };
        index_fact(&tx, &sf, &rel(&path), &hash)?;
        report.facts += 1;
    }

    for path in walk_md(&memory_dir.join("procedurals"))? {
        let (doc, hash) = read_doc(&path)?;
        // Files written before the habit lifecycle carry no key or status: the id is the key
        // (the file already sits at `procedurals/<id>.md`) and the habit is live.
        let id = doc.req("id", &path)?.to_string();
        let created_ms: TsMs = doc.num("created_ms", &path)?;
        let p = Procedural {
            key: doc.get("key").map_or_else(|| id.clone(), str::to_string),
            rule: doc.body.clone(),
            sources: serde_json::from_str(doc.req("sources", &path)?)
                .map_err(|e| BrainError::Store(format!("{}: sources: {e}", path.display())))?,
            supersedes: doc.get("supersedes").map(str::to_string),
            status: HabitStatus::parse(doc.get("status").unwrap_or("live"))?,
            importance: doc.num("importance", &path)?,
            created_ms,
            updated_ms: opt_num(&doc, "updated_ms", &path)?.unwrap_or(created_ms),
            last_used_ms: opt_num(&doc, "last_used_ms", &path)?.unwrap_or(0),
            last_decay_ms: opt_num(&doc, "last_decay_ms", &path)?.unwrap_or(created_ms),
            id,
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

/// What a read-only comparison of the index against the markdown truth found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexCheck {
    /// Markdown counts.
    pub files: RebuildReport,
    /// Whether `index.sqlite` exists.
    pub index_present: bool,
    /// How the index disagrees with the markdown; empty when they agree.
    pub problems: Vec<String>,
}

impl IndexCheck {
    /// The index exists and matches every markdown file.
    pub fn in_step(&self) -> bool {
        self.index_present && self.problems.is_empty()
    }
}

/// Compare the SQLite index with the markdown truth under `memory_dir` without writing
/// anything: the index is opened read-only, never created, migrated or rebuilt. Checks:
/// SQLite `quick_check`, one index node per episode, fact, procedural and cycle file with the
/// same path and content hash, and the tombstone set. A markdown file that does not parse is
/// an error (a rebuild would fail on it too); every index-side disagreement is a problem.
///
/// [`rebuild_index`] is the repair, run by an explicit command, never by a check.
pub fn verify_index(memory_dir: &Path) -> Result<IndexCheck, BrainError> {
    use std::collections::BTreeMap;
    let mut check = IndexCheck::default();
    let rel = |p: &Path| rel_path(memory_dir, p);
    // (kind, file_path) -> content_hash, from the markdown.
    let mut want: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut want_tombs: BTreeSet<(String, String)> = BTreeSet::new();
    for (kind, dir) in [
        ("episode", "episodes"),
        ("fact", "facts"),
        ("procedural", "procedurals"),
        ("cycle", "cycles"),
    ] {
        let tomb_root = memory_dir.join("facts").join("_tombstones");
        for path in walk_md(&memory_dir.join(dir))? {
            let (doc, hash) = read_doc(&path)?;
            if kind == "fact" && path.starts_with(&tomb_root) {
                want_tombs.insert((
                    doc.req("scope", &path)?.to_string(),
                    doc.req("key", &path)?.to_string(),
                ));
                check.files.tombstones += 1;
                continue;
            }
            want.insert((kind.to_string(), rel(&path)), hash);
            match kind {
                "episode" => check.files.episodes += 1,
                "fact" => check.files.facts += 1,
                "procedural" => check.files.procedurals += 1,
                _ => check.files.cycles += 1,
            }
        }
    }

    let index = memory_dir.join(INDEX_FILE);
    if !index.is_file() {
        check.problems.push(format!("{INDEX_FILE} is missing"));
        return Ok(check);
    }
    check.index_present = true;
    let conn = match Connection::open_with_flags(
        &index,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            check.problems.push(format!("cannot open the index: {e}"));
            return Ok(check);
        }
    };
    let quick: Result<String, rusqlite::Error> =
        conn.query_row("PRAGMA quick_check", [], |r| r.get(0));
    match quick {
        Ok(s) if s == "ok" => {}
        Ok(s) => {
            check.problems.push(format!("integrity check: {s}"));
            return Ok(check);
        }
        Err(e) => {
            check.problems.push(format!("integrity check failed: {e}"));
            return Ok(check);
        }
    }

    let mut have: BTreeMap<(String, String), String> = BTreeMap::new();
    let nodes = conn
        .prepare("SELECT kind, file_path, content_hash FROM mem_node")
        .and_then(|mut st| {
            st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?
                .collect::<Result<Vec<((String, String), String)>, _>>()
        });
    match nodes {
        Ok(rows) => have.extend(rows),
        Err(e) => {
            check.problems.push(format!("cannot read index nodes: {e}"));
            return Ok(check);
        }
    }
    let tombs = conn
        .prepare("SELECT scope, key FROM mem_fact_tombstone")
        .and_then(|mut st| {
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<BTreeSet<(String, String)>, _>>()
        });
    let have_tombs = match tombs {
        Ok(t) => t,
        Err(e) => {
            check
                .problems
                .push(format!("cannot read index tombstones: {e}"));
            return Ok(check);
        }
    };

    let missing: Vec<&(String, String)> = want.keys().filter(|k| !have.contains_key(*k)).collect();
    let extra: Vec<&(String, String)> = have.keys().filter(|k| !want.contains_key(*k)).collect();
    let changed: Vec<&(String, String)> = want
        .iter()
        .filter(|(k, h)| have.get(*k).is_some_and(|x| x != *h))
        .map(|(k, _)| k)
        .collect();
    for (what, keys) in [
        ("not indexed", &missing),
        ("indexed but absent from the markdown", &extra),
        ("changed since indexed", &changed),
    ] {
        if let Some((_, first)) = keys.first() {
            check
                .problems
                .push(format!("{} file(s) {what} (first: {first})", keys.len()));
        }
    }
    if want_tombs != have_tombs {
        check.problems.push(format!(
            "tombstones: {} in markdown, {} in the index",
            want_tombs.len(),
            have_tombs.len()
        ));
    }
    Ok(check)
}

#[cfg(test)]
mod verify_tests {
    use super::*;

    fn stamp(p: &Path) -> (Vec<u8>, std::time::SystemTime) {
        (
            fs::read(p).unwrap(),
            fs::metadata(p).unwrap().modified().unwrap(),
        )
    }

    fn seeded() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        store
            .write_episode(&Episode {
                id: "ep_1".into(),
                ts_ms: 1_000,
                stream_id: "s1".into(),
                kind: EpisodeKind::Stream,
                text: "Gorbo begged for a title.".into(),
                importance: 3,
            })
            .unwrap();
        store
            .write_fact(
                &Fact {
                    id: String::new(),
                    scope: FactScope::Viewer,
                    key: "v42".into(),
                    value: "Begs for titles".into(),
                    confidence: 0.9,
                    sources: vec!["ep_1".into()],
                    supersedes: None,
                },
                3,
                2_000,
            )
            .unwrap();
        store
            .write_fact(
                &Fact {
                    id: String::new(),
                    scope: FactScope::Character,
                    key: "secret".into(),
                    value: "fears mirrors".into(),
                    confidence: 0.9,
                    sources: vec!["ep_1".into()],
                    supersedes: None,
                },
                3,
                2_500,
            )
            .unwrap();
        store
            .forget_fact(FactScope::Character, "secret", "operator request", 3_000)
            .unwrap();
        dir
    }

    #[test]
    fn verify_reads_without_touching_the_index() {
        let dir = seeded();
        let index = dir.path().join(INDEX_FILE);
        let before = stamp(&index);
        for _ in 0..2 {
            let c = verify_index(dir.path()).unwrap();
            assert!(c.in_step(), "{c:?}");
            assert_eq!(c.files.episodes, 1);
            assert_eq!(c.files.facts, 2);
            assert_eq!(c.files.tombstones, 1);
        }
        assert_eq!(stamp(&index), before, "bytes and mtime unchanged");
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!dir.path().join(format!("{INDEX_FILE}{suffix}")).exists());
        }
    }

    #[test]
    fn verify_reports_drift_and_rebuild_repairs_it() {
        let dir = seeded();
        // A markdown file the index never saw (written by hand, or by a crashed write).
        let ep = dir.path().join("episodes").join("1970").join("01");
        let src = walk_md(&ep).unwrap().remove(0);
        let text = fs::read_to_string(&src).unwrap().replace("ep_1", "ep_2");
        fs::write(ep.join("ep_2.md"), text).unwrap();
        let c = verify_index(dir.path()).unwrap();
        assert!(!c.in_step());
        assert!(c.problems[0].contains("1 file(s) not indexed"), "{c:?}");

        // An edited file: same path, different content.
        fs::write(&src, fs::read_to_string(&src).unwrap() + "\nedited\n").unwrap();
        let c = verify_index(dir.path()).unwrap();
        assert!(
            c.problems
                .iter()
                .any(|p| p.contains("changed since indexed")),
            "{c:?}"
        );

        rebuild_index(dir.path()).unwrap();
        assert!(verify_index(dir.path()).unwrap().in_step());

        // A missing or corrupt index is a problem, not an error, and is not created.
        fs::remove_file(dir.path().join(INDEX_FILE)).unwrap();
        let c = verify_index(dir.path()).unwrap();
        assert!(!c.index_present && !c.in_step());
        assert!(
            !dir.path().join(INDEX_FILE).exists(),
            "never created by a check"
        );
        fs::write(dir.path().join(INDEX_FILE), b"not a database").unwrap();
        let c = verify_index(dir.path()).unwrap();
        assert!(!c.in_step(), "{c:?}");
        assert_eq!(
            fs::read(dir.path().join(INDEX_FILE)).unwrap(),
            b"not a database"
        );
    }
}
