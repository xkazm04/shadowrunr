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

use std::collections::{BTreeMap, BTreeSet};
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
pub const OP_TAGS: [&str; 12] = [
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
    "show_screen",
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
        Op::ShowScreen { .. } => "show_screen",
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

/// The state a `[clock.cold_open]` rule clocks.
pub const COLD_OPEN_STATE: &str = "cold_open";

/// SEG-01's cold open: the room decides when it ends (distinct chatters held over a sustain
/// window), the clock decides how long at most. The rule runs while the segment is in the
/// [`COLD_OPEN_STATE`] state and moves it to `to` when it ends (a clock rule, like the
/// handover's, rather than a transition row). All values are assumptions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColdOpenRule {
    /// The state the segment enters when the cold open ends.
    pub to: String,
    /// Floor: the cold open lasts at least this long.
    pub min_s: u32,
    /// Ceiling: it ends at this point whatever the room does. A config override of 0 skips
    /// the cold open (the segment starts in the state after it).
    pub max_s: u32,
    /// Distinct chatters who wrote in the last `window_s` seconds...
    pub active_chatters: u32,
    #[serde(default = "default_active_window_s")]
    pub window_s: u32,
    /// ...held without a dip for this long.
    pub sustain_s: u32,
}

impl ColdOpenRule {
    /// The rule fits `table`: it has the cold-open state and the `to` state, and the numbers
    /// are consistent (a ceiling of 0, the skip, passes).
    pub fn check(&self, table: &SegmentTable) -> Result<(), String> {
        let has = |s: &str| table.states.iter().any(|x| x == s);
        if !has(COLD_OPEN_STATE) || !has(&self.to) || self.to == COLD_OPEN_STATE {
            return Err(format!(
                "needs a {COLD_OPEN_STATE:?} state and another state to move to, not {:?}",
                self.to
            ));
        }
        if self.max_s > 0
            && (self.min_s > self.max_s || self.active_chatters == 0 || self.window_s == 0)
        {
            return Err(
                "a ceiling at or above the floor, and a positive chatter count and window".into(),
            );
        }
        Ok(())
    }
}

/// The rolling window distinct active chatters are counted over.
fn default_active_window_s() -> u32 {
    60
}

/// Per-state rules: what a state narrows and what it tells the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRules {
    /// Ops allowed in this state, a subset of the segment's `allowed_ops`. Absent: all of them.
    #[serde(default)]
    pub allowed_ops: Option<Vec<String>>,
    /// A short rules line the state summary carries while the segment is in this state.
    #[serde(default)]
    pub prompt: Option<String>,
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
    /// Research briefs the bone-clerks run against one hex per stream. The next attempt is
    /// refused and the hex's next move goes to a Plebiscite instead (SEG-02A).
    #[serde(default = "default_max_searches_per_hex")]
    pub max_searches_per_hex: u32,
    /// Length of the Plebiscite the runtime opens when a hex's searches run out.
    #[serde(default = "default_plebiscite_secs")]
    pub plebiscite_secs: u32,
    #[serde(default)]
    pub handover: Option<HandoverRule>,
    #[serde(default)]
    pub cold_open: Option<ColdOpenRule>,
}

/// Searches per hex per stream before the Plebiscite takes over: twice ends the "fog stays
/// fogged" loop without starving a hex that needed a second look.
pub const DEFAULT_MAX_SEARCHES_PER_HEX: u32 = 2;

fn default_foil_vote_secs() -> u32 {
    60
}
fn default_max_searches_per_hex() -> u32 {
    DEFAULT_MAX_SEARCHES_PER_HEX
}
fn default_plebiscite_secs() -> u32 {
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
            max_searches_per_hex: default_max_searches_per_hex(),
            plebiscite_secs: default_plebiscite_secs(),
            handover: None,
            cold_open: None,
        }
    }
}

