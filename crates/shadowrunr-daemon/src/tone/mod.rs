//! The Tone Lab (docs/guide/characters.md § Tone Lab): the owner directs a character's voice
//! from the console. The daemon reads and writes two files of the package,
//! `lines/<tone>.toml` and `directions.md`, and never anything else or anywhere else: the
//! character comes through [`catalog::character_dir`], the tone must be one of the package's
//! `tones/*.md`, and every target is checked to resolve inside `paths.characters`. The owner
//! commits what it writes.
//!
//! - Every write is a text edit ([`bank`]) checked by parsing before and after, written through
//!   a temp file and a rename, then the whole package is validated (`character validate`): a
//!   write that adds an error is undone and refused (`invalid_params`, the issues in `detail`).
//!   Writes run one at a time.
//! - [`variants`]: the core model drafts candidates (`tone.variants`); nothing is written.
//! - [`inbox`]: Review feedback mapped to bank candidates and direction suggestions.

pub mod bank;
pub mod inbox;
pub mod variants;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use shadowrunr_brain::character::{lines_file, parse_exemplars, BankLine};
use shadowrunr_brain::moderation::gate_text;
use shadowrunr_types::catalog::{CharacterIssue, IssueSeverity};
use shadowrunr_types::tone::{
    LineStatus, ToneDirections, ToneDirectionsGetParams, ToneDirectionsSetParams, ToneInbox,
    ToneInboxParams, ToneLineWrite, ToneLines, ToneLinesAddParams, ToneLinesParams,
    ToneLinesSetStatusParams, ToneReviewItem, ToneReviewLineParams, ToneVariants,
    ToneVariantsParams,
};
use shadowrunr_types::{Engine, EngineRequest, ErrorCode, Role};

use crate::config::Config;
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::{catalog, character, review};

/// The package-relative directions file.
pub const DIRECTIONS_FILE: &str = "directions.md";
/// The longest `directions.md` the Tone Lab writes, in characters.
pub const MAX_DIRECTIONS_CHARS: usize = 20_000;
/// The longest line to improve or note `tone.variants` takes, in characters.
pub const MAX_INPUT_CHARS: usize = 2_000;
/// The most candidates one `tone.variants` call asks for.
pub const MAX_VARIANTS: u32 = 5;

/// One package write at a time (the Tone Lab's and the segment directions'), over every
/// package: the read, edit, write and validation of a file never interleave with another.
pub(crate) static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, RpcError> + Send + 'static,
) -> Result<T, RpcError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RpcError::internal(format!("tone task: {e}")))?
}

pub(crate) fn io(path: &Path, e: impl std::fmt::Display) -> RpcError {
    RpcError::io(format!("{}: {e}", path.display()))
}

/// `invalid_params` unless `path` resolves inside `root` (both canonicalised: a symlink or a
/// junction out of the characters directory is refused).
pub(crate) fn contained(root: &Path, path: &Path) -> Result<PathBuf, RpcError> {
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    let real = path.canonicalize().map_err(|e| io(path, e))?;
    if real.starts_with(&root) {
        Ok(real)
    } else {
        Err(RpcError::invalid(format!(
            "{} is outside the characters directory",
            path.display()
        )))
    }
}

/// The package dir of `character`, checked to be inside `paths.characters`.
pub fn package_dir(cfg: &Config, character: &str) -> Result<PathBuf, RpcError> {
    let dir = catalog::character_dir(cfg, character)?;
    contained(&cfg.paths.characters, &dir)?;
    Ok(dir)
}

/// `invalid_params` unless `tone` is a plain id naming one of the package's `tones/*.md`.
fn check_tone(dir: &Path, tone: &str) -> Result<(), RpcError> {
    let plain = !tone.is_empty()
        && tone.len() <= 100
        && tone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !plain {
        return Err(RpcError::invalid(format!(
            "tone {tone:?} must be [A-Za-z0-9_-]"
        )));
    }
    if !dir.join("tones").join(format!("{tone}.md")).is_file() {
        return Err(RpcError::not_found(format!(
            "tone {tone} (no tones/{tone}.md in the package)"
        )));
    }
    Ok(())
}

