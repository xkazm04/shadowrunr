//! SEG-03 scenes on the daemon side (contract appendix "SEG-03 Table Read"): the output gate's
//! check over a scene, the `scene list|check|approve` commands, and the prep's `table_read`
//! check. The files are `<data>/scenes/<guest>/<id>.toml` (owner-local; `/data/` is ignored by
//! git); loading and the shape check are `shadowrunr_segments::scenes`.
//!
//! - **The gate.** Every fixed line (`text`, and a live slot's `fallback`) and the title and
//!   pitch (the ballot shows them) run the brain's output gate (`gate_text`, the policy the prep
//!   checks use). A term rule that blocks is `gate_blocked`; any other rule that fires (a bleep,
//!   a warned term, a character break, a persona leak, a personal-data ask, an unqualified
//!   claim) is `gate_recast`: the line would not air as written.
//! - **Approve** refuses while a scene has any problem, and sets `status = "approved"` by
//!   editing that one value in the file: the owner's formatting and comments stay.
//! - **The foils** a scene's guest must be one of are the configured character's (`[sim]
//!   character`); the prep's check uses the prep's character.
//! - Every command works on the files directly, like `shadowrunr prep`; no daemon is needed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use shadowrunr_brain::moderation::{gate_text, Policy};
use shadowrunr_segments::scenes::{load_guest, scenes_dir, LoadedScene};
use shadowrunr_types::scene::{Scene, SceneProblem, SceneStatus, SceneSummary, TableReadPlan};
use shadowrunr_types::ErrorCode;

use crate::config::Config;
use crate::rpc::RpcError;
use crate::runs;

/// A table read plans this many scenes, at least and at most.
pub const TABLE_READ_SCENES: (usize, usize) = (1, 3);

/// `shadowrunr scene` exit codes (also in docs/guide/cli.md).
pub const SCENE_EXIT_CODES: &str = "Exit codes:
  0  done (check: the scene has no problem)
  1  refused or failed (approve: the scene has a problem; a scene that does not exist; I/O)
  2  usage error (a malformed <guest>/<id>)
  6  check: the scene has a problem (shown; it cannot be approved)";

/// Exit code: `scene check` found a problem.
pub const EXIT_PROBLEMS: u8 = 6;

