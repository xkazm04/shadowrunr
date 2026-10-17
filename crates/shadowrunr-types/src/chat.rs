//! Viewers (minions), ranks and chat messages.

use serde::{Deserialize, Serialize};

use crate::TsMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Twitch,
    Youtube,
    Kick,
    Sim,
}

/// The weighted rank ladder, lowest first. `Thrall` is a non-subscriber who votes with
/// channel points at weight 1. Rank derivation and vote weights live in shadowrunr-segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rank {
    Thrall,
    Acolyte,
    Cultist,
    Warlock,
    Lich,
    DreadCouncil,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewer {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub rank: Rank,
    /// 1, 2 or 3; omitted for non-subscribers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_tier: Option<u8>,
    #[serde(default)]
    pub months: u32,
    #[serde(default)]
    pub gifted: u32,
    #[serde(default)]
    pub is_mod: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EvidenceGrade {
    /// Documented by a primary source (court, regulator, registry).
    Doc,
    /// Reported by press.
    Rep,
    /// Community claim.
    Clm,
    /// Malakor's speculation.
    Spc,
}

/// Parsed chat commands. Wire form in chat: `!summon <foil>`, `!hint <hex> <text>`,
/// `!grade <hex> <DOC|REP|CLM|SPC>`, `!propose <text>`, `!vote <option>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Summon { foil: String },
    Hint { hex: String, text: String },
    Grade { hex: String, grade: EvidenceGrade },
    Propose { text: String },
    Vote { option: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMsg {
    pub id: String,
    pub ts_ms: TsMs,
    pub viewer: Viewer,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
}