/// SEG-02A's arc for a long stream: the case unfolds in acts (digest section 4: anomaly and
/// filing map, shell and archive trail, dossier sealed) instead of being revealed in the first
/// half hour. Every act gets an equal share of the planned segment time; a hex staged for a
/// later act stays locked (no reveal, no research) until its act opens, and `sign_off` waits
/// for the seal window at the end of the last act. Values are assumptions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcRules {
    /// The arc runs only when the segment is planned for at least this many seconds.
    pub min_planned_s: u32,
    /// Act titles, in order.
    pub acts: Vec<String>,
    /// The seal window: the last this-many seconds of the planned time, when the dossier is
    /// recapped and sealed and `sign_off` is allowed.
    pub seal_s: u32,
    /// Reveal pacing inside each act: the act's reveal span (the act, or the last act up to
    /// the seal window) is cut into one slot per hex staged for it, and a new reveal waits for
    /// the next slot (`reveal_not_due`). Off when absent: the acts gate which hexes only.
    #[serde(default)]
    pub reveal_pacing: bool,
    /// The least time between two reveals, as a percentage of the running act's slot, so a
    /// host who idled cannot spend the slots it saved in one burst. 0 disables it.
    #[serde(default)]
    pub reveal_gap_pct: u32,
    /// A hex is revealed only after a beat of work on it: a bone-clerk search, or a closed
    /// crowd vote (a poll, or an "Annex" Plebiscite) that chose it (`reveal_needs_work`).
    #[serde(default)]
    pub reveal_needs_work: bool,
    /// Direction only (the runtime cannot hear a recap): at most one act recap per this many
    /// seconds; the turn context says so. 0 leaves it out.
    #[serde(default)]
    pub recap_cooldown_s: u32,
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
    /// Rules per state name (`[state_rules.<state>]`): the ops it allows and its prompt line.
    #[serde(default)]
    pub state_rules: BTreeMap<String, StateRules>,
    /// The multi-act arc of a long stream (SEG-02A).
    #[serde(default)]
    pub arc: Option<ArcRules>,
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
        if c.max_searches_per_hex == 0
            || !(c.poll_min_secs..=c.poll_max_secs).contains(&c.plebiscite_secs)
        {
            return Err(bad(
                "clock: the search cap must be positive and the plebiscite within poll bounds"
                    .into(),
            ));
        }
        for (state, rules) in &self.state_rules {
            if !states.contains(state.as_str()) {
                return Err(bad(format!("state_rules for unknown state {state:?}")));
            }
            if let Some(op) = rules
                .allowed_ops
                .iter()
                .flatten()
                .find(|o| !self.allows_op(o))
            {
                return Err(bad(format!(
                    "state_rules.{state}: op {op:?} is not in the segment's allowed_ops"
                )));
            }
        }
        if let Some(r) = &c.cold_open {
            if r.max_s == 0 {
                return Err(bad("clock.cold_open: the ceiling must be positive".into()));
            }
            r.check(self)
                .map_err(|e| bad(format!("clock.cold_open: {e}")))?;
        }
        if let Some(arc) = &self.arc {
            let acts = u32::try_from(arc.acts.len()).unwrap_or(u32::MAX);
            if acts < 2
                || arc.acts.iter().any(|a| a.trim().is_empty())
                || arc.min_planned_s == 0
                || arc.seal_s == 0
                || arc.seal_s >= arc.min_planned_s / acts
            {
                return Err(bad(
                    "arc: two or more named acts, and a seal window shorter than one act".into(),
                ));
            }
            if arc.reveal_gap_pct > 100 {
                return Err(bad("arc: reveal_gap_pct is a percentage of one slot".into()));
            }
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

    /// Whether `tag` is allowed in `state`: the segment allows it and the state's rules, if
    /// they list ops, list it too.
    pub fn allows_op_in(&self, state: &str, tag: &str) -> bool {
        self.allows_op(tag)
            && self
                .state_rules
                .get(state)
                .and_then(|r| r.allowed_ops.as_ref())
                .is_none_or(|ops| ops.iter().any(|o| o == tag))
    }

    /// The rules line of `state`, if the table gives it one.
    pub fn state_prompt(&self, state: &str) -> Option<&str> {
        self.state_rules
            .get(state)
            .and_then(|r| r.prompt.as_deref())
            .map(str::trim)
            .filter(|p| !p.is_empty())
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
                "cold_open",
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
    fn seg01_opens_cold_and_the_table_holds_polls_and_foils_back() {
        let t = segment_table(SegmentId::Seg01ImprovisedRoom).unwrap();
        assert_eq!(t.initial, COLD_OPEN_STATE);
        let c = t.clock.cold_open.as_ref().unwrap();
        assert_eq!(
            (c.min_s, c.max_s, c.active_chatters, c.window_s, c.sustain_s),
            (60, 180, 5, 60, 20)
        );
        assert_eq!(c.to, "opening");
        // The cold open leaves by its clock only: no row starts anything in it.
        assert!(!t.transitions.iter().any(|r| r.from == COLD_OPEN_STATE));
        for op in ["start_poll", "summon_foil", "switch_segment"] {
            assert!(t.allows_op(op));
            assert!(!t.allows_op_in(COLD_OPEN_STATE, op), "{op}");
            assert!(t.allows_op_in("opening", op), "{op}");
        }
        assert!(t.allows_op_in(COLD_OPEN_STATE, "highlight"));
        assert!(!t.accepts(COLD_OPEN_STATE, Trigger::PollOpened));
        assert!(!t.accepts(COLD_OPEN_STATE, Trigger::FoilVoteOpened));
        assert!(t
            .state_prompt(COLD_OPEN_STATE)
            .is_some_and(|p| p.starts_with("The room is still filling")));
        assert_eq!(t.state_prompt("opening"), None);
    }

    #[test]
    fn invalid_cold_open_and_state_rules_rejected() {
        let src = SEGMENT_TABLE_SOURCES[0].1;
        let inverted = src.replace("min_s = 60", "min_s = 600");
        assert!(SegmentTable::from_toml_str("x", &inverted).is_err());
        let nowhere = src.replace("to = \"opening\"\nmin_s", "to = \"nowhere\"\nmin_s");
        assert_ne!(nowhere, src);
        assert!(SegmentTable::from_toml_str("x", &nowhere).is_err());
        let to_itself = src.replace("to = \"opening\"\nmin_s", "to = \"cold_open\"\nmin_s");
        assert!(SegmentTable::from_toml_str("x", &to_itself).is_err());
        let no_ceiling = src.replace("max_s = 180", "max_s = 0");
        assert!(SegmentTable::from_toml_str("x", &no_ceiling).is_err());
        let foreign_op = src.replace(
            "allowed_ops = [\"highlight\", \"show_card\"",
            "allowed_ops = [\"reveal_hex\", \"show_card\"",
        );
        assert_ne!(foreign_op, src);
        assert!(SegmentTable::from_toml_str("x", &foreign_op).is_err());
        let unknown_state = src.replace("[state_rules.cold_open]", "[state_rules.nowhere]");
        assert!(SegmentTable::from_toml_str("x", &unknown_state).is_err());
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
    fn seg02a_caps_searches_per_hex() {
        let c = &segment_table(SegmentId::Seg02aHexmap).unwrap().clock;
        assert_eq!(c.max_searches_per_hex, DEFAULT_MAX_SEARCHES_PER_HEX);
        assert_eq!(c.max_searches_per_hex, 2);
        assert!((c.poll_min_secs..=c.poll_max_secs).contains(&c.plebiscite_secs));
        let src = SEGMENT_TABLE_SOURCES[1].1;
        let zero = src.replace("max_searches_per_hex = 2", "max_searches_per_hex = 0");
        assert!(SegmentTable::from_toml_str("x", &zero).is_err());
        let long = src.replace("plebiscite_secs = 60", "plebiscite_secs = 6000");
        assert!(SegmentTable::from_toml_str("x", &long).is_err());
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