#[derive(Debug, clap::Subcommand)]
pub enum SceneCommand {
    /// Every scene in <data>/scenes/ (or one guest's), with its status and problems.
    #[command(after_help = SCENE_EXIT_CODES)]
    List {
        /// Only this guest's scenes (a foil id).
        #[arg(long)]
        guest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// One scene's shape and output-gate problems.
    #[command(after_help = SCENE_EXIT_CODES)]
    Check {
        /// `<guest>/<id>`, e.g. `the-painter/the-gallery`.
        scene: String,
        #[arg(long)]
        json: bool,
    },
    /// The owner approves a scene for the ballots: refused while it has any problem. Sets
    /// `status = "approved"` in the file and changes nothing else in it.
    #[command(after_help = SCENE_EXIT_CODES)]
    Approve {
        /// `<guest>/<id>`.
        scene: String,
        #[arg(long)]
        json: bool,
    },
}

/// The foils `character` declares (none when its package does not load).
pub fn foils_of(cfg: &Config, character: &str) -> Vec<String> {
    crate::character::manifest(&cfg.paths.characters, character)
        .map(|m| m.foils)
        .unwrap_or_default()
}

fn problem(line: Option<u32>, code: &str, detail: String) -> SceneProblem {
    SceneProblem {
        line,
        code: code.into(),
        detail,
    }
}

/// What the output gate would do to `text`, as a problem (`None` when it airs as written).
fn gated(line: Option<u32>, what: &str, text: &str, policy: &Policy) -> Option<SceneProblem> {
    let (_, fired) = gate_text(text, policy, &[]);
    if fired.is_empty() {
        return None;
    }
    let blocked = fired
        .iter()
        .any(|(rule, action)| rule.starts_with("term:") && action == "replaced");
    let rules: Vec<String> = fired.iter().map(|(r, a)| format!("{r} {a}")).collect();
    Some(problem(
        line,
        if blocked {
            "gate_blocked"
        } else {
            "gate_recast"
        },
        format!("{what}: {}", rules.join(", ")),
    ))
}

/// The output gate's problems over `scene`: its title, pitch, fixed lines and fallbacks.
pub fn gate_problems(scene: &Scene, policy: &Policy) -> Vec<SceneProblem> {
    let mut out: Vec<SceneProblem> = [("title", &scene.title), ("pitch", &scene.pitch)]
        .into_iter()
        .filter_map(|(what, text)| gated(None, what, text, policy))
        .collect();
    for (i, l) in scene.lines.iter().enumerate() {
        let at = Some(i as u32);
        if let Some(t) = &l.text {
            out.extend(gated(at, "the line", t, policy));
        }
        if let Some(f) = &l.fallback {
            out.extend(gated(at, "the fallback", f, policy));
        }
    }
    out
}

/// `loaded` with the gate's problems added to its shape problems.
pub fn checked(mut loaded: LoadedScene, policy: &Policy) -> LoadedScene {
    let gate = gate_problems(&loaded.scene, policy);
    loaded.problems.extend(gate);
    loaded
}

/// Every scene of `guest`, shape- and gate-checked, sorted by id.
pub fn load_checked(
    data: &Path,
    guest: &str,
    foils: &[String],
    policy: &Policy,
) -> Vec<LoadedScene> {
    load_guest(data, guest, foils)
        .into_iter()
        .map(|l| checked(l, policy))
        .collect()
}

/// The guests with a folder in `<data>/scenes/`, sorted.
pub fn guests(data: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(scenes_dir(data)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| runs::check_id("guest", n).is_ok())
        .collect();
    out.sort();
    out
}

/// A scene's list row.
pub fn summary(l: &LoadedScene) -> SceneSummary {
    SceneSummary {
        id: l.scene.id.clone(),
        guest: l.scene.guest.clone(),
        title: l.scene.title.clone(),
        pitch: l.scene.pitch.clone(),
        status: l.scene.status,
        lines: l.scene.lines.len() as u32,
        live_slots: l.scene.lines.iter().filter(|x| x.live.is_some()).count() as u32,
        problems: l.problems.clone(),
        fictional: l.scene.fictional,
    }
}

/// `<guest>/<id>`, both plain ids.
pub fn parse_ref(s: &str) -> Result<(String, String), RpcError> {
    let Some((guest, id)) = s.split_once('/') else {
        return Err(RpcError::invalid(format!(
            "scene {s:?} must be <guest>/<id>, e.g. the-painter/the-gallery"
        )));
    };
    runs::check_id("guest", guest)?;
    runs::check_id("scene", id)?;
    Ok((guest.to_string(), id.to_string()))
}

/// `<data>/scenes/<guest>/<id>.toml`.
pub fn scene_path(data: &Path, guest: &str, id: &str) -> PathBuf {
    scenes_dir(data).join(guest).join(format!("{id}.toml"))
}

/// The file `<guest>/<id>.toml` among `scenes` (its file stem, whatever id it declares).
fn by_stem<'a>(scenes: &'a [LoadedScene], id: &str) -> Option<&'a LoadedScene> {
    scenes
        .iter()
        .find(|l| l.path.file_stem().and_then(|s| s.to_str()) == Some(id))
}

/// One scene, checked against the configured character's foils.
pub fn find(cfg: &Config, guest: &str, id: &str) -> Result<LoadedScene, RpcError> {
    runs::check_id("guest", guest)?;
    runs::check_id("scene", id)?;
    let policy = crate::clips::policy(cfg)?;
    let foils = foils_of(cfg, &cfg.sim.character);
    let all = load_checked(&cfg.paths.data, guest, &foils, &policy);
    by_stem(&all, id).cloned().ok_or_else(|| {
        RpcError::not_found(format!(
            "scene {guest}/{id}: no {}",
            scene_path(&cfg.paths.data, guest, id).display()
        ))
    })
}

/// `text` with its top-level `status = "draft"` set to `"approved"`, the quotes, spacing and
/// any comment on the line kept and every other byte untouched. `Ok(None)` when it is approved
/// already.
pub fn set_approved(text: &str) -> Result<Option<String>, String> {
    let mut out = String::with_capacity(text.len() + 3);
    let mut found = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if found.is_none() && t.starts_with('[') {
            // Past the top-level keys: a `status` in a table is not the scene's.
            found = Some(false);
        }
        if found.is_none() {
            if let Some(edited) = status_line(line)? {
                found = Some(true);
                match edited {
                    Some(e) => out.push_str(&e),
                    None => return Ok(None),
                }
                continue;
            }
        }
        out.push_str(line);
    }
    if found != Some(true) {
        return Err("the file has no top-level `status` line".into());
    }
    Ok(Some(out))
}

