//! The Tone Lab: the owner directs a character's voice from the console's package page. The
//! daemon reads and writes two package files, `lines/<tone>.toml` (the line bank) and
//! `directions.md` (the do and don't list), inside `paths.characters` only, by text insertion
//! and status edits; the owner commits them. Review feedback feeds it (`tone.inbox`), and the
//! core model writes candidate lines (`tone.variants`) that become bank lines only when the
//! owner approves one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::review::FeedbackTag;
use crate::TsMs;

/// Where a bank line stands in the owner's review (`status` in `lines/<tone>.toml`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineStatus {
    /// Drafted, waiting for the owner's review; shown until retired.
    #[default]
    Seed,
    Approved,
    /// Never shown.
    Retired,
}

/// The situation tags a bank line may carry; each beat is classified into the same words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineSituation {
    RoastRequest,
    Bait,
    Newcomer,
    VoteLoss,
    Gift,
    AiQuestion,
    Quiet,
    ResearchWait,
    Proposal,
    Patron,
    Flood,
    General,
}

impl LineSituation {
    /// Every tag, in the contract's order.
    pub const ALL: [LineSituation; 12] = [
        LineSituation::RoastRequest,
        LineSituation::Bait,
        LineSituation::Newcomer,
        LineSituation::VoteLoss,
        LineSituation::Gift,
        LineSituation::AiQuestion,
        LineSituation::Quiet,
        LineSituation::ResearchWait,
        LineSituation::Proposal,
        LineSituation::Patron,
        LineSituation::Flood,
        LineSituation::General,
    ];

    /// The wire name, as serde writes it and the bank file stores it.
    pub fn as_str(self) -> &'static str {
        match self {
            LineSituation::RoastRequest => "roast_request",
            LineSituation::Bait => "bait",
            LineSituation::Newcomer => "newcomer",
            LineSituation::VoteLoss => "vote_loss",
            LineSituation::Gift => "gift",
            LineSituation::AiQuestion => "ai_question",
            LineSituation::Quiet => "quiet",
            LineSituation::ResearchWait => "research_wait",
            LineSituation::Proposal => "proposal",
            LineSituation::Patron => "patron",
            LineSituation::Flood => "flood",
            LineSituation::General => "general",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// One line of a bank as written: placeholders unfilled, every status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLine {
    pub id: String,
    pub status: LineStatus,
    pub tags: Vec<LineSituation>,
    pub text: String,
}

/// `tone.lines.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLinesParams {
    pub character: String,
    pub tone: String,
}

/// `tone.lines.list` result: a tone's whole bank in file order (empty without the file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLines {
    pub character: String,
    pub tone: String,
    /// The package-relative path, `lines/<tone>.toml`.
    pub file: String,
    pub lines: Vec<ToneLine>,
}

/// `tone.lines.add`: append a line to the bank, with the next id of the file's scheme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLinesAddParams {
    pub character: String,
    pub tone: String,
    pub text: String,
    /// One or more.
    pub tags: Vec<LineSituation>,
    /// `approved` when absent; `retired` is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<LineStatus>,
}

/// `tone.lines.set_status`: approve a seed, or retire a line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLinesSetStatusParams {
    pub character: String,
    pub tone: String,
    pub id: String,
    pub status: LineStatus,
}

/// What a bank write changed: the line as it now stands, and the file it went to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneLineWrite {
    pub line: ToneLine,
    /// The package-relative path written, `lines/<tone>.toml`.
    pub file: String,
}

/// `tone.directions.get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneDirectionsGetParams {
    pub character: String,
}

/// `tone.directions.set`: replace `directions.md` with `text`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneDirectionsSetParams {
    pub character: String,
    pub text: String,
}

/// `directions.md` as it stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneDirections {
    pub character: String,
    /// `directions.md`.
    pub file: String,
    /// Empty when the file does not exist.
    pub text: String,
}

/// `tone.variants`: the core model writes up to `n` candidate lines in `tone` for
/// `situation`. Nothing is written; a candidate becomes a bank line through `tone.lines.add`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneVariantsParams {
    pub character: String,
    pub tone: String,
    pub situation: LineSituation,
    /// A line to improve on (a Review line, a bank line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The owner's note on what to change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 1..=5.
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneVariants {
    /// At most `n`, deduplicated, none already in the bank.
    pub candidates: Vec<String>,
    /// Candidates the brain's output gate would have changed, dropped; omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gated: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `tone.inbox`: Review feedback on the character's sessions, mapped for the Tone Lab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneInboxParams {
    pub character: String,
}

/// `tone.review_line`: one host line of a Review session, for "Send to Tone Lab".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneReviewLineParams {
    pub character: String,
    pub session_id: String,
    pub line: u64,
}

/// One host line from Review, with what the owner said about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneReviewItem {
    pub session_id: String,
    /// The 1-based transcript line.
    pub line: u64,
    pub text: String,
    /// The session's tone (`stream_start.tone_variant`).
    pub tone: String,
    /// The most specific situation of the chat the line answers; absent when none is
    /// derivable or the beat carried no roast material (a viewer in care).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub situation: Option<LineSituation>,
    /// The owner's marks on the line (empty without feedback).
    pub tags: Vec<FeedbackTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When the feedback was given; absent without feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<TsMs>,
}

/// `tone.inbox` result, newest first in each list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneInbox {
    /// Lines marked `great_roast` or `landed`, not yet in their tone's bank: bank candidates.
    pub candidates: Vec<ToneReviewItem>,
    /// Lines marked `flat` or `too_mean`, with the owner's note: direction suggestions.
    pub suggestions: Vec<ToneReviewItem>,
}
