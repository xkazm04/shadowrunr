//! The moderator character's chat line for each decision: one fresh `ModVoice` model call,
//! written from the character's identity and the decision (the rule's name, never the viewer's
//! message text), screened by the output gate, with the package's fallback line on a block,
//! a timeout or an engine error. The action never waits on its line.
//!
//! The package is `<characters>/<[moderator] character>/` with `role = "moderator"`: its
//! `[identity]`, `constitution.md` and the fallback line in `moderator.toml` (`[lines]` in the
//! manifest has no key for it). See docs/guide/moderator.md.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use shadowrunr_brain::moderation::{gate_text, Policy};
use shadowrunr_brain::{
    is_moderator, load_character_with, LineBank, PersonaGate, PolicySource, RulesOverride,
};
use shadowrunr_types::character::Persona;
use shadowrunr_types::moderation::{ModActionKind, ModDecision, ModMode};
use shadowrunr_types::{Effort, Engine, EngineRequest, ModelTier, Role};

use crate::config::Config;

/// The package file that holds the moderator's model-free lines.
pub const LINES_FILE: &str = "moderator.toml";

/// The most characters a posted line may carry.
pub const MAX_LINE_CHARS: usize = 300;

/// The neutral voice's name, when the package is missing or invalid.
const NEUTRAL_NAME: &str = "Moderator";

/// The neutral voice's fallback (placeholders as in [`LINES_FILE`]).
const NEUTRAL_FALLBACK: &str = "{viewer}, your message was removed under the rule: {rule}.";

/// The last resort when even the filled fallback does not pass the gate (the viewer's name or
/// the rule's name tripped it): names nobody.
const LAST_RESORT: &str = "A message was removed under the channel rules.";

/// The neutral voice's constitution.
const NEUTRAL_CONSTITUTION: &str = "You are the chat moderator of a livestream, an AI. Write one \
plain line, under 200 characters, saying what was done and naming only the rule. Never repeat, \
quote or describe the removed message. Never threaten beyond the action taken. No emojis, no \
@-mentions except the actioned viewer's name.";

/// Slack over the route's timeout before the call counts as timed out here (the engine
/// enforces the route's own timeout; this guards a driver that does not).
const TIMEOUT_SLACK: Duration = Duration::from_secs(2);

/// `moderator.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinesToml {
    fallback: String,
}

/// The fallback template of the package at `dir` (`moderator.toml` `fallback`), checked: not
/// empty, one line, only the `{viewer}` and `{rule}` placeholders.
pub fn read_fallback(dir: &Path) -> Result<String, String> {
    let path = dir.join(LINES_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{LINES_FILE}: {e}"))?;
    let l: LinesToml =
        toml::from_str(&text).map_err(|e| format!("{LINES_FILE}: {}", e.message().trim()))?;
    let f = l.fallback.trim().to_string();
    if f.is_empty() {
        return Err(format!("{LINES_FILE}: fallback: must not be empty"));
    }
    if f.contains('\n') {
        return Err(format!("{LINES_FILE}: fallback: must be one line"));
    }
    let mut rest = f.as_str();
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(end) = after.find('}') {
            let key = &after[..end];
            if !key.is_empty()
                && key.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
                && key != "viewer"
                && key != "rule"
            {
                return Err(format!(
                    "{LINES_FILE}: fallback: unknown placeholder {{{key}}} (known: viewer, rule)"
                ));
            }
        }
        rest = after;
    }
    Ok(f)
}

/// The loaded moderator character, ready to write lines.
pub struct ModVoice {
    /// The character's short name.
    pub name: String,
    persona: Persona,
    constitution: String,
    /// The fallback template (`{viewer}`, `{rule}`).
    fallback: String,
    /// The output gate, with the moderator's persona.
    policy: Arc<Policy>,
    tier: ModelTier,
    effort: Effort,
    timeout_ms: u64,
}

