//! `shadowrunr.toml`: discovered upward from the working directory (or named with
//! `--config`). Relative paths in the file resolve against the file's own directory, so the
//! binary behaves the same from any subdirectory of the repo.
//!
//! Accounts are listed by label and `config_dir` only; no credential ever lives here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shadowrunr_engine::EngineConfig;
use shadowrunr_types::{Effort, ModelTier, SegmentId};

pub const FILE_NAME: &str = "shadowrunr.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub sim: SimDefaults,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub clips: ClipsConfig,
    /// `[research]`: the off-stream SEG-02A research pipeline (`shadowrunr case research`).
    #[serde(default)]
    pub research: crate::casework::ResearchConfig,
}

/// `[paths]`, as written in the file (relative to the file's directory unless absolute).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// Character packages (`<characters>/<id>/character.toml` ...).
    pub characters: String,
    /// Run output root; simulations land in `<runs>/sim/`.
    pub runs: String,
    /// Persistent state: memory store, directives, reconcile reports, the engine ledger.
    pub data: String,
    /// Holds `platform-rules.json`.
    pub policy: String,
    /// Holds `calibration.json`, `rubric.toml` and `matrices/`.
    pub sim: String,
    /// Segment tables hashed into the sim instrument.
    pub segments: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            characters: "characters".into(),
            runs: "runs".into(),
            data: "data".into(),
            policy: "policy".into(),
            sim: "sim".into(),
            segments: "crates/shadowrunr-segments/data".into(),
        }
    }
}

/// `[sim]`: defaults for `shadowrunr sim run` and the daemon's `sim.start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SimDefaults {
    pub character: String,
    pub tone: String,
    pub tier: ModelTier,
    pub effort: Effort,
    pub segment: SegmentId,
    pub audience: u32,
    pub seed: u64,
    pub duration_s: u32,
    pub tick_s: u32,
    /// Population used when `populations` has no entry for the audience size.
    pub population: String,
    /// Audience size -> population id (from `sim/calibration.json`).
    pub populations: BTreeMap<String, String>,
    /// Judge tiers in preference order; the first that differs from the core tier judges.
    pub judge_tiers: Vec<ModelTier>,
    pub concurrency: usize,
    pub max_featured: usize,
    pub featured_every_ticks: u32,
    pub pool_size: usize,
    /// Directory under `<runs>/sim/` for single runs (CLI `sim run` and daemon `sim.start`).
    pub adhoc_dir: String,
    /// Total quota pause a tournament may spend waiting for an account, in seconds; past it
    /// the tournament stops with exit code 4 (`sim tournament --max-pause` overrides it).
    pub max_quota_pause_s: u64,
}

impl Default for SimDefaults {
    fn default() -> Self {
        Self {
            character: "malakor".into(),
            tone: "unhinged-roastmaster".into(),
            tier: ModelTier::Sonnet,
            effort: Effort::Low,
            segment: SegmentId::Seg01ImprovisedRoom,
            audience: 30,
            seed: 1,
            duration_s: 600,
            tick_s: 5,
            population: "growing_channel".into(),
            populations: [
                ("3", "small_new_channel"),
                ("30", "growing_channel"),
                ("300", "raid_moment"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            judge_tiers: vec![ModelTier::Opus, ModelTier::Sonnet],
            concurrency: 2,
            max_featured: 8,
            featured_every_ticks: 1,
            pool_size: 2,
            adhoc_dir: "adhoc".into(),
            max_quota_pause_s: shadowrunr_sim::pacing::DEFAULT_MAX_PAUSE_MS / 1000,
        }
    }
}

impl SimDefaults {
    /// The population for an audience size: an exact `populations` entry, else the entry
    /// nearest on a log scale, else `population`.
    pub fn population_for(&self, audience: u32) -> String {
        if let Some(p) = self.populations.get(&audience.to_string()) {
            return p.clone();
        }
        let target = f64::from(audience.max(1)).ln();
        self.populations
            .iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|n| (n, v)))
            .min_by(|(a, _), (b, _)| {
                let da = (f64::from((*a).max(1)).ln() - target).abs();
                let db = (f64::from((*b).max(1)).ln() - target).abs();
                da.total_cmp(&db)
            })
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| self.population.clone())
    }
}

/// `[daemon]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub port: u16,
    /// Concurrent `sim.start` runs allowed (each spends quota on a live engine).
    pub max_live_runs: usize,
    /// Extra exact browser origins allowed besides `http://localhost:*`,
    /// `tauri://localhost` and `http://tauri.localhost`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_origins: Vec<String>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            port: 7717,
            max_live_runs: 2,
            extra_origins: Vec::new(),
        }
    }
}

