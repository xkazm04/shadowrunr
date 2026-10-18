//! Segments as data. Each segment is a TOML table under `data/segments/` (compiled in):
//! states, a transition table, allowed ops, allowed chat commands, rules prompt text and clock
//! rules. One generic interpreter ([`SegmentTable::next_state`]) runs every segment; there are
//! no per-segment match arms.
//!
//! # Descoped segments
//!
//! SEG-02B-I and SEG-02B-II are defined with status `descoped`, and
//! [`crate::SegmentRuntime::with_config`] refuses to start them. Their design notes are kept
//! here (and in each table's `design_notes`) so they can be revisited:
//!
//! - **SEG-02B-I Cartography Legion.** Viewers map real HOT Tasking Manager tiles in their own
//!   OSM accounts (`!enlist <osm_handle>`), hashtag `#OverlordCartographyLegion`, following the
//!   OSM Organised Editing Guidelines. Points only for VALIDATED changesets
//!   (`buildings_added_count * 10`), polled from `/api/0.6/changesets` every 30 s. The agent
//!   never edits OSM. Table `osm_legion_contributions`.
//! - **SEG-02B-II Dependency Doctrine.** Claude Code builds PhylacteryGuard (an anti-scam
//!   detector fed by SEG-02A patterns) and LichReader on stream. Chat polls pick features,
//!   `!issue` files cases, credits go to CHANGELOG.md. Pushes only to `staging/ep-XX`; the owner
//!   signs releases with a hardware key. osv-scanner, `--ignore-scripts`, a PreToolUse npm
//!   block. Table `dependency_doctrine_releases` (FK `linked_fraud_case_id`).
//!
//! Reviving either needs its command (`!enlist` / `!issue`, a contract change), its worker,
//! and the cross-references stripped when descoping (digest section 5).

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::Deserialize;
use shadowrunr_types::{Op, SegmentId, SegmentStatus};

use crate::SegmentError;

/// Compiled-in table sources, one per [`SegmentId`].
pub const SEGMENT_TABLE_SOURCES: [(&str, &str); 4] = [
    ("seg-01.toml", include_str!("../data/segments/seg-01.toml")),
    (
        "seg-02a.toml",
        include_str!("../data/segments/seg-02a.toml"),
    ),
    (
        "seg-02b-i.toml",
        include_str!("../data/segments/seg-02b-i.toml"),
    ),
    (
        "seg-02b-ii.toml",
        include_str!("../data/segments/seg-02b-ii.toml"),
    ),
];

/// Every `Op` tag (serde name) in the action grammar.
pub const OP_TAGS: [&str; 11] = [
    "highlight",
    "start_poll",
    "close_poll",
    "summon_foil",
    "dismiss_foil",
    "reveal_hex",
    "dispatch_research",
    "show_card",
    "mark_moment",
    "switch_segment",
    "sign_off",
];

/// Chat command names a segment may accept.
pub const COMMAND_NAMES: [&str; 5] = ["summon", "hint", "grade", "propose", "vote"];

/// Wire name of a segment (`SEG-01`, ...), as serde writes it.
pub fn segment_name(id: SegmentId) -> &'static str {
    match id {
        SegmentId::Seg01ImprovisedRoom => "SEG-01",
        SegmentId::Seg02aHexmap => "SEG-02A",
        SegmentId::Seg02bCartography => "SEG-02B-I",
        SegmentId::Seg02bDependency => "SEG-02B-II",
    }
}

/// Serde tag of an op, as listed in `allowed_ops`.
pub fn op_tag(op: &Op) -> &'static str {
    match op {
        Op::Highlight { .. } => "highlight",
        Op::StartPoll { .. } => "start_poll",
        Op::ClosePoll { .. } => "close_poll",
        Op::SummonFoil { .. } => "summon_foil",
        Op::DismissFoil => "dismiss_foil",
        Op::RevealHex { .. } => "reveal_hex",
        Op::DispatchResearch { .. } => "dispatch_research",
        Op::ShowCard { .. } => "show_card",
        Op::MarkMoment { .. } => "mark_moment",
        Op::SwitchSegment { .. } => "switch_segment",
        Op::SignOff { .. } => "sign_off",
    }
}