impl ModVoice {
    /// Loads `[moderator] character` from the characters root. A missing or invalid package
    /// gives a neutral voice and an error string for `moderator.status`.
    pub fn load(cfg: &Config) -> (Self, Option<String>) {
        let (tier, effort, timeout_ms) = cfg.engine.route(Role::ModVoice);
        let id = &cfg.moderator.character;
        let policy_src = {
            let file = cfg.paths.policy.join("platform-rules.json");
            if file.is_file() {
                PolicySource::File(file)
            } else {
                PolicySource::Builtin
            }
        };
        let neutral = |error: String| {
            let persona = Persona::neutral(NEUTRAL_NAME);
            let gate = PersonaGate::new(&persona, &LineBank::resolve(None, &persona));
            let policy = Policy::load(&policy_src, &cfg.paths.characters, &[])
                .unwrap_or_else(|_| Policy::builtin())
                .with_persona(gate);
            (
                Self {
                    name: persona.name.clone(),
                    persona,
                    constitution: NEUTRAL_CONSTITUTION.into(),
                    fallback: NEUTRAL_FALLBACK.into(),
                    policy: Arc::new(policy),
                    tier,
                    effort,
                    timeout_ms,
                },
                Some(error),
            )
        };
        let safe = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !safe {
            return neutral(format!(
                "moderator character {id:?} is not a package id ([A-Za-z0-9_-])"
            ));
        }
        let dir = cfg.paths.characters.join(id);
        let pkg = match load_character_with(&dir, "", &RulesOverride::None) {
            Ok(p) => p,
            Err(e) => return neutral(format!("moderator character {id}: {e}")),
        };
        if !is_moderator(&pkg.manifest) {
            return neutral(format!(
                "moderator character {id}: character.toml must say role = \"moderator\""
            ));
        }
        let fallback = match read_fallback(&dir) {
            Ok(f) => f,
            Err(e) => return neutral(format!("moderator character {id}: {e}")),
        };
        let gate = PersonaGate::new(&pkg.persona, &pkg.lines);
        let (policy, error) = match Policy::load(&policy_src, &dir, &[]) {
            Ok(p) => (p, None),
            Err(e) => (
                Policy::builtin(),
                Some(format!("moderator output gate: {e}")),
            ),
        };
        (
            Self {
                name: pkg.persona.name.clone(),
                persona: pkg.persona,
                constitution: pkg.constitution,
                fallback,
                policy: Arc::new(policy.with_persona(gate)),
                tier,
                effort,
                timeout_ms,
            },
            error,
        )
    }

    /// Writes the line for `decision`, broken by `rule_name`. Always returns a postable,
    /// gated line (the fallback when the model fails or the gate blocks).
    pub async fn line(
        &self,
        engine: &Arc<dyn Engine>,
        decision: &ModDecision,
        rule_name: &str,
    ) -> String {
        let req = self.request(decision, rule_name);
        let wait = Duration::from_millis(self.timeout_ms) + TIMEOUT_SLACK;
        let reply = match tokio::time::timeout(wait, engine.complete(req)).await {
            Ok(Ok(r)) => r.text,
            Ok(Err(_)) | Err(_) => return self.fallback_line(decision, rule_name),
        };
        let Some(line) = self.clean(&reply) else {
            return self.fallback_line(decision, rule_name);
        };
        if !self.passes(&line, &decision.viewer_name) {
            return self.fallback_line(decision, rule_name);
        }
        line
    }

    /// The one `ModVoice` request for a decision: no tools, the route's tier and timeout. The
    /// prompt carries the action, the viewer's name, the rule's name and the mode; never the
    /// message (a decision has no text to carry).
    fn request(&self, d: &ModDecision, rule_name: &str) -> EngineRequest {
        EngineRequest {
            role: Role::ModVoice,
            tier: self.tier,
            effort: self.effort,
            system: self.system(),
            prompt: prompt(d, rule_name),
            allowed_tools: Vec::new(),
            timeout_ms: self.timeout_ms,
            json_schema: None,
            correlation_id: Some(format!("mod:{}", d.id)),
        }
    }

    /// The stable prefix: the constitution and the identity words.
    fn system(&self) -> String {
        let p = &self.persona;
        format!(
            "{constitution}\n\n## Identity\n\nYou are {full}. Chat calls you {name}. You serve the \
             {patron}. The viewers are {followers}. If asked what you are: \"{disclosure}\"\n\n\
             ## Output\n\nReply with the line only: one line, no speaker label, no quotes, under \
             200 characters.",
            constitution = self.constitution.trim(),
            full = p.full_name,
            name = p.name,
            patron = p.patron,
            followers = p.followers,
            disclosure = p.fill(&p.disclosure),
        )
    }

