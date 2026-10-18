//! Catalogue RPCs: `character.list`, `character.get`, `character.rules[.set]`,
//! `character.tone.set`, `segments.tables` and `ranks.table`. Everything is read from the same sources the brain,
//! the segment runtime and the sim instrument use, so the operator sees what will run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shadowrunr_brain::rules::{self, RulesOverride};
use shadowrunr_brain::{load_character_with, BrainError, CharacterPackage};
use shadowrunr_segments::command::COMMAND_SPECS;
use shadowrunr_segments::ranks::LADDER;
use shadowrunr_segments::table::{self, Guard, Trigger, COMMAND_NAMES, OP_TAGS};
use shadowrunr_segments::RankTable;
use shadowrunr_types::catalog::*;

use crate::config::Config;
use crate::rpc::RpcError;
use crate::runs;

/// `<characters>/<id>/`, which must hold a `character.toml`.
pub fn character_dir(cfg: &Config, character: &str) -> Result<PathBuf, RpcError> {
    runs::check_id("character", character)?;
    let dir = cfg.paths.characters.join(character);
    if !dir.join("character.toml").is_file() {
        return Err(RpcError::not_found(format!("character {character}")));
    }
    Ok(dir)
}

fn rule_state(r: &rules::RuleState) -> RuleState {
    RuleState {
        id: r.id.clone(),
        title: r.title.clone(),
        enabled: r.enabled,
        default: r.default,
    }
}

/// `character.rules`: the package's optional rules with the operator's override applied.
pub fn character_rules(cfg: &Config, character: &str) -> Result<Vec<RuleState>, RpcError> {
    let dir = character_dir(cfg, character)?;
    let over = rules::override_path(&cfg.paths.data, character);
    let list = rules::load_rules(&dir, &RulesOverride::File(over))
        .map_err(|e| RpcError::io(e.to_string()))?;
    Ok(list
        .iter()
        .map(|r| rule_state(&rules::RuleState::from(r)))
        .collect())
}

/// `character.rules.set`: write one rule into `<data>/characters/<id>/rules.json`.
pub fn character_rules_set(
    cfg: &Config,
    character: &str,
    id: &str,
    enabled: bool,
) -> Result<Vec<RuleState>, RpcError> {
    let dir = character_dir(cfg, character)?;
    let over = rules::override_path(&cfg.paths.data, character);
    let list = rules::set_rule(&dir, character, &over, id, enabled).map_err(|e| match e {
        BrainError::Rejected(m) => RpcError::not_found(m),
        other => RpcError::io(other.to_string()),
    })?;
    Ok(list.iter().map(rule_state).collect())
}

/// The package with its default tone and the operator's rule override.
fn load(cfg: &Config, id: &str) -> Result<CharacterPackage, RpcError> {
    let dir = character_dir(cfg, id)?;
    let over = RulesOverride::File(rules::override_path(&cfg.paths.data, id));
    load_character_with(&dir, "", &over).map_err(|e| RpcError::io(format!("character {id}: {e}")))
}

/// `character.toml` fields the brain's loader does not keep.
#[derive(Deserialize, Default)]
struct Display {
    #[serde(default)]
    tagline: Option<String>,
}

fn tagline(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("character.toml")).ok()?;
    let d: Display = toml::from_str(&text).ok()?;
    d.tagline
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// `<data>/characters/<id>/active.json`: the operator's choices for a character.
#[derive(Serialize, Deserialize, Default)]
struct Active {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tone: Option<String>,
}

fn active_path(cfg: &Config, id: &str) -> PathBuf {
    cfg.paths
        .data
        .join("characters")
        .join(id)
        .join("active.json")
}

/// The operator's tone when it names one of `tones`, else `default_tone`. A stale override
/// (its tone file was removed) falls back rather than failing the catalogue.
fn active_tone(cfg: &Config, id: &str, default_tone: &str, tones: &[String]) -> String {
    std::fs::read_to_string(active_path(cfg, id))
        .ok()
        .and_then(|t| serde_json::from_str::<Active>(&t).ok())
        .and_then(|a| a.tone)
        .filter(|t| tones.iter().any(|x| x == t))
        .unwrap_or_else(|| default_tone.to_string())
}