/// Events the runtime feeds into a segment's transition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    FoilVoteOpened,
    FoilVoteClosed,
    FoilSummoned,
    FoilDismissed,
    PollOpened,
    PollClosed,
    HexRevealed,
    HandoverDue,
}

impl Trigger {
    /// Starting triggers are refused in a state with no matching row. The rest (closings and
    /// the clock) always happen and simply leave the state unchanged when no row matches.
    pub fn is_starting(self) -> bool {
        matches!(
            self,
            Trigger::FoilVoteOpened
                | Trigger::FoilSummoned
                | Trigger::FoilDismissed
                | Trigger::PollOpened
                | Trigger::HexRevealed
        )
    }
}

/// Named guard conditions the interpreter evaluates against [`GuardFacts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Guard {
    FoilActive,
    NoFoil,
    AnyRevealed,
    NoneRevealed,
}

/// Runtime facts the guards read.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuardFacts {
    pub foil_active: bool,
    pub any_revealed: bool,
}

impl Guard {
    fn holds(self, f: GuardFacts) -> bool {
        match self {
            Guard::FoilActive => f.foil_active,
            Guard::NoFoil => !f.foil_active,
            Guard::AnyRevealed => f.any_revealed,
            Guard::NoneRevealed => !f.any_revealed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// A state name, or `*` for any state.
    pub from: String,
    pub on: Trigger,
    #[serde(default)]
    pub guard: Option<Guard>,
    pub to: String,
}

/// SEG-01's handover clock rule. All values are assumptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoverRule {
    pub to: SegmentId,
    /// Concurrent viewers needed...
    pub ccv_min: u32,
    /// ...held without a dip for this long.
    pub sustain_s: u32,
    /// Or hand over once the segment has run this long.
    pub max_elapsed_s: u32,
    /// Seconds the brain has to emit `switch_segment` before the clock switches itself.
    pub grace_s: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockRules {
    /// Length of a `!summon` foil vote.
    #[serde(default = "default_foil_vote_secs")]
    pub foil_vote_secs: u32,
    #[serde(default = "default_poll_min_secs")]
    pub poll_min_secs: u32,
    #[serde(default = "default_poll_max_secs")]
    pub poll_max_secs: u32,
    #[serde(default = "default_max_poll_options")]
    pub max_poll_options: usize,
    #[serde(default)]
    pub handover: Option<HandoverRule>,
}

fn default_foil_vote_secs() -> u32 {
    60
}
fn default_poll_min_secs() -> u32 {
    10
}
fn default_poll_max_secs() -> u32 {
    600
}
fn default_max_poll_options() -> usize {
    6
}

impl Default for ClockRules {
    fn default() -> Self {
        Self {
            foil_vote_secs: default_foil_vote_secs(),
            poll_min_secs: default_poll_min_secs(),
            poll_max_secs: default_poll_max_secs(),
            max_poll_options: default_max_poll_options(),
            handover: None,
        }
    }
}

/// One segment, as data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentTable {
    pub id: SegmentId,
    pub title: String,
    pub status: SegmentStatus,
    pub initial: String,
    pub states: Vec<String>,
    pub allowed_ops: Vec<String>,
    pub commands: Vec<String>,
    /// Segments `switch_segment` may move to.
    pub switch_to: Vec<SegmentId>,
    pub rules_prompt: String,
    #[serde(default)]
    pub design_notes: Option<String>,
    #[serde(default)]
    pub clock: ClockRules,
    #[serde(default)]
    pub transitions: Vec<Transition>,
}

impl SegmentTable {
    pub fn from_toml_str(file: &str, text: &str) -> Result<Self, SegmentError> {
        let table: SegmentTable = toml::from_str(text).map_err(|e| SegmentError::Data {
            file: file.into(),
            detail: e.to_string(),
        })?;
        table.validate(file)?;
        Ok(table)
    }