/// `[clips]`: where the clip factory finds its recording tools. Every key is optional; a
/// missing tool makes a render fall back to a poster frame plus caption tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipsConfig {
    /// The stage app: its `dist/` build and `scripts/record-clip.mjs`.
    pub stage: String,
    /// Commands (or paths) for node, ffmpeg and ffprobe.
    pub node: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    /// A headless-capable Chromium; the usual Chrome, Edge and Playwright install locations
    /// are searched when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<String>,
}

impl Default for ClipsConfig {
    fn default() -> Self {
        Self {
            stage: "apps/stage".into(),
            node: "node".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            chrome: None,
        }
    }
}

/// Absolute paths, resolved from [`PathsConfig`].
#[derive(Debug, Clone)]
pub struct Paths {
    pub characters: PathBuf,
    pub runs: PathBuf,
    pub data: PathBuf,
    pub policy: PathBuf,
    pub sim: PathBuf,
    pub segments: PathBuf,
}

impl Paths {
    /// `<runs>/sim`: tournaments and ad-hoc runs.
    pub fn sim_runs(&self) -> PathBuf {
        self.runs.join("sim")
    }

    /// `<data>/memory`: the brain's memory store.
    pub fn memory(&self) -> PathBuf {
        self.data.join("memory")
    }
}

/// The loaded configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Directory the relative paths resolve against (the config file's directory, or the
    /// working directory when no file was found).
    pub root: PathBuf,
    /// The file that was loaded, if any.
    pub source: Option<PathBuf>,
    pub engine: EngineConfig,
    pub paths: Paths,
    pub sim: SimDefaults,
    pub daemon: DaemonConfig,
    /// `[clips]`, with `stage` and `chrome` resolved against the root.
    pub clips: ClipsConfig,
    /// `[research]`.
    pub research: crate::casework::ResearchConfig,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

/// Walk up from `start` looking for `shadowrunr.toml`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let candidate = d.join(FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