/// The bank file of `tone`: its path, checked to stay inside the characters directory once the
/// `lines/` dir exists.
fn bank_path(cfg: &Config, dir: &Path, tone: &str) -> Result<PathBuf, RpcError> {
    check_tone(dir, tone)?;
    let lines = dir.join("lines");
    if lines.exists() {
        contained(&cfg.paths.characters, &lines)?;
    }
    let path = lines.join(format!("{tone}.toml"));
    if path.exists() {
        contained(&cfg.paths.characters, &path)?;
    }
    Ok(path)
}

pub(crate) fn read_text(path: &Path) -> Result<Option<String>, RpcError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

/// Write through a synced temp file beside `path` and a rename.
fn write_durable(path: &Path, text: &str) -> Result<(), RpcError> {
    use std::io::Write;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{name}.tone-lab.tmp"));
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
    written.map_err(|e| io(path, e))
}

fn errors(issues: Vec<CharacterIssue>) -> Vec<CharacterIssue> {
    issues
        .into_iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .collect()
}

/// Write `text` to `path` (package-relative `file`), then validate the package. When the write
/// adds an error the file is put back as it was (or removed) and the call fails with
/// `invalid_params`, the new issues in `detail.issues`.
pub(crate) fn write_validated(
    dir: &Path,
    path: &Path,
    file: &str,
    text: &str,
) -> Result<(), RpcError> {
    let before_issues = errors(character::validate(dir));
    let before_text = read_text(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    write_durable(path, text)?;
    let added: Vec<CharacterIssue> = errors(character::validate(dir))
        .into_iter()
        .filter(|i| !before_issues.contains(i))
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    match &before_text {
        Some(t) => write_durable(path, t)?,
        None => std::fs::remove_file(path).map_err(|e| io(path, e))?,
    }
    let first = character::issue_line(&added[0]);
    Err(RpcError::invalid(format!(
        "refused: the write would make the package invalid ({first}); {file} is unchanged"
    ))
    .with_detail(serde_json::json!({ "issues": added })))
}

/// The bank as written; an absent file is an empty bank.
fn read_bank(path: &Path) -> Result<(Option<String>, Vec<BankLine>), RpcError> {
    match read_text(path)? {
        None => Ok((None, Vec::new())),
        Some(t) => {
            let f = bank::parse(&t.replace("\r\n", "\n")).map_err(|e| io(path, e))?;
            Ok((Some(t), f.lines))
        }
    }
}

// ── tone.lines.* ───────────────────────────────────────────────────────

pub fn lines_list(cfg: &Config, p: &ToneLinesParams) -> Result<ToneLines, RpcError> {
    let dir = package_dir(cfg, &p.character)?;
    let path = bank_path(cfg, &dir, &p.tone)?;
    let (_, lines) = read_bank(&path)?;
    Ok(ToneLines {
        character: p.character.clone(),
        tone: p.tone.clone(),
        file: lines_file(&p.tone),
        lines: lines.iter().map(bank::wire).collect(),
    })
}

pub fn lines_add(cfg: &Config, p: ToneLinesAddParams) -> Result<ToneLineWrite, RpcError> {
    let dir = package_dir(cfg, &p.character)?;
    let path = bank_path(cfg, &dir, &p.tone)?;
    let file = lines_file(&p.tone);
    let status = p.status.unwrap_or(LineStatus::Approved);
    if status == LineStatus::Retired {
        return Err(RpcError::invalid(
            "a new line is approved or a seed, never retired",
        ));
    }
    let text = bank::normalize(&p.text);
    if text.is_empty() {
        return Err(RpcError::invalid("text must not be empty"));
    }
    if text.chars().count() > bank::MAX_LINE_CHARS {
        return Err(RpcError::invalid(format!(
            "text is longer than {} characters",
            bank::MAX_LINE_CHARS
        )));
    }
    let mut tags: Vec<String> = Vec::new();
    for t in &p.tags {
        if !tags.iter().any(|x| x == t.as_str()) {
            tags.push(t.as_str().to_string());
        }
    }
    if tags.is_empty() {
        return Err(RpcError::invalid("tags needs at least one situation"));
    }
    let policy = crate::clips::policy(cfg)?;
    let (_, fired) = gate_text(&text, &policy, &[]);
    if let Some((rule, action)) = fired.first() {
        return Err(RpcError::invalid(format!(
            "the brain's output gate would change this line ({rule} {action}); a bank line must pass it unchanged"
        )));
    }

    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (current, lines) = read_bank(&path)?;
    if let Some(same) = lines
        .iter()
        .find(|l| bank::normalize(&l.text).to_lowercase() == text.to_lowercase())
    {
        return Err(RpcError::new(
            ErrorCode::Conflict,
            format!("the bank already has this line as {}", same.id),
        ));
    }
    let ids: Vec<String> = lines.iter().map(|l| l.id.clone()).collect();
    let line = BankLine {
        id: bank::next_id(&p.tone, &ids),
        status,
        tags,
        text,
    };
    let base = current.unwrap_or_else(|| bank::new_file(&p.tone));
    let next = bank::append(&base, &line).map_err(RpcError::internal)?;
    let persona = character::persona_of(&cfg.paths.characters, &p.character);
    if let Err(i) = parse_exemplars(&next.replace("\r\n", "\n"), &p.tone, &persona) {
        return Err(RpcError::invalid(format!("refused: {i}")));
    }
    write_validated(&dir, &path, &file, &next)?;
    Ok(ToneLineWrite {
        line: bank::wire(&line),
        file,
    })
}

pub fn lines_set_status(
    cfg: &Config,
    p: &ToneLinesSetStatusParams,
) -> Result<ToneLineWrite, RpcError> {
    let dir = package_dir(cfg, &p.character)?;
    let path = bank_path(cfg, &dir, &p.tone)?;
    let file = lines_file(&p.tone);
    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (current, lines) = read_bank(&path)?;
    let Some(old) = lines.iter().find(|l| l.id == p.id) else {
        return Err(RpcError::not_found(format!("line {} in {file}", p.id)));
    };
    let line = BankLine {
        status: p.status,
        ..old.clone()
    };
    if old.status != p.status {
        let text = current.unwrap_or_default();
        let next = bank::set_status(&text, &p.id, p.status).map_err(RpcError::internal)?;
        write_validated(&dir, &path, &file, &next)?;
    }
    Ok(ToneLineWrite {
        line: bank::wire(&line),
        file,
    })
}

// ── tone.directions.* ──────────────────────────────────────────────────

pub fn directions_get(cfg: &Config, character: &str) -> Result<ToneDirections, RpcError> {
    let dir = package_dir(cfg, character)?;
    let path = dir.join(DIRECTIONS_FILE);
    if path.exists() {
        contained(&cfg.paths.characters, &path)?;
    }
    Ok(ToneDirections {
        character: character.to_string(),
        file: DIRECTIONS_FILE.into(),
        text: read_text(&path)?.unwrap_or_default().replace("\r\n", "\n"),
    })
}

/// Replace `directions.md`: line ends as LF, one trailing newline.
pub fn directions_set(
    cfg: &Config,
    p: &ToneDirectionsSetParams,
) -> Result<ToneDirections, RpcError> {
    let dir = package_dir(cfg, &p.character)?;
    let path = dir.join(DIRECTIONS_FILE);
    if path.exists() {
        contained(&cfg.paths.characters, &path)?;
    }
    if p.text.chars().count() > MAX_DIRECTIONS_CHARS {
        return Err(RpcError::invalid(format!(
            "directions are longer than {MAX_DIRECTIONS_CHARS} characters"
        )));
    }
    let body = p.text.replace("\r\n", "\n");
    let text = format!("{}\n", body.trim_end());
    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    write_validated(&dir, &path, DIRECTIONS_FILE, &text)?;
    Ok(ToneDirections {
        character: p.character.clone(),
        file: DIRECTIONS_FILE.into(),
        text,
    })
}

// ── tone.variants ──────────────────────────────────────────────────────

fn clean_input(what: &str, s: Option<&str>) -> Result<Option<String>, RpcError> {
    let s = s.map(str::trim).filter(|s| !s.is_empty());
    if s.is_some_and(|s| s.chars().count() > MAX_INPUT_CHARS) {
        return Err(RpcError::invalid(format!(
            "{what} is longer than {MAX_INPUT_CHARS} characters"
        )));
    }
    Ok(s.map(String::from))
}

/// `tone.variants` against an explicit engine: one core-model call (`core_thread`'s route,
/// correlation `tone.variants:<character>`).
pub async fn variants_with(
    cfg: Arc<Config>,
    engine: &dyn Engine,
    p: ToneVariantsParams,
) -> Result<ToneVariants, RpcError> {
    if p.n == 0 || p.n > MAX_VARIANTS {
        return Err(RpcError::invalid(format!(
            "n must be 1 to {MAX_VARIANTS}, not {}",
            p.n
        )));
    }
    let line = clean_input("line", p.line.as_deref())?;
    let note = clean_input("note", p.note.as_deref())?;
    let (pkg, known, policy) = {
        let cfg = cfg.clone();
        let (character, tone) = (p.character.clone(), p.tone.clone());
        blocking(move || {
            let dir = package_dir(&cfg, &character)?;
            let path = bank_path(&cfg, &dir, &tone)?;
            let pkg = shadowrunr_brain::load_character(&dir, &tone)
                .map_err(|e| RpcError::io(format!("character {character}: {e}")))?;
            let (_, all) = read_bank(&path)?;
            let known: HashSet<String> = all
                .iter()
                .map(|l| bank::normalize(&pkg.persona.fill(&l.text)).to_lowercase())
                .collect();
            Ok((pkg, known, crate::clips::policy(&cfg)?))
        })
        .await?
    };
    let examples = variants::examples(&pkg.exemplars, p.situation);
    let (tier, effort, timeout_ms) = cfg.engine.route(Role::CoreThread);
    let reply = engine
        .complete(EngineRequest {
            role: Role::CoreThread,
            tier,
            effort,
            system: variants::system(&pkg),
            prompt: variants::prompt(
                p.situation,
                p.n,
                &examples,
                line.as_deref(),
                note.as_deref(),
            ),
            allowed_tools: Vec::new(),
            timeout_ms,
            json_schema: Some(variants::schema(p.n)),
            correlation_id: Some(format!("tone.variants:{}", p.character)),
        })
        .await
        .map_err(|e| RpcError::new(ErrorCode::Engine, format!("tone variants: {e}")))?;
    let mut known = known;
    if let Some(l) = &line {
        known.insert(bank::normalize(l).to_lowercase());
    }
    let (candidates, gated) =
        variants::keep(variants::parse(&reply.text), &known, &policy, p.n as usize);
    Ok(ToneVariants { candidates, gated })
}

// ── tone.inbox / tone.review_line ──────────────────────────────────────

fn session_events(
    cfg: &Config,
    session_id: &str,
) -> Result<Vec<shadowrunr_types::StreamEvent>, RpcError> {
    review::check_session_id(session_id)?;
    let dir = cfg.paths.sandbox_runs().join(session_id);
    if !dir.join(review::rows::TRANSCRIPT_FILE).is_file() {
        return Err(RpcError::not_found(format!("sandbox session {session_id}")));
    }
    review::rows::read_events(&dir)
}

fn folded(cfg: &Config) -> Result<Vec<shadowrunr_types::review::LineFeedback>, RpcError> {
    let path = review::store::feedback_path(&cfg.paths.review_dir());
    review::store::fold(&path)
        .map(|f| f.lines)
        .map_err(|e| io(&path, e))
}

pub fn inbox(cfg: &Config, character: &str) -> Result<ToneInbox, RpcError> {
    let dir = package_dir(cfg, character)?;
    let persona = character::persona_of(&cfg.paths.characters, character);
    // Lines already in a bank (any tone, any status) are not candidates any more.
    let mut banked: HashSet<String> = HashSet::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("lines")) {
        for e in rd.filter_map(|e| e.ok()) {
            if let Ok((_, lines)) = read_bank(&e.path()) {
                banked.extend(
                    lines
                        .iter()
                        .map(|l| bank::normalize(&persona.fill(&l.text)).to_lowercase()),
                );
            }
        }
    }
    let mut candidates = Vec::new();
    let mut suggestions = Vec::new();
    for (sid, rows) in inbox::by_session(folded(cfg)?) {
        let events = match session_events(cfg, &sid) {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("tone: inbox skips {sid}: {}", e.message);
                continue;
            }
        };
        if inbox::stream_of(&events).map(|(c, _)| c) != Some(character) {
            continue;
        }
        for f in &rows {
            let Some(item) = inbox::item(&sid, &events, f.line, Some(f), &persona) else {
                continue;
            };
            if f.tags.iter().any(|t| inbox::SUGGESTION_TAGS.contains(t)) {
                suggestions.push(item.clone());
            }
            if f.tags.iter().any(|t| inbox::CANDIDATE_TAGS.contains(t))
                && !banked.contains(&bank::normalize(&item.text).to_lowercase())
            {
                candidates.push(item);
            }
        }
    }
    Ok(ToneInbox {
        candidates: inbox::newest_first(candidates),
        suggestions: inbox::newest_first(suggestions),
    })
}