fn tone_ids(dir: &Path) -> Vec<String> {
    markdown_files(&dir.join("tones"))
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// `character.tone.set`: write the operator's tone into `<data>/characters/<id>/active.json`
/// and return the updated summary. The tone must be one of the package's `tones/*.md`.
pub fn character_tone_set(
    cfg: &Config,
    id: &str,
    tone: &str,
) -> Result<CharacterSummary, RpcError> {
    let dir = character_dir(cfg, id)?;
    if !tone_ids(&dir).iter().any(|t| t == tone) {
        return Err(RpcError::not_found(format!(
            "character {id} has no tone {tone:?}"
        )));
    }
    let path = active_path(cfg, id);
    let mut active: Active = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    active.tone = Some(tone.to_string());
    let text = serde_json::to_string_pretty(&active)
        .map_err(|e| RpcError::internal(format!("active.json: {e}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| RpcError::io(format!("{}: {e}", parent.display())))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n")
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    summary(cfg, id)
}

fn read_text(path: &Path) -> Result<String, RpcError> {
    std::fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
}

/// `(id, path)` of every `<dir>/*.md`, sorted by id.
fn markdown_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
                .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every file under `dir`, relative with `/` separators, sorted.
fn package_files(dir: &Path) -> Vec<PackageFile> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<PackageFile>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(root, &p, depth + 1, out);
            } else if p.is_file() {
                let rel = p.strip_prefix(root).unwrap_or(&p);
                let path = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(PackageFile { path, bytes });
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[derive(Deserialize)]
struct SituationsFile {
    #[serde(default)]
    situations: Vec<Situation>,
}

fn situations(dir: &Path) -> Result<Vec<Situation>, RpcError> {
    let path = dir.join("situations.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file: SituationsFile = serde_json::from_str(&read_text(&path)?)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    Ok(file.situations)
}

fn summary(cfg: &Config, id: &str) -> Result<CharacterSummary, RpcError> {
    let pkg = load(cfg, id)?;
    let tones = tone_ids(&pkg.dir);
    Ok(CharacterSummary {
        rules_enabled: pkg.rules.iter().filter(|r| r.enabled).count() as u32,
        rules_total: pkg.rules.len() as u32,
        tagline: tagline(&pkg.dir),
        active_tone: active_tone(cfg, &pkg.id, &pkg.default_tone, &tones),
        id: pkg.id,
        name: pkg.name,
        default_tone: pkg.default_tone,
        tones,
        foils: pkg.foil_ids,
    })
}

/// `character.list`: every package under `<characters>/`, sorted by id. A package that does
/// not load is left out and reported on stderr (`character.get` names its error).
pub fn character_list(cfg: &Config) -> Vec<CharacterSummary> {
    let mut ids: Vec<String> = std::fs::read_dir(&cfg.paths.characters)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.path().join("character.toml").is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.iter()
        .filter_map(|id| match summary(cfg, id) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("character.list: skipping {id}: {e}");
                None
            }
        })
        .collect()
}

/// `character.get`: package files, tones, foils, situations and rules with their state.
pub fn character_get(cfg: &Config, id: &str) -> Result<CharacterDetail, RpcError> {
    let pkg = load(cfg, id)?;
    let tones = markdown_files(&pkg.dir.join("tones"))
        .into_iter()
        .map(|(id, path)| {
            Ok(ToneInfo {
                id,
                text: read_text(&path)?,
            })
        })
        .collect::<Result<Vec<_>, RpcError>>()?;
    let foils = pkg
        .foils
        .iter()
        .map(|f| FoilInfo {
            declared: pkg.foil_ids.contains(&f.id),
            id: f.id.clone(),
            text: f.text.clone(),
        })
        .collect();
    let rules = pkg
        .rules
        .iter()
        .map(|r| CharacterRuleInfo {
            id: r.id.clone(),
            title: r.title.clone(),
            enabled: r.enabled,
            default: r.default,
            text: r.text.clone(),
        })
        .collect();
    let rank_titles: BTreeMap<_, _> = LADDER
        .iter()
        .map(|r| (*r, pkg.rank_titles.title(*r)))
        .collect();
    let tone_list: Vec<String> = tones.iter().map(|t| t.id.clone()).collect();
    Ok(CharacterDetail {
        files: package_files(&pkg.dir),
        situations: situations(&pkg.dir)?,
        tagline: tagline(&pkg.dir),
        active_tone: active_tone(cfg, &pkg.id, &pkg.default_tone, &tone_list),
        id: pkg.id,
        name: pkg.name,
        default_tone: pkg.default_tone,
        constitution: pkg.constitution,
        bible: pkg.bible,
        tones,
        foils,
        rules,
        rank_titles,
    })
}

fn trigger_name(t: Trigger) -> &'static str {
    match t {
        Trigger::FoilVoteOpened => "foil_vote_opened",
        Trigger::FoilVoteClosed => "foil_vote_closed",
        Trigger::FoilSummoned => "foil_summoned",
        Trigger::FoilDismissed => "foil_dismissed",
        Trigger::PollOpened => "poll_opened",
        Trigger::PollClosed => "poll_closed",
        Trigger::HexRevealed => "hex_revealed",
        Trigger::HandoverDue => "handover_due",
    }
}

fn guard_name(g: Guard) -> &'static str {
    match g {
        Guard::FoilActive => "foil_active",
        Guard::NoFoil => "no_foil",
        Guard::AnyRevealed => "any_revealed",
        Guard::NoneRevealed => "none_revealed",
    }
}