    fn validate(&self, file: &str) -> Result<(), SegmentError> {
        let bad = |detail: String| SegmentError::Data {
            file: file.into(),
            detail,
        };
        let states: BTreeSet<&str> = self.states.iter().map(String::as_str).collect();
        if states.len() != self.states.len() {
            return Err(bad("duplicate state names".into()));
        }
        if !states.contains(self.initial.as_str()) {
            return Err(bad(format!(
                "initial state {:?} is not a state",
                self.initial
            )));
        }
        for t in &self.transitions {
            if t.from != "*" && !states.contains(t.from.as_str()) {
                return Err(bad(format!("transition from unknown state {:?}", t.from)));
            }
            if !states.contains(t.to.as_str()) {
                return Err(bad(format!("transition to unknown state {:?}", t.to)));
            }
        }
        if let Some(op) = self
            .allowed_ops
            .iter()
            .find(|o| !OP_TAGS.contains(&o.as_str()))
        {
            return Err(bad(format!("unknown op {op:?}")));
        }
        if let Some(c) = self
            .commands
            .iter()
            .find(|c| !COMMAND_NAMES.contains(&c.as_str()))
        {
            return Err(bad(format!("unknown command {c:?}")));
        }
        let c = &self.clock;
        if c.poll_min_secs == 0 || c.poll_min_secs > c.poll_max_secs || c.max_poll_options < 2 {
            return Err(bad("clock: poll bounds are inconsistent".into()));
        }
        if self.status == SegmentStatus::Descoped
            && (!self.allowed_ops.is_empty() || self.design_notes.is_none())
        {
            return Err(bad(
                "a descoped segment allows no ops and keeps its design notes".into(),
            ));
        }
        Ok(())
    }

    pub fn allows_op(&self, tag: &str) -> bool {
        self.allowed_ops.iter().any(|o| o == tag)
    }

    pub fn allows_command(&self, name: &str) -> bool {
        self.commands.iter().any(|c| c == name)
    }

    pub fn is_descoped(&self) -> bool {
        self.status == SegmentStatus::Descoped
    }

    /// The generic interpreter: first row matching (state, trigger, guard) wins.
    pub fn next_state(&self, state: &str, on: Trigger, facts: GuardFacts) -> Option<&str> {
        self.transitions
            .iter()
            .find(|t| {
                (t.from == "*" || t.from == state)
                    && t.on == on
                    && t.guard.is_none_or(|g| g.holds(facts))
            })
            .map(|t| t.to.as_str())
    }

    /// Whether any row exists for (state, trigger), ignoring guards.
    pub fn accepts(&self, state: &str, on: Trigger) -> bool {
        self.transitions
            .iter()
            .any(|t| (t.from == "*" || t.from == state) && t.on == on)
    }
}

/// All compiled-in segment tables, parsed once.
pub fn tables() -> Result<&'static [SegmentTable], SegmentError> {
    static TABLES: OnceLock<Result<Vec<SegmentTable>, SegmentError>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            SEGMENT_TABLE_SOURCES
                .iter()
                .map(|(file, text)| SegmentTable::from_toml_str(file, text))
                .collect()
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// The compiled-in table for one segment.
pub fn segment_table(id: SegmentId) -> Result<&'static SegmentTable, SegmentError> {
    tables()?
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| SegmentError::Data {
            file: "data/segments".into(),
            detail: format!("no table for {id:?}"),
        })
}

/// Whether `id` is descoped, read from its compiled-in table (`status = "descoped"`). This is
/// the one place the descoped set lives: callers ask here instead of naming SEG-02B by hand. A
/// segment without a table, or tables that fail to parse, counts as descoped (never startable).
pub fn segment_is_descoped(id: SegmentId) -> bool {
    segment_table(id).map_or(true, SegmentTable::is_descoped)
}

/// The segments that can start (not descoped), in table order. Empty when the tables fail to
/// parse.
pub fn active_segments() -> Vec<SegmentId> {
    tables()
        .map(|ts| {
            ts.iter()
                .filter(|t| !t.is_descoped())
                .map(|t| t.id)
                .collect()
        })
        .unwrap_or_default()
}