pub fn review_line(cfg: &Config, p: &ToneReviewLineParams) -> Result<ToneReviewItem, RpcError> {
    package_dir(cfg, &p.character)?;
    let events = session_events(cfg, &p.session_id)?;
    match inbox::stream_of(&events) {
        Some((c, _)) if c == p.character => {}
        _ => {
            return Err(RpcError::invalid(format!(
                "session {} is not a {} session",
                p.session_id, p.character
            )))
        }
    }
    let persona = character::persona_of(&cfg.paths.characters, &p.character);
    let feedback: Option<shadowrunr_types::review::LineFeedback> = folded(cfg)?
        .into_iter()
        .find(|f| f.session_id == p.session_id && f.line == p.line);
    inbox::item(&p.session_id, &events, p.line, feedback.as_ref(), &persona).ok_or_else(|| {
        RpcError::invalid(format!(
            "line {} of {} is not a host line",
            p.line, p.session_id
        ))
    })
}

// ── the RPCs ───────────────────────────────────────────────────────────

pub async fn rpc_variants(
    state: &Arc<AppState>,
    p: ToneVariantsParams,
) -> Result<ToneVariants, RpcError> {
    variants_with(state.cfg.clone(), state.engine.engine.as_ref(), p).await
}

pub async fn rpc_lines_list(
    state: &Arc<AppState>,
    p: ToneLinesParams,
) -> Result<ToneLines, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || lines_list(&cfg, &p)).await
}

pub async fn rpc_lines_add(
    state: &Arc<AppState>,
    p: ToneLinesAddParams,
) -> Result<ToneLineWrite, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || lines_add(&cfg, p)).await
}

pub async fn rpc_lines_set_status(
    state: &Arc<AppState>,
    p: ToneLinesSetStatusParams,
) -> Result<ToneLineWrite, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || lines_set_status(&cfg, &p)).await
}

pub async fn rpc_directions_get(
    state: &Arc<AppState>,
    p: ToneDirectionsGetParams,
) -> Result<ToneDirections, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || directions_get(&cfg, &p.character)).await
}

pub async fn rpc_directions_set(
    state: &Arc<AppState>,
    p: ToneDirectionsSetParams,
) -> Result<ToneDirections, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || directions_set(&cfg, &p)).await
}

pub async fn rpc_inbox(state: &Arc<AppState>, p: ToneInboxParams) -> Result<ToneInbox, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || inbox(&cfg, &p.character)).await
}

pub async fn rpc_review_line(
    state: &Arc<AppState>,
    p: ToneReviewLineParams,
) -> Result<ToneReviewItem, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || review_line(&cfg, &p)).await
}