/// `segments.tables`: the compiled-in tables the segment runtime interprets, and the chat
/// commands with the rank gate of `<segments>/ranks.toml`.
pub fn segment_tables(cfg: &Config) -> Result<SegmentTables, RpcError> {
    let path = cfg.paths.segments.join("ranks.toml");
    let ranks = RankTable::load_or_builtin(&path)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    let tables = table::tables().map_err(|e| RpcError::internal(e.to_string()))?;
    let segments = tables
        .iter()
        .map(|t| SegmentTableInfo {
            id: t.id,
            title: t.title.clone(),
            status: t.status,
            initial: t.initial.clone(),
            states: t.states.clone(),
            allowed_ops: t.allowed_ops.clone(),
            commands: t.commands.clone(),
            switch_to: t.switch_to.clone(),
            rules_prompt: t.rules_prompt.clone(),
            design_notes: t.design_notes.clone(),
            clock: ClockInfo {
                foil_vote_secs: t.clock.foil_vote_secs,
                poll_min_secs: t.clock.poll_min_secs,
                poll_max_secs: t.clock.poll_max_secs,
                max_poll_options: u32::try_from(t.clock.max_poll_options).unwrap_or(u32::MAX),
                handover: t.clock.handover.map(|h| HandoverInfo {
                    to: h.to,
                    ccv_min: h.ccv_min,
                    sustain_s: h.sustain_s,
                    max_elapsed_s: h.max_elapsed_s,
                    grace_s: h.grace_s,
                }),
            },
            transitions: t
                .transitions
                .iter()
                .map(|r| TransitionInfo {
                    from: r.from.clone(),
                    on: trigger_name(r.on).into(),
                    guard: r.guard.map(|g| guard_name(g).into()),
                    to: r.to.clone(),
                })
                .collect(),
        })
        .collect();
    Ok(SegmentTables {
        segments,
        op_tags: OP_TAGS.iter().map(|s| s.to_string()).collect(),
        command_names: COMMAND_NAMES.iter().map(|s| s.to_string()).collect(),
        commands: COMMAND_SPECS
            .iter()
            .map(|c| CommandInfo {
                name: c.name.to_string(),
                args: c.args.to_string(),
                what: c.what.to_string(),
                min_rank: c.rank_gated.then_some(ranks.propose_min_rank),
            })
            .collect(),
    })
}

/// `ranks.table`: `<segments>/ranks.toml` (the built-in copy when absent) with the titles of
/// `character` (default: `[sim] character`).
pub fn rank_table(cfg: &Config, character: Option<&str>) -> Result<RankTableInfo, RpcError> {
    let character = character.unwrap_or(&cfg.sim.character);
    let pkg = load(cfg, character)?;
    let path = cfg.paths.segments.join("ranks.toml");
    let table = RankTable::load_or_builtin(&path)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    Ok(RankTableInfo {
        character: pkg.id.clone(),
        propose_min_rank: table.propose_min_rank,
        ranks: table
            .ranks
            .iter()
            .map(|r| RankInfo {
                rank: r.rank,
                title: pkg.rank_titles.title(r.rank),
                weight: r.weight,
                min_gifted: r.min_gifted,
                tiers: r
                    .tiers
                    .iter()
                    .map(|t| TierRuleInfo {
                        tier: t.tier,
                        min_months: t.min_months,
                    })
                    .collect(),
            })
            .collect(),
    })
}