fn resolve(root: &Path, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

impl Config {
    /// Load `explicit`, or discover the file upward from `cwd`, or fall back to defaults
    /// rooted at `cwd`.
    pub fn load(explicit: Option<&Path>, cwd: &Path) -> Result<Self, ConfigError> {
        let file = match explicit {
            Some(p) => {
                let p = resolve(cwd, &p.to_string_lossy());
                if !p.is_file() {
                    return Err(ConfigError(format!("config {} not found", p.display())));
                }
                Some(p)
            }
            None => discover(cwd),
        };
        match file {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
                let root = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| cwd.to_path_buf());
                let mut cfg = Self::from_toml(&text, &root)
                    .map_err(|e| ConfigError(format!("{}: {}", path.display(), e.0)))?;
                cfg.source = Some(path);
                Ok(cfg)
            }
            None => Ok(Self::from_file_config(FileConfig::default(), cwd)),
        }
    }

    pub fn from_toml(text: &str, root: &Path) -> Result<Self, ConfigError> {
        let fc: FileConfig = toml::from_str(text).map_err(|e| ConfigError(e.to_string()))?;
        Ok(Self::from_file_config(fc, root))
    }

    pub fn from_file_config(fc: FileConfig, root: &Path) -> Self {
        let p = &fc.paths;
        let paths = Paths {
            characters: resolve(root, &p.characters),
            runs: resolve(root, &p.runs),
            data: resolve(root, &p.data),
            policy: resolve(root, &p.policy),
            sim: resolve(root, &p.sim),
            segments: resolve(root, &p.segments),
        };
        let mut engine = fc.engine;
        // The process-wide engine ledger lives under <data>, never in whatever directory the
        // binary was started from. Sim runs keep their own per-run ledger on top of it.
        engine.ledger_path = Some(match &engine.ledger_path {
            Some(l) => resolve(root, l).to_string_lossy().into_owned(),
            None => paths
                .data
                .join("engine-ledger.jsonl")
                .to_string_lossy()
                .into_owned(),
        });
        for a in &mut engine.accounts {
            if let Some(d) = &a.config_dir {
                a.config_dir = Some(resolve(root, d).to_string_lossy().into_owned());
            }
        }
        Self {
            root: root.to_path_buf(),
            source: None,
            engine,
            paths,
            sim: fc.sim,
            daemon: fc.daemon,
            clips: ClipsConfig {
                stage: resolve(root, &fc.clips.stage)
                    .to_string_lossy()
                    .into_owned(),
                chrome: fc
                    .clips
                    .chrome
                    .as_deref()
                    .map(|c| resolve(root, c).to_string_lossy().into_owned()),
                ..fc.clips
            },
            research: fc.research,
        }
    }

    /// The clip factory's recording tools.
    pub fn render_tools(&self) -> shadowrunr_clips::RenderTools {
        shadowrunr_clips::RenderTools {
            stage_dir: PathBuf::from(&self.clips.stage),
            node: self.clips.node.clone(),
            ffmpeg: self.clips.ffmpeg.clone(),
            ffprobe: self.clips.ffprobe.clone(),
            chrome: self.clips.chrome.as_ref().map(PathBuf::from),
        }
    }

    /// The clip store under `<data>/clips`.
    pub fn clip_store(&self) -> shadowrunr_clips::ClipStore {
        shadowrunr_clips::ClipStore::new(&self.paths.data)
    }

    /// Simulation options built from `[paths]` and `[sim]`.
    pub fn sim_options(&self, judge: bool) -> shadowrunr_sim::SimOptions {
        shadowrunr_sim::SimOptions {
            judge,
            judge_tiers: self.sim.judge_tiers.clone(),
            tick_s: self.sim.tick_s,
            characters_root: self.paths.characters.clone(),
            sim_root: self.paths.sim.clone(),
            segments_data_dir: self.paths.segments.clone(),
            max_featured: self.sim.max_featured,
            featured_every_ticks: self.sim.featured_every_ticks,
            pool_size: self.sim.pool_size,
            ..shadowrunr_sim::SimOptions::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::Role;

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../shadowrunr.example.toml");
        let cfg = Config::from_toml(text, Path::new("/repo")).unwrap();
        assert_eq!(cfg.engine.accounts.len(), 1);
        assert_eq!(cfg.engine.accounts[0].label, "main");
        assert!(cfg.engine.accounts[0].config_dir.is_none());
        assert!(!cfg.engine.scripted);
        assert_eq!(cfg.daemon.port, 7717);
        assert_eq!(cfg.sim.character, "malakor");
        assert_eq!(cfg.sim.max_quota_pause_s, 28_800);
        assert_eq!(cfg.paths.characters, Path::new("/repo").join("characters"));
        assert_eq!(
            cfg.paths.sim_runs(),
            Path::new("/repo").join("runs").join("sim")
        );
    }

    #[test]
    fn relative_paths_resolve_against_the_file() {
        let cfg = Config::from_toml(
            r#"
[paths]
characters = "chars"
[engine]
accounts = [{ label = "alt", config_dir = "accounts/alt" }]
[engine.routes]
core_thread = { tier = "sonnet" }
"#,
            Path::new("/r"),
        )
        .unwrap();
        assert_eq!(cfg.paths.characters, Path::new("/r").join("chars"));
        assert_eq!(cfg.paths.data, Path::new("/r").join("data"));
        assert_eq!(
            cfg.engine.accounts[0]
                .config_dir
                .as_deref()
                .map(PathBuf::from),
            Some(Path::new("/r").join("accounts/alt"))
        );
        assert_eq!(cfg.engine.route(Role::CoreThread).0, ModelTier::Sonnet);
        assert_eq!(
            cfg.engine.ledger_path.as_deref().map(PathBuf::from),
            Some(Path::new("/r").join("data").join("engine-ledger.jsonl"))
        );
    }

    #[test]
    fn discovery_walks_upward() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(FILE_NAME), "[daemon]\nport = 9000\n").unwrap();
        let deep = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        let cfg = Config::load(None, &deep).unwrap();
        assert_eq!(cfg.daemon.port, 9000);
        assert_eq!(cfg.root, tmp.path());
    }

    #[test]
    fn max_quota_pause_defaults_to_eight_hours_and_reads_the_file() {
        assert_eq!(SimDefaults::default().max_quota_pause_s, 28_800);
        let cfg = Config::from_toml("[sim]\nmax_quota_pause_s = 60\n", Path::new("/r")).unwrap();
        assert_eq!(cfg.sim.max_quota_pause_s, 60);
        assert_eq!(
            cfg.sim.character, "malakor",
            "other [sim] keys keep their defaults"
        );
    }

    #[test]
    fn population_falls_back_to_nearest_size() {
        let s = SimDefaults::default();
        assert_eq!(s.population_for(30), "growing_channel");
        assert_eq!(s.population_for(250), "raid_moment");
        assert_eq!(s.population_for(1), "small_new_channel");
    }
}