/// The descoped segments, in table order.
pub fn descoped_segments() -> Vec<SegmentId> {
    tables()
        .map(|ts| {
            ts.iter()
                .filter(|t| t.is_descoped())
                .map(|t| t.id)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descoped_set_comes_from_the_tables() {
        assert_eq!(
            active_segments(),
            vec![SegmentId::Seg01ImprovisedRoom, SegmentId::Seg02aHexmap]
        );
        assert_eq!(
            descoped_segments(),
            vec![SegmentId::Seg02bCartography, SegmentId::Seg02bDependency]
        );
        for id in descoped_segments() {
            assert!(segment_is_descoped(id));
        }
        for id in active_segments() {
            assert!(!segment_is_descoped(id));
        }
    }

    #[test]
    fn all_tables_parse_and_cover_every_segment() {
        let ts = tables().unwrap();
        for id in [
            SegmentId::Seg01ImprovisedRoom,
            SegmentId::Seg02aHexmap,
            SegmentId::Seg02bCartography,
            SegmentId::Seg02bDependency,
        ] {
            assert!(ts.iter().any(|t| t.id == id), "{id:?}");
        }
    }

    #[test]
    fn seg01_matches_brief() {
        let t = segment_table(SegmentId::Seg01ImprovisedRoom).unwrap();
        assert_eq!(
            t.states,
            [
                "opening",
                "foil_vote",
                "foil_live",
                "crowd_game",
                "handover_pending"
            ]
        );
        assert_eq!(
            t.allowed_ops,
            [
                "highlight",
                "start_poll",
                "close_poll",
                "summon_foil",
                "dismiss_foil",
                "show_card",
                "mark_moment",
                "switch_segment",
                "sign_off"
            ]
        );
        let h = t.clock.handover.unwrap();
        assert_eq!((h.ccv_min, h.sustain_s, h.max_elapsed_s), (15, 180, 1500));
        assert_eq!(h.to, SegmentId::Seg02aHexmap);
    }

    #[test]
    fn seg02a_ops_are_seg01_plus_investigation_minus_foil() {
        let one = segment_table(SegmentId::Seg01ImprovisedRoom).unwrap();
        let two = segment_table(SegmentId::Seg02aHexmap).unwrap();
        let mut want: BTreeSet<&str> = one.allowed_ops.iter().map(String::as_str).collect();
        want.remove("summon_foil");
        want.remove("dismiss_foil");
        want.insert("reveal_hex");
        want.insert("dispatch_research");
        let got: BTreeSet<&str> = two.allowed_ops.iter().map(String::as_str).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn seg02a_rules_state_guardrails() {
        let p = &segment_table(SegmentId::Seg02aHexmap)
            .unwrap()
            .rules_prompt
            .to_lowercase();
        for needle in [
            "public-record subjects only",
            "alleged",
            "no crowd action",
            "publish gate",
        ] {
            assert!(p.contains(needle), "{needle}");
        }
    }

    #[test]
    fn descoped_tables_keep_notes() {
        for id in [SegmentId::Seg02bCartography, SegmentId::Seg02bDependency] {
            let t = segment_table(id).unwrap();
            assert!(t.is_descoped());
            assert!(t.design_notes.as_deref().is_some_and(|n| n.len() > 100));
        }
    }

    #[test]
    fn interpreter_uses_guards() {
        let t = segment_table(SegmentId::Seg01ImprovisedRoom).unwrap();
        let foil = GuardFacts {
            foil_active: true,
            ..Default::default()
        };
        let none = GuardFacts::default();
        assert_eq!(
            t.next_state("crowd_game", Trigger::PollClosed, foil),
            Some("foil_live")
        );
        assert_eq!(
            t.next_state("crowd_game", Trigger::PollClosed, none),
            Some("opening")
        );
        assert_eq!(
            t.next_state("foil_live", Trigger::HandoverDue, none),
            Some("handover_pending")
        );
        assert!(!t.accepts("handover_pending", Trigger::PollOpened));
        assert!(!t.accepts("foil_vote", Trigger::FoilSummoned));
    }

    #[test]
    fn invalid_tables_rejected() {
        let src = SEGMENT_TABLE_SOURCES[0].1;
        let bad_state = src.replace("to = \"foil_vote\"", "to = \"nowhere\"");
        assert!(SegmentTable::from_toml_str("x", &bad_state).is_err());
        let bad_op = src.replace("\"mark_moment\",", "\"launch_missiles\",");
        assert!(SegmentTable::from_toml_str("x", &bad_op).is_err());
        let bad_trigger = src.replace("on = \"handover_due\"", "on = \"whenever\"");
        assert!(SegmentTable::from_toml_str("x", &bad_trigger).is_err());
    }
}
