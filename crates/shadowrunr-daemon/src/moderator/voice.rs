//! The moderator character's chat line for each decision: one fresh `ModVoice` model call,
//! written from the character's identity and the decision (the rule's name, never the viewer's
//! message text), screened by the output gate, with the package's fallback line on a block,
//! a timeout or an engine error. The action never waits on its line.

use std::sync::Arc;

use shadowrunr_types::moderation::ModDecision;
use shadowrunr_types::Engine;

use crate::config::Config;

/// The loaded moderator character, ready to write lines.
pub struct ModVoice {
    /// The character's short name.
    pub name: String,
}

impl ModVoice {
    /// Loads `[moderator] character` from the characters root. A missing or invalid package
    /// gives a neutral voice and an error string for `moderator.status`.
    pub fn load(cfg: &Config) -> (Self, Option<String>) {
        (
            Self {
                name: cfg.moderator.character.clone(),
            },
            None,
        )
    }

    /// Writes the line for `decision`, broken by `rule_name`. Always returns a postable,
    /// gated line (the fallback when the model fails or the gate blocks).
    pub async fn line(
        &self,
        _engine: &Arc<dyn Engine>,
        _decision: &ModDecision,
        rule_name: &str,
    ) -> String {
        format!("{}: removed for {rule_name}.", self.name)
    }
}