/// For a `status = ...` line: `Some(Some(edited))` for a draft, `Some(None)` when approved;
/// `None` for any other line.
fn status_line(line: &str) -> Result<Option<Option<String>>, String> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let Some(after_key) = rest
        .strip_prefix("status")
        .or_else(|| rest.strip_prefix("\"status\""))
    else {
        return Ok(None);
    };
    let eq = after_key.trim_start();
    let Some(after_eq) = eq.strip_prefix('=') else {
        return Ok(None);
    };
    let value = after_eq.trim_start();
    let head = line.len() - value.len();
    for q in ['"', '\''] {
        for (word, approved) in [("draft", false), ("approved", true)] {
            let token = format!("{q}{word}{q}");
            if value.starts_with(&token) {
                if approved {
                    return Ok(Some(None));
                }
                let mut e = String::with_capacity(line.len() + 3);
                e.push_str(&line[..head]);
                e.push_str(&format!("{q}approved{q}"));
                e.push_str(&value[token.len()..]);
                return Ok(Some(Some(e)));
            }
        }
    }
    Err(format!(
        "`status` is not \"draft\" or \"approved\": {}",
        line.trim_end()
    ))
}

/// What `scene approve` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approved {
    Now,
    Already,
}

/// Approve `<guest>/<id>`: refused (`conflict`) while it has any problem.
pub fn approve(cfg: &Config, guest: &str, id: &str) -> Result<(LoadedScene, Approved), RpcError> {
    let mut l = find(cfg, guest, id)?;
    if !l.problems.is_empty() {
        let codes: Vec<String> = l.problems.iter().map(|p| p.code.clone()).collect();
        return Err(RpcError::new(
            ErrorCode::Conflict,
            format!(
                "scene {guest}/{id} has {} problem(s) ({}): fix them first (`scene check`)",
                l.problems.len(),
                codes.join(", ")
            ),
        )
        .with_detail(serde_json::json!({ "problems": l.problems })));
    }
    if l.scene.status == SceneStatus::Approved {
        return Ok((l, Approved::Already));
    }
    let io = |e: std::io::Error| RpcError::io(format!("{}: {e}", l.path.display()));
    let text = std::fs::read_to_string(&l.path).map_err(io)?;
    let Some(new) = set_approved(&text)
        .map_err(|why| RpcError::invalid(format!("{}: {why}", l.path.display())))?
    else {
        return Ok((l, Approved::Already));
    };
    // The edit must read back as the same scene, approved.
    let mut want = l.scene.clone();
    want.status = SceneStatus::Approved;
    match toml::from_str::<Scene>(&new) {
        Ok(s) if s == want => {}
        _ => {
            return Err(RpcError::internal(format!(
                "{}: the status edit did not read back; the file is unchanged",
                l.path.display()
            )))
        }
    }
    write_atomic(&l.path, &new).map_err(io)?;
    l.scene.status = SceneStatus::Approved;
    Ok((l, Approved::Now))
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The prep's `table_read` problems, each a sentence for the owner: the guest is a foil of
/// `character` (`foils`), the plan names 1-3 distinct scenes, and each is in
/// `<data>/scenes/<guest>/`, approved and free of shape and gate problems.
pub fn table_read_problems(
    plan: &TableReadPlan,
    character: &str,
    data: &Path,
    foils: &[String],
    policy: &Policy,
) -> Vec<String> {
    let mut out = Vec::new();
    let guest = plan.guest.as_str();
    if runs::check_id("guest", guest).is_err() {
        out.push(format!("guest {guest:?} is not a foil id"));
        return out;
    }
    if !foils.iter().any(|f| f == guest) {
        out.push(format!("guest {guest} is not a foil of {character}"));
    }
    let n = plan.scenes.len();
    let (lo, hi) = TABLE_READ_SCENES;
    if !(lo..=hi).contains(&n) {
        out.push(format!("{n} scenes ({lo}-{hi})"));
    }
    let scenes = load_checked(data, guest, foils, policy);
    let mut seen: Vec<&str> = Vec::new();
    for id in &plan.scenes {
        if seen.contains(&id.as_str()) {
            out.push(format!("{id} is planned twice"));
            continue;
        }
        seen.push(id);
        if runs::check_id("scene", id).is_err() {
            out.push(format!("{id:?} is not a scene id"));
            continue;
        }
        let Some(l) = by_stem(&scenes, id) else {
            out.push(format!("{id} is not in scenes/{guest}/"));
            continue;
        };
        if l.scene.status != SceneStatus::Approved {
            out.push(format!("{id} is a draft (`scene approve {guest}/{id}`)"));
        }
        for p in &l.problems {
            let at = p
                .line
                .map(|i| format!(" line {}", i + 1))
                .unwrap_or_default();
            out.push(format!("{id}{at}: {} ({})", p.code, p.detail));
        }
    }
    out
}

// ── the CLI ────────────────────────────────────────────────────────────

fn print_json<T: serde::Serialize>(v: &T) {
    match serde_json::to_string_pretty(v) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn status_name(s: SceneStatus) -> &'static str {
    match s {
        SceneStatus::Draft => "draft",
        SceneStatus::Approved => "approved",
    }
}

/// One problem as a line: `line 3: gate_blocked: the line: term:x replaced`.
pub fn problem_text(p: &SceneProblem) -> String {
    match p.line {
        Some(i) => format!("line {}: {}: {}", i + 1, p.code, p.detail),
        None => format!("scene: {}: {}", p.code, p.detail),
    }
}

/// `scene list`'s rows.
pub fn list_rows(cfg: &Config, guest: Option<&str>) -> Result<Vec<SceneSummary>, RpcError> {
    let policy = crate::clips::policy(cfg)?;
    let foils = foils_of(cfg, &cfg.sim.character);
    let guests = match guest {
        Some(g) => {
            runs::check_id("guest", g)?;
            vec![g.to_string()]
        }
        None => guests(&cfg.paths.data),
    };
    Ok(guests
        .iter()
        .flat_map(|g| load_checked(&cfg.paths.data, g, &foils, &policy))
        .map(|l| summary(&l))
        .collect())
}

fn exit_for(e: &RpcError) -> u8 {
    match e.code {
        ErrorCode::InvalidParams => 2,
        _ => 1,
    }
}

/// Run one `scene` command; the exit code.
pub fn command(cfg: &Config, cmd: SceneCommand) -> u8 {
    let res: Result<u8, RpcError> = (|| match cmd {
        SceneCommand::List { guest, json } => {
            let rows = list_rows(cfg, guest.as_deref())?;
            if json {
                print_json(&rows);
            } else if rows.is_empty() {
                println!(
                    "no scenes: write <data>/scenes/<guest>/<id>.toml ({})",
                    scenes_dir(&cfg.paths.data).display()
                );
            } else {
                for r in &rows {
                    println!(
                        "{}/{}  {:<8}  {:>2} lines ({} live)  {}  {}",
                        r.guest,
                        r.id,
                        status_name(r.status),
                        r.lines,
                        r.live_slots,
                        if r.problems.is_empty() {
                            "ok".to_string()
                        } else {
                            format!("{} PROBLEMS", r.problems.len())
                        },
                        r.title
                    );
                }
            }
            Ok(0)
        }
        SceneCommand::Check { scene, json } => {
            let (guest, id) = parse_ref(&scene)?;
            let l = find(cfg, &guest, &id)?;
            if json {
                print_json(&summary(&l));
            } else {
                println!(
                    "{guest}/{id}  {}  {}",
                    status_name(l.scene.status),
                    l.scene.title
                );
                if l.problems.is_empty() {
                    println!("no problems");
                } else {
                    for p in &l.problems {
                        println!("  {}", problem_text(p));
                    }
                }
            }
            Ok(if l.problems.is_empty() {
                0
            } else {
                EXIT_PROBLEMS
            })
        }
        SceneCommand::Approve { scene, json } => {
            let (guest, id) = parse_ref(&scene)?;
            let (l, done) = approve(cfg, &guest, &id)?;
            if json {
                print_json(&summary(&l));
            } else if done == Approved::Already {
                println!("{guest}/{id} is already approved");
            } else {
                println!("{guest}/{id} approved");
            }
            Ok(0)
        }
    })();
    match res {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            if let Some(problems) = e.detail.as_ref().and_then(|d| d.get("problems")) {
                if let Ok(ps) = serde_json::from_value::<Vec<SceneProblem>>(problems.clone()) {
                    for p in &ps {
                        eprintln!("  {}", problem_text(p));
                    }
                }
            }
            exit_for(&e)
        }
    }
}

pub async fn run(cfg: &Config, cmd: SceneCommand) -> ExitCode {
    ExitCode::from(command(cfg, cmd))
}

#[cfg(test)]
mod tests;