    /// The reply as a postable line: the first non-empty line, a leading speaker label and
    /// wrapping quotes stripped, at most [`MAX_LINE_CHARS`]. `None` when nothing is left.
    fn clean(&self, reply: &str) -> Option<String> {
        let first = reply.lines().map(str::trim).find(|l| !l.is_empty())?;
        let mut line = strip_label(first, &self.persona).trim().to_string();
        for (open, close) in [('"', '"'), ('\u{201c}', '\u{201d}'), ('\'', '\'')] {
            if line.chars().count() >= 2 && line.starts_with(open) && line.ends_with(close) {
                line = line[open.len_utf8()..line.len() - close.len_utf8()]
                    .trim()
                    .to_string();
            }
        }
        let line = cap(&line);
        (!line.is_empty()).then_some(line)
    }

    /// Whether `line` may be posted as written: the output gate changes nothing (a warning
    /// alone passes), and the constitution's form holds (no emoji, no @-mention but the
    /// viewer's).
    fn passes(&self, line: &str, viewer: &str) -> bool {
        let (out, fired) = gate_text(line, &self.policy, &[]);
        if out.trim().is_empty() || fired.iter().any(|(_, action)| action != "warned") {
            return false;
        }
        !has_emoji(line) && mentions_only(line, viewer)
    }

    /// The package's fallback, names filled; when that does not pass the gate, the fallback
    /// with the follower word for the viewer; then a line naming nobody.
    fn fallback_line(&self, d: &ModDecision, rule_name: &str) -> String {
        let fill = |viewer: &str| {
            cap(&self
                .fallback
                .replace("{viewer}", viewer)
                .replace("{rule}", rule_name))
        };
        let named = fill(&d.viewer_name);
        if self.passes(&named, &d.viewer_name) {
            return named;
        }
        let unnamed = fill(&self.persona.follower);
        if self.passes(&unnamed, "") {
            return unnamed;
        }
        LAST_RESORT.to_string()
    }
}

/// The per-decision ask.
fn prompt(d: &ModDecision, rule_name: &str) -> String {
    let action = match d.action {
        ModActionKind::Delete => "their message was deleted".to_string(),
        ModActionKind::Timeout => {
            let minutes = d.timeout_s.unwrap_or(600).div_ceil(60).max(1);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("their message was deleted and they are timed out for {minutes} {unit}")
        }
        ModActionKind::Ban => "their message was deleted and they are banned".to_string(),
    };
    let mode = match d.mode {
        ModMode::Armed => "armed: the line is posted in chat",
        ModMode::Shadow => "shadow: the line is logged for the operator, not posted",
    };
    format!(
        "A moderation decision to announce.\n\
         Viewer: {viewer}\n\
         Action: {action}\n\
         Rule: {rule_name}\n\
         Mode: {mode}\n\n\
         Write the one line you post in chat about it, addressed to {viewer}. Name only the rule. \
         You have not seen the message and must not guess at it.",
        viewer = d.viewer_name,
    )
}

/// `text` without a leading speaker label: the persona's own labels, then any `Name:` /
/// `**Name:**` / `**Name**:` form of the short or full name, in any case.
fn strip_label<'a>(text: &'a str, p: &Persona) -> &'a str {
    for pre in &p.reply_prefixes {
        if let Some(rest) = text.strip_prefix(pre.as_str()) {
            return rest;
        }
    }
    let lower = text.to_lowercase();
    for name in [&p.full_name, &p.name] {
        let n = name.to_lowercase();
        for form in [
            format!("**{n}:**"),
            format!("**{n}**:"),
            format!("{n}:"),
            format!("[{n}]"),
        ] {
            if lower.starts_with(&form) && text.is_char_boundary(form.len()) {
                return &text[form.len()..];
            }
        }
    }
    text
}

/// At most [`MAX_LINE_CHARS`] characters, cut on a char boundary.
fn cap(s: &str) -> String {
    s.chars()
        .take(MAX_LINE_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn has_emoji(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x1F000..=0x1FAFF).contains(&u) || (0x2600..=0x27BF).contains(&u) || u == 0xFE0F
    })
}

/// Every `@name` in `s` names `viewer` (case-insensitive).
fn mentions_only(s: &str, viewer: &str) -> bool {
    let viewer = viewer.trim_start_matches('@').to_lowercase();
    s.split('@').skip(1).all(|after| {
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        name.is_empty() || (!viewer.is_empty() && name.to_lowercase() == viewer)
    })
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;
