//! `SegmentRuntime`: drives the active segment's table from chat, the brain's ops and the
//! clock. Deterministic given its inputs: time only ever comes from `now_ms` arguments, and
//! every collection that affects output order is ordered.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use serde::Serialize;
use shadowrunr_types::character::{Persona, SegmentFlavour, Tuning};
use shadowrunr_types::{
    Case, ChatMsg, Command, EvidenceGrade, Hex, HexKind, HexRole, LinkRel, ModStage, Op, Rank,
    ScreenId, SegmentContext, SegmentId, ShownRecord, StreamEvent, TsMs, VoteTally,
};

use crate::case::{demo_case, validate_case};
use crate::cases::{StagedCase, StreamKind};
use crate::command::{normalize_hex, parse_command};
use crate::evidence::{ResearchAllowlist, SourceAllowlist};
use crate::ranks::RankTable;
use crate::research::{target_hex, CaseFile};
use crate::rounds::{
    foil_name, Cue, RoundTimes, Rounds, BALLOT_STATE, CLOSING_STATE, INTRO_STATE, SCENE_STATE,
    SCENE_TIMEOUT_RULE, SCENE_UP_PREFIX, TALK_STATE,
};
use crate::scenes::TableRead;
use crate::table::{
    op_tag, screen_name, segment_name, segment_table, ArcRules, ColdOpenRule, GuardFacts,
    HandoverRule, SegmentTable, Trigger, COLD_OPEN_STATE,
};
use crate::SegmentError;

/// How many chat message ids the runtime remembers for `highlight` validation.
const SEEN_MSG_CAP: usize = 10_000;

/// How long a refused op stays named in the state summary.
const REFUSED_SHOWN_MS: TsMs = 60_000;

/// A screen default the runtime picks itself (a research job came back) never overrides a
/// `show_screen` the model made this recently. ASSUMPTION: about two beats, so his choice holds
/// while he talks about it; uncalibrated. The engine default of `[tuning] screen_hold_ms`.
pub const SCREEN_HOLD_MS: TsMs = Tuning::DEFAULT.screen_hold_ms;

/// After this long on `browser`, the runtime moves to `desk`: the record stays up and the board
/// behind it shows the record's card. ASSUMPTION: a record read aloud takes about half a minute,
/// and the 2026-10-21 10-minute show sat on one record for over five; uncalibrated. The engine
/// default of `[tuning] browser_dwell_ms`.
pub const BROWSER_DWELL_MS: TsMs = Tuning::DEFAULT.browser_dwell_ms;

/// After this long on `desk`, the runtime pulls back to the board's overview to choose the
/// next card. ASSUMPTION: uncalibrated, like [`BROWSER_DWELL_MS`]. The engine default of
/// `[tuning] desk_dwell_ms`.
pub const DESK_DWELL_MS: TsMs = Tuning::DEFAULT.desk_dwell_ms;

/// Records on file the state summary's screen line names, newest first.
const SCREEN_RECORDS_SHOWN: usize = 4;

/// Open leads the state summary's board line names, newest record first.
const BOARD_LEADS_SHOWN: usize = 3;

/// Runtime configuration. Every field has a working default.
#[derive(Debug, Clone, Default)]
pub struct SegmentConfig {
    /// Foil ids from the character package (`character.toml` `foils`), for SEG-01.
    pub foils: Vec<String>,
    /// The SEG-02A case. `None` loads the fictional demo case (`fictional: true`).
    pub case: Option<Case>,
    /// The case's records (see [`crate::research`]): planted for a fictional case, staged from
    /// the public record for a real one. `None` gives the demo case its built-in pack when
    /// `case` is `None` too, and a custom case no records.
    pub case_file: Option<CaseFile>,
    /// Who the stream is for. A real case (`fictional: false`) on a live stream needs
    /// `case_approved`; a private simulation may run it unapproved.
    pub stream: StreamKind,
    /// The owner approved the real case in `case` for a live stream (from the case store,
    /// [`SegmentConfig::with_staged_case`]).
    pub case_approved: bool,
    pub ranks: RankTable,
    pub allowlist: SourceAllowlist,
    /// The research allowlist a real case's records were fetched under.
    pub research_allowlist: ResearchAllowlist,
    /// Planned length of the segment in seconds. With the table's `[arc]` and a plan at least
    /// its `min_planned_s` long, the case runs as a multi-act arc.
    pub planned_s: Option<u32>,
    /// Overrides the table's handover thresholds (assumptions) for segments that have one.
    pub handover: Option<HandoverRule>,
    /// Overrides the table's cold-open rule (assumptions) for segments that have one. A
    /// ceiling (`max_s`) of 0 skips the cold open: the segment starts in the state after it.
    pub cold_open: Option<ColdOpenRule>,
    /// Segment start time. `None`: the first `now_ms` the runtime sees.
    pub start_ms: Option<TsMs>,
    /// The character's words: the segment prompts and poll questions are filled from them
    /// (the engine's neutral persona by default).
    pub persona: Persona,
    /// The tuning view the runtime reads (`screen_hold_ms`, `browser_dwell_ms`,
    /// `desk_dwell_ms`); the engine defaults unless the package overrides them.
    pub tuning: Tuning,
    /// The package's per-segment flavour (`[segments."SEG-01"]`), appended to the prompts.
    pub flavour: BTreeMap<SegmentId, SegmentFlavour>,
    /// The stream's Table Read (from the prep's `table_read`, or the fictional sample for a
    /// run that starts in SEG-03): the guest and the playable scenes in ballot order. A segment
    /// that plays scenes refuses to start without one; with one, a handover goes to the
    /// segment that plays scenes ([`crate::scenes::table_read_for`]).
    pub table_read: Option<TableRead>,
    /// Overrides the scene rounds' clock (assumptions, [`RoundTimes::DEFAULT`]).
    pub round_times: Option<RoundTimes>,
}

impl SegmentConfig {
    /// This config running a staged real case for a stream of `kind`: the map, its public
    /// records and the owner's approval (of exactly this content) from the case store.
    pub fn with_staged_case(mut self, staged: &StagedCase, kind: StreamKind) -> Self {
        self.case = Some(staged.case.clone());
        self.case_file = Some(staged.file.clone());
        self.stream = kind;
        self.case_approved = staged.approved_for_stream();
        self
    }
}

/// The multi-act arc a long SEG-02A stream runs (the table's `[arc]`).
#[derive(Debug, Clone)]
struct ArcPlan {
    acts: Vec<String>,
    /// Act index of every hex, in `case.hexes` order.
    act_of: Vec<usize>,
    planned_ms: TsMs,
    seal_ms: TsMs,
    /// Acts whose opening card has been shown (act 0 opens with the segment).
    announced: usize,
    sealed_announced: bool,
    /// Hexes staged for each act.
    staged: Vec<usize>,
    /// Reveals inside an act wait for their slot (`[arc] reveal_pacing`).
    pacing: bool,
    /// Least gap between two reveals, percent of the running act's slot.
    gap_pct: u32,
    /// A hex needs a search or a closed crowd vote before its reveal.
    needs_work: bool,
    /// Direction only: at most one act recap per this many seconds (0: not said).
    recap_cooldown_s: u32,
}

impl ArcPlan {
    /// The arc for `case`, when the table has one and the plan is long enough. The table sets
    /// the number of acts and their timing. Acts come from the case's own `acts` (the evidence
    /// board's regions, which also name the acts), then from the case file's `arc` (research
    /// staged them); a hex neither names, or every hex of a case without them, is dealt into
    /// the acts in kind order (entities and claims first, then domains, shells and wallets),
    /// an even share each. An act past the table's last joins the last.
    fn new(rules: &ArcRules, planned_s: u32, case: &Case, file: Option<&CaseFile>) -> Option<Self> {
        if planned_s < rules.min_planned_s || rules.acts.len() < 2 {
            return None;
        }
        let n = rules.acts.len();
        let mut act_of: Vec<Option<usize>> = vec![None; case.hexes.len()];
        let mut place = |ids: &[String], k: usize| {
            for id in ids {
                if let Some(i) = case.hexes.iter().position(|h| &h.id == id) {
                    act_of[i].get_or_insert(k.min(n - 1));
                }
            }
        };
        for (k, act) in case.acts.iter().enumerate() {
            place(&act.hexes, k);
        }
        if let Some(f) = file {
            for (k, ids) in f.arc.iter().enumerate() {
                place(ids, k);
            }
        }
        let kind_rank = |k: HexKind| match k {
            HexKind::Entity => 0,
            HexKind::Claim => 1,
            HexKind::Domain => 2,
            HexKind::ShellCompany => 3,
            HexKind::Wallet => 4,
        };
        let mut loose: Vec<usize> = (0..case.hexes.len())
            .filter(|&i| act_of[i].is_none())
            .collect();
        loose.sort_by_key(|&i| (kind_rank(case.hexes[i].kind), i));
        let total = loose.len();
        for (rank, i) in loose.into_iter().enumerate() {
            act_of[i] = Some((rank * n / total.max(1)).min(n - 1));
        }
        let act_of: Vec<usize> = act_of.into_iter().map(|a| a.unwrap_or(0)).collect();
        let mut staged = vec![0; n];
        for &k in &act_of {
            staged[k] += 1;
        }
        // The case's own act names its board region; a table act it does not reach keeps the
        // table's name.
        let acts = rules
            .acts
            .iter()
            .enumerate()
            .map(|(k, name)| case.acts.get(k).map_or(name, |a| &a.name).clone())
            .collect();
        Some(Self {
            acts,
            act_of,
            planned_ms: i64::from(planned_s) * 1000,
            seal_ms: i64::from(planned_s.saturating_sub(rules.seal_s)) * 1000,
            announced: 0,
            sealed_announced: false,
            staged,
            pacing: rules.reveal_pacing,
            gap_pct: rules.reveal_gap_pct,
            needs_work: rules.reveal_needs_work,
            recap_cooldown_s: rules.recap_cooldown_s,
        })
    }

    /// Where act `k`'s reveals must be done: the next act's opening, or the seal window.
    fn reveal_end_ms(&self, k: usize) -> TsMs {
        if k + 1 < self.acts.len() {
            self.opens_ms(k + 1)
        } else {
            self.seal_ms
        }
    }

    /// One reveal slot of act `k`: its reveal span shared by the hexes staged for it.
    fn slot_ms(&self, k: usize) -> TsMs {
        let span = self.reveal_end_ms(k) - self.opens_ms(k);
        let hexes = self.staged.get(k).copied().unwrap_or(0).max(1) as i64;
        (span / hexes).max(1)
    }

    /// The act running `elapsed_ms` into the segment.
    fn act_at(&self, elapsed_ms: TsMs) -> usize {
        let n = self.acts.len() as i64;
        let k = elapsed_ms.max(0).saturating_mul(n) / self.planned_ms.max(1);
        usize::try_from(k.clamp(0, n - 1)).unwrap_or(0)
    }

    /// When act `k` opens, ms into the segment.
    fn opens_ms(&self, k: usize) -> TsMs {
        self.planned_ms * k as i64 / self.acts.len() as i64
    }

    fn sealing(&self, elapsed_ms: TsMs) -> bool {
        elapsed_ms >= self.seal_ms
    }
}

fn roman(n: usize) -> String {
    match n {
        1 => "I".into(),
        2 => "II".into(),
        3 => "III".into(),
        4 => "IV".into(),
        5 => "V".into(),
        _ => n.to_string(),
    }
}

fn minutes(ms: TsMs) -> i64 {
    ms / 60_000
}

/// Whole minutes, rounded to the nearest (at least 1).
fn round_minutes(ms: TsMs) -> i64 {
    ((ms + 30_000) / 60_000).max(1)
}

/// A queued `!hint` lead. The text is untrusted chat; it is never put into the prompt
/// context by the runtime (only counts are).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lead {
    pub viewer_id: String,
    pub text: String,
    pub ts_ms: TsMs,
}

/// A string pinned on the evidence board: an accepted `pin_link`, with its record's grade
/// (appendix (screens and board)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PinnedLink {
    pub from: String,
    pub to: String,
    pub rel: LinkRel,
    pub record_id: String,
    pub grade: EvidenceGrade,
}

/// An accepted `!propose`. Untrusted chat text, like [`Lead`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub msg_id: String,
    pub viewer_id: String,
    pub rank: Rank,
    pub text: String,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollKind {
    Crowd,
    Foil,
    /// Opened by the runtime when a hex's searches run out: the audience picks its next move.
    Plebiscite,
    /// Opened by the runtime between scene rounds: the audience picks the next scene.
    Scene,
}

#[derive(Debug, Clone)]
struct Poll {
    id: String,
    question: String,
    options: Vec<String>,
    min_rank: Rank,
    closes_ms: TsMs,
    kind: PollKind,
    /// The hex a Plebiscite decides; `None` for other polls.
    subject: Option<String>,
    /// viewer id -> (option index, weight). One vote per viewer; the last one counts.
    votes: BTreeMap<String, (usize, f64)>,
}

impl Poll {
    fn option_index(&self, raw: &str) -> Option<usize> {
        let raw = raw.trim();
        if let Ok(n) = raw.parse::<usize>() {
            if (1..=self.options.len()).contains(&n) {
                return Some(n - 1);
            }
        }
        self.options
            .iter()
            .position(|o| o.trim().eq_ignore_ascii_case(raw))
    }

    fn tallies(&self) -> Vec<VoteTally> {
        let mut out: Vec<VoteTally> = self
            .options
            .iter()
            .map(|o| VoteTally {
                option: o.clone(),
                voters: 0,
                weight: 0.0,
            })
            .collect();
        for (idx, w) in self.votes.values() {
            out[*idx].voters += 1;
            out[*idx].weight += w;
        }
        out
    }

    fn summary(&self, now_ms: TsMs) -> String {
        let tallies = self
            .tallies()
            .iter()
            .map(|t| format!("{} = {:.1} ({} votes)", t.option, t.weight, t.voters))
            .collect::<Vec<_>>()
            .join(", ");
        let left = ((self.closes_ms - now_ms).max(0) + 999) / 1000;
        let kind = match self.kind {
            PollKind::Crowd => "crowd poll",
            PollKind::Foil => "foil vote",
            PollKind::Plebiscite => "Plebiscite",
            PollKind::Scene => "scene ballot",
        };
        format!(
            "open {kind} {}: \"{}\" | {tallies} | closes in {left} s | min rank {:?}",
            self.id, self.question, self.min_rank
        )
    }
}

/// How a vote closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    NoVotes,
    Winner(String),
    /// Two or more options share the top weight and the top voter count. The runtime never
    /// picks among them: the host breaks the tie on air.
    Tie {
        options: Vec<String>,
        voters: u32,
    },
}

impl PollOutcome {
    /// The tied options as "A and B" / "A, B and C".
    fn tied(options: &[String]) -> String {
        match options.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => options.join(""),
        }
    }
}

/// Highest weight wins; equal weight goes to more voters. Options level on both are a tie,
/// in option order.
fn outcome(tallies: &[VoteTally]) -> PollOutcome {
    let mut top: Vec<&VoteTally> = Vec::new();
    for t in tallies.iter().filter(|t| t.voters > 0) {
        let Some(b) = top.first() else {
            top.push(t);
            continue;
        };
        let level = (t.weight - b.weight).abs() <= 1e-9;
        if t.weight > b.weight + 1e-9 || (level && t.voters > b.voters) {
            top = vec![t];
        } else if level && t.voters == b.voters {
            top.push(t);
        }
    }
    match top.as_slice() {
        [] => PollOutcome::NoVotes,
        [one] => PollOutcome::Winner(one.option.clone()),
        many => PollOutcome::Tie {
            options: many.iter().map(|t| t.option.clone()).collect(),
            voters: many[0].voters,
        },
    }
}

/// The outright winner. No votes or a tie, no winner.
fn winner(tallies: &[VoteTally]) -> Option<String> {
    match outcome(tallies) {
        PollOutcome::Winner(w) => Some(w),
        _ => None,
    }
}

fn moderation(
    ts_ms: TsMs,
    stage: ModStage,
    rule: &str,
    msg_id: Option<&str>,
    action: String,
) -> StreamEvent {
    StreamEvent::Moderation {
        ts_ms,
        stage,
        rule: rule.to_string(),
        msg_id: msg_id.map(str::to_string),
        action,
    }
}

/// How the state summary words a command-gate rule.
fn decline_reason(rule: &str) -> &str {
    match rule {
        "command_not_in_segment" => "not in this segment",
        "command_closed_in_state" => "closed for now",
        "no_open_poll" => "no poll open",
        "unknown_option" => "not an option",
        "foil_vote_not_open" => "no foil vote open",
        "unknown_foil" => "no such foil",
        other => other,
    }
}

fn command_name(c: &Command) -> &'static str {
    match c {
        Command::Summon { .. } => "summon",
        Command::Hint { .. } => "hint",
        Command::Grade { .. } => "grade",
        Command::Propose { .. } => "propose",
        Command::Vote { .. } => "vote",
    }
}

const GRADES: [EvidenceGrade; 4] = [
    EvidenceGrade::Doc,
    EvidenceGrade::Rep,
    EvidenceGrade::Clm,
    EvidenceGrade::Spc,
];

/// An op as the state summary names it: its tag and what it targets.
fn op_label(op: &Op) -> String {
    match op {
        Op::StartPoll { poll_id, .. } => format!("start_poll {poll_id}"),
        Op::ClosePoll { poll_id } => format!("close_poll {poll_id}"),
        Op::RevealHex { hex, .. } => format!("reveal_hex {hex}"),
        Op::SummonFoil { foil } => format!("summon_foil {foil}"),
        other => op_tag(other).to_string(),
    }
}

/// What a card is, as the board line names it: its role, else its kind.
fn card_kind(h: &Hex) -> &'static str {
    match (h.role, h.kind) {
        (Some(HexRole::Victim), _) => "victim",
        (Some(HexRole::Promoter), _) => "promoter",
        (Some(HexRole::Shell), _) => "shell",
        (Some(HexRole::Processor), _) => "processor",
        (Some(HexRole::Regulator), _) => "regulator",
        (Some(HexRole::Claim), _) => "claim",
        (Some(HexRole::Wallet), _) => "wallet",
        (Some(HexRole::Exchange), _) => "exchange",
        (Some(HexRole::Person), _) => "person",
        (Some(HexRole::Company), _) => "company",
        (None, HexKind::Entity) => "entity",
        (None, HexKind::Wallet) => "wallet",
        (None, HexKind::Domain) => "domain",
        (None, HexKind::ShellCompany) => "shell company",
        (None, HexKind::Claim) => "claim",
    }
}

fn grade_name(g: EvidenceGrade) -> &'static str {
    match g {
        EvidenceGrade::Doc => "DOC",
        EvidenceGrade::Rep => "REP",
        EvidenceGrade::Clm => "CLM",
        EvidenceGrade::Spc => "SPC",
    }
}

/// A Table Read the runtime can play: the guest is a declared foil, every scene is the guest's
/// and has lines, and ids and titles are distinct (a ballot option is a title), at most
/// `max_options` of them.
fn check_table_read(
    tr: &TableRead,
    foils: &[String],
    max_options: usize,
) -> Result<(), SegmentError> {
    let bad = |detail: String| Err(SegmentError::Config { detail });
    if !foils.contains(&tr.guest) {
        return bad(format!(
            "the Table Read's guest {:?} is not a declared foil",
            tr.guest
        ));
    }
    if tr.scenes.len() > max_options {
        return bad(format!(
            "the Table Read has {} scenes, at most {max_options}",
            tr.scenes.len()
        ));
    }
    let mut ids = BTreeSet::new();
    let mut titles = BTreeSet::new();
    for s in &tr.scenes {
        if s.guest != tr.guest || s.lines.is_empty() || s.title.trim().is_empty() {
            return bad(format!(
                "scene {:?} is not a playable scene for {}",
                s.id, tr.guest
            ));
        }
        if !ids.insert(s.id.as_str()) || !titles.insert(s.title.trim().to_lowercase()) {
            return bad(format!("scene {:?}: ids and titles must be distinct", s.id));
        }
    }
    Ok(())
}

pub struct SegmentRuntime {
    config: SegmentConfig,
    table: &'static SegmentTable,
    state: String,
    case: Case,
    started_ms: Option<TsMs>,
    last_now_ms: TsMs,
    poll: Option<Poll>,
    used_poll_ids: BTreeSet<String>,
    /// The last crowd poll closed: its id and how it closed.
    last_crowd: Option<(String, PollOutcome)>,
    /// The last foil vote that closed level: its id and the tied foils, until a foil is
    /// summoned or another foil vote opens.
    foil_tie: Option<(String, Vec<String>)>,
    /// Cold open: viewer id -> when they last wrote (ingest time), while the room fills.
    chatters: BTreeMap<String, TsMs>,
    /// Cold open: since when the active chatters have held at the rule's count.
    room_since: Option<TsMs>,
    /// When the cold open ended and why ("room filled ..." / "ceiling ...").
    cold_open_ended: Option<(TsMs, String)>,
    /// Ops refused lately, newest last: (when, "close_poll p3: no_such_poll"). The state
    /// summary names them so the model never tells chat a refused op happened: the 3 h case
    /// run (2026-10-20) narrated polls as open after 16 refused start_polls.
    refused: Vec<(TsMs, String)>,
    /// Chat commands the command gate declined lately, newest last: (when, command, rule).
    /// Counts only, never chat text: the state summary tells the host what chat reached for
    /// so the decline can be acknowledged in-world.
    declined: Vec<(TsMs, &'static str, &'static str)>,
    foil_vote_seq: u32,
    active_foil: Option<String>,
    /// A foil carried over from a previous segment as a "skeptical observer".
    observer_foil: Option<String>,
    ccv_since: Option<TsMs>,
    handover_pending_since: Option<TsMs>,
    /// hex id -> viewer id -> (grade, weight). Last grade per viewer per hex counts.
    grades: BTreeMap<String, BTreeMap<String, (EvidenceGrade, f64)>>,
    /// hex id -> viewer id -> lead. Last lead per viewer per hex counts.
    leads: BTreeMap<String, BTreeMap<String, Lead>>,
    proposals: Vec<Proposal>,
    jobs: BTreeSet<String>,
    /// Planted records of a fictional case; `None` when the case has none.
    case_file: Option<CaseFile>,
    /// hex id -> research briefs accepted against it this stream (the re-search cap).
    searches: BTreeMap<String, u32>,
    /// Ids of planted records the bone-clerks have brought back, in the order found.
    filed: Vec<String>,
    /// A hex whose searches ran out, waiting for a poll slot to open its Plebiscite.
    plebiscite_due: Option<String>,
    /// Hexes that have had their Plebiscite (one per hex per stream).
    plebiscited: BTreeSet<String>,
    /// The last closed Plebiscite: its hex and how it closed.
    last_plebiscite: Option<(String, PollOutcome)>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    signed_off: bool,
    /// The multi-act arc, when the stream is planned long enough (SEG-02A).
    arc: Option<ArcPlan>,
    /// Elapsed ms of every fog-lifting reveal under the arc, in order (the reveal pacing).
    reveals: Vec<TsMs>,
    /// Hexes a closed crowd vote chose (a poll's winner naming it, or an "Annex" Plebiscite).
    voted: BTreeSet<String>,
    /// What fills the broadcast's main screen now.
    screen: ScreenId,
    /// The record last opened in the browser window: `browser`/`desk` without a `record_id`
    /// reopen it.
    screen_record: Option<String>,
    /// The hex the map or board is zoomed to on `hexmap`/`board`/`desk` (`None`: the
    /// overview). On `desk` without a `focus` it is the record's hex, as the stage draws it.
    screen_focus: Option<String>,
    /// When the screen up was last put up, by anyone (the dwell defaults).
    screen_since: TsMs,
    /// When the model's last accepted `show_screen` was made (the hold rule).
    llm_screen_ms: Option<TsMs>,
    /// Records already sent as `record_shown` this session.
    shown: BTreeSet<String>,
    /// Accepted research job id -> the records its dispatch filed, in filing order, until the
    /// job comes back ([`SegmentRuntime::job_done`]).
    job_filed: BTreeMap<String, Vec<String>>,
    /// Filed records whose job has come back, in the order they came back: only these may
    /// be shown.
    returned: Vec<String>,
    /// `record_shown` events an accepted op emits before its own echo.
    pre_echo: Vec<StreamEvent>,
    /// The strings pinned on the evidence board, in pinning order.
    links: Vec<PinnedLink>,
    /// The scene rounds, while the segment plays scenes ([`crate::rounds`]).
    rounds: Option<Rounds>,
}

impl SegmentRuntime {
    /// A runtime with the default config (no foils, the fictional demo case).
    ///
    /// # Panics
    ///
    /// Panics for a descoped segment (SEG-02B-I, SEG-02B-II). Use [`Self::with_config`] to
    /// get the refusal as an error instead.
    pub fn new(segment: SegmentId) -> Self {
        Self::with_config(segment, SegmentConfig::default()).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A runtime for `segment`. Refuses descoped segments with [`SegmentError::Descoped`] and
    /// rejects an invalid config (bad foil ids, a broken case, zero thresholds).
    pub fn with_config(segment: SegmentId, config: SegmentConfig) -> Result<Self, SegmentError> {
        let table = Self::startable_table(segment)?;
        let mut foils = BTreeSet::new();
        for f in &config.foils {
            let ok = !f.is_empty()
                && f.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                });
            if !ok || !foils.insert(f.as_str()) {
                return Err(SegmentError::Config {
                    detail: format!("foil id {f:?} is not a unique lowercase slug"),
                });
            }
        }
        if let Some(h) = &config.handover {
            if h.ccv_min == 0 || h.max_elapsed_s == 0 {
                return Err(SegmentError::Config {
                    detail: "handover thresholds must be positive".into(),
                });
            }
        }
        if let (Some(c), Some(_)) = (&config.cold_open, &table.clock.cold_open) {
            c.check(table).map_err(|e| SegmentError::Config {
                detail: format!("cold open: {e}"),
            })?;
        }
        if let Some(t) = &config.round_times {
            t.check()
                .map_err(|detail| SegmentError::Config { detail })?;
        }
        if let Some(tr) = &config.table_read {
            check_table_read(tr, &config.foils, table.clock.max_poll_options)?;
        }
        let rounds = Self::rounds_for(table, &config)?;
        let case = config.case.clone().unwrap_or_else(demo_case);
        validate_case(&case)?;
        // The owner gate: a real case goes out live only once the owner approved it.
        if !case.fictional && config.stream == StreamKind::Live && !config.case_approved {
            return Err(SegmentError::NotApproved {
                case_id: case.id.clone(),
            });
        }
        let case_file = match (&config.case, &config.case_file) {
            (_, Some(file)) => Some(file.clone()),
            (None, None) => Some(CaseFile::demo()),
            (Some(_), None) => None,
        };
        if let Some(file) = &case_file {
            file.validate_with(&case, &config.allowlist, &config.research_allowlist)?;
        }
        let arc = match (&table.arc, config.planned_s) {
            (Some(rules), Some(planned)) if table.allows_op("reveal_hex") => {
                ArcPlan::new(rules, planned, &case, case_file.as_ref())
            }
            _ => None,
        };
        let mut rt = Self {
            screen: table.default_screen(&table.initial),
            screen_record: None,
            screen_focus: None,
            screen_since: 0,
            llm_screen_ms: None,
            shown: BTreeSet::new(),
            job_filed: BTreeMap::new(),
            returned: Vec::new(),
            pre_echo: Vec::new(),
            links: Vec::new(),
            // The guest is the active foil for the whole segment.
            active_foil: rounds.as_ref().map(|r| r.guest.clone()),
            rounds,
            started_ms: config.start_ms,
            config,
            table,
            state: table.initial.clone(),
            case,
            last_now_ms: 0,
            poll: None,
            used_poll_ids: BTreeSet::new(),
            last_crowd: None,
            foil_tie: None,
            chatters: BTreeMap::new(),
            room_since: None,
            cold_open_ended: None,
            refused: Vec::new(),
            declined: Vec::new(),
            foil_vote_seq: 0,
            observer_foil: None,
            ccv_since: None,
            handover_pending_since: None,
            grades: BTreeMap::new(),
            leads: BTreeMap::new(),
            proposals: Vec::new(),
            jobs: BTreeSet::new(),
            case_file,
            searches: BTreeMap::new(),
            filed: Vec::new(),
            plebiscite_due: None,
            plebiscited: BTreeSet::new(),
            last_plebiscite: None,
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            signed_off: false,
            arc,
            reveals: Vec::new(),
            voted: BTreeSet::new(),
        };
        rt.skip_cold_open();
        Ok(rt)
    }

    /// A cold-open override with a ceiling of 0 starts the segment past its cold open.
    fn skip_cold_open(&mut self) {
        if let Some(rule) = self.cold_open_rule().filter(|r| r.max_s == 0) {
            if self.in_cold_open() {
                self.state = rule.to;
                self.screen = self.table.default_screen(&self.state);
            }
        }
    }

    /// The scene rounds of `table` when it plays scenes: the config's Table Read, or the
    /// refusal to start without a playable scene.
    fn rounds_for(
        table: &SegmentTable,
        config: &SegmentConfig,
    ) -> Result<Option<Rounds>, SegmentError> {
        if !table.plays_scenes() {
            return Ok(None);
        }
        match &config.table_read {
            Some(tr) if !tr.scenes.is_empty() => Ok(Some(Rounds::new(
                tr,
                config.round_times.unwrap_or_default(),
            ))),
            _ => Err(SegmentError::Config {
                detail: format!(
                    "{} ({}) plays scenes and has no playable scene: the Table Read needs at \
                     least one approved, clean scene",
                    segment_name(table.id),
                    table.title
                ),
            }),
        }
    }

    /// A handover goes to the table's target, or, when the stream has a Table Read, to the
    /// segment among `switch_to` that plays scenes.
    fn handover_target(&self, default: SegmentId) -> SegmentId {
        if self
            .config
            .table_read
            .as_ref()
            .is_none_or(|t| t.scenes.is_empty())
        {
            return default;
        }
        self.table
            .switch_to
            .iter()
            .copied()
            .find(|s| segment_table(*s).is_ok_and(|t| t.plays_scenes() && !t.is_descoped()))
            .unwrap_or(default)
    }

    fn startable_table(segment: SegmentId) -> Result<&'static SegmentTable, SegmentError> {
        let table = segment_table(segment)?;
        if table.is_descoped() {
            return Err(SegmentError::Descoped {
                segment,
                title: table.title.clone(),
            });
        }
        Ok(table)
    }

    /// Mark the segment as started at `now_ms` and return its `segment_start` event, followed
    /// in SEG-02A by `case_open` carrying the case being audited, then the runtime's
    /// `show_screen` of the segment's default screen.
    /// Optional: without it the start time is the first `now_ms` seen.
    pub fn start(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        self.started_ms = Some(now_ms);
        self.last_now_ms = now_ms;
        let mut events = vec![StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment: self.table.id,
        }];
        events.extend(self.case_open(now_ms, self.table.id));
        events.push(self.screen_start(now_ms));
        if let Some(card) = self.arc_card(now_ms, 0) {
            events.push(card);
        }
        events
    }

    /// The `case_open` event that follows a `segment_start` into SEG-02A; nothing otherwise.
    fn case_open(&self, now_ms: TsMs, segment: SegmentId) -> Option<StreamEvent> {
        (segment == SegmentId::Seg02aHexmap).then(|| StreamEvent::CaseOpen {
            ts_ms: now_ms,
            case: self.case.clone(),
        })
    }

    // ----- accessors -------------------------------------------------------------------

    pub fn segment(&self) -> SegmentId {
        self.table.id
    }

    pub fn table(&self) -> &'static SegmentTable {
        self.table
    }

    /// Current state name in the active segment's table.
    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn active_foil(&self) -> Option<&str> {
        self.active_foil.as_deref()
    }

    pub fn open_poll_id(&self) -> Option<&str> {
        self.poll.as_ref().map(|p| p.id.as_str())
    }

    /// Running tallies of the open poll, in option order.
    pub fn open_poll_tallies(&self) -> Option<Vec<VoteTally>> {
        self.poll.as_ref().map(Poll::tallies)
    }

    /// The case map with live fog and grades.
    pub fn case(&self) -> &Case {
        &self.case
    }

    /// The case's planted records, when it has any (fictional cases only).
    pub fn case_file(&self) -> Option<&CaseFile> {
        self.case_file.as_ref()
    }

    /// Research briefs accepted against `hex` this stream.
    pub fn searches(&self, hex: &str) -> u32 {
        self.searches.get(hex).copied().unwrap_or(0)
    }

    /// The hex whose Plebiscite is waiting to open or is open, if any.
    pub fn plebiscite_hex(&self) -> Option<&str> {
        self.poll
            .as_ref()
            .filter(|p| p.kind == PollKind::Plebiscite)
            .and_then(|p| p.subject.as_deref())
            .or(self.plebiscite_due.as_deref())
    }

    pub fn proposals(&self) -> &[Proposal] {
        &self.proposals
    }

    /// Queued leads for a hex, ordered by viewer id.
    pub fn leads(&self, hex: &str) -> Vec<&Lead> {
        self.leads
            .get(hex)
            .map(|m| m.values().collect())
            .unwrap_or_default()
    }

    /// Rank-weighted community grades for a hex (DOC, REP, CLM, SPC order).
    pub fn jury_tally(&self, hex: &str) -> Vec<VoteTally> {
        let mut out: Vec<VoteTally> = GRADES
            .iter()
            .map(|g| VoteTally {
                option: grade_name(*g).to_string(),
                voters: 0,
                weight: 0.0,
            })
            .collect();
        if let Some(votes) = self.grades.get(hex) {
            for (g, w) in votes.values() {
                let i = GRADES.iter().position(|x| x == g).unwrap_or(0);
                out[i].voters += 1;
                out[i].weight += w;
            }
        }
        out
    }

    /// The jury's leading grade for a hex: advice for the brain, never proof. A level jury
    /// leads with nothing.
    pub fn jury_grade(&self, hex: &str) -> Option<EvidenceGrade> {
        let w = winner(&self.jury_tally(hex))?;
        GRADES.iter().copied().find(|g| grade_name(*g) == w)
    }

    pub fn is_signed_off(&self) -> bool {
        self.signed_off
    }

    /// What fills the broadcast's main screen now.
    pub fn screen(&self) -> ScreenId {
        self.screen
    }

    /// The record last opened in the browser window, if any.
    pub fn screen_record(&self) -> Option<&str> {
        self.screen_record.as_deref()
    }

    /// The strings pinned on the evidence board, in pinning order.
    pub fn links(&self) -> &[PinnedLink] {
        &self.links
    }

    /// The running act (0-based) and the number of acts, when the stream runs an arc.
    pub fn arc_act(&self) -> Option<(usize, usize)> {
        self.arc
            .as_ref()
            .map(|a| (a.act_at(self.elapsed_ms()), a.acts.len()))
    }

    /// The act (0-based) a hex is staged for, when the stream runs an arc.
    pub fn hex_act(&self, hex: &str) -> Option<usize> {
        let arc = self.arc.as_ref()?;
        let i = self.case.hexes.iter().position(|h| h.id == hex)?;
        arc.act_of.get(i).copied()
    }

    /// When the next reveal unlocks, ms into the segment, under an arc with reveal pacing.
    /// A reveal at or after it is due; before it, `reveal_hex` is refused (`reveal_not_due`).
    pub fn next_reveal_ms(&self) -> Option<TsMs> {
        let arc = self.arc.as_ref().filter(|a| a.pacing)?;
        let act = arc.act_at(self.elapsed_ms());
        let open = arc.opens_ms(act);
        let slot = arc.slot_ms(act);
        let done = self.reveals.iter().filter(|&&t| t >= open).count() as i64;
        let mut next = open + done * slot;
        if act + 1 < arc.acts.len() {
            next = next.min(arc.opens_ms(act + 1));
        }
        if let Some(last) = self.reveals.last() {
            next = next.max(last + slot * i64::from(arc.gap_pct) / 100);
        }
        Some(next)
    }

    /// The hex has had its beat of work: a bone-clerk search, or a closed crowd vote chose it.
    pub fn hex_worked(&self, hex: &str) -> bool {
        self.searches(hex) > 0 || self.voted.contains(hex)
    }

    pub fn is_handover_pending(&self) -> bool {
        self.handover_pending_since.is_some()
    }

    /// The segment is in its cold open: the room is still filling.
    pub fn in_cold_open(&self) -> bool {
        self.state == COLD_OPEN_STATE
    }

    /// When the cold open ended and why (`room filled: ...` or `ceiling: ...`).
    pub fn cold_open_ended(&self) -> Option<(TsMs, &str)> {
        self.cold_open_ended
            .as_ref()
            .map(|(t, why)| (*t, why.as_str()))
    }

    /// How the last crowd poll closed: its id and outcome.
    pub fn last_crowd(&self) -> Option<(&str, &PollOutcome)> {
        self.last_crowd.as_ref().map(|(id, o)| (id.as_str(), o))
    }

    /// How the last Plebiscite closed: its hex and outcome.
    pub fn last_plebiscite(&self) -> Option<(&str, &PollOutcome)> {
        self.last_plebiscite.as_ref().map(|(h, o)| (h.as_str(), o))
    }

    // ----- internals -------------------------------------------------------------------

    /// Milliseconds since the segment started, at the latest time seen.
    /// The screen the investigation's defaults put up and focus: the evidence board when the
    /// segment shows it, else the hexmap.
    fn map_screen(&self) -> ScreenId {
        if self.table.screens.iter().any(|n| n == "board") {
            ScreenId::Board
        } else {
            ScreenId::Hexmap
        }
    }

    fn elapsed_ms(&self) -> TsMs {
        self.last_now_ms - self.started_ms.unwrap_or(self.last_now_ms)
    }

    /// The hex at `i` is staged for an act that has not opened yet.
    fn hex_locked(&self, i: usize) -> bool {
        self.arc
            .as_ref()
            .is_some_and(|a| a.act_of.get(i).copied().unwrap_or(0) > a.act_at(self.elapsed_ms()))
    }

    /// `sign_off` waits for the arc's seal window (a fleet-loss sign-off never waits).
    fn sign_off_withheld(&self) -> bool {
        self.arc
            .as_ref()
            .is_some_and(|a| !a.sealing(self.elapsed_ms()))
    }

    /// The opening card of act `k`: a `show_card` op the runtime emits itself.
    fn arc_card(&mut self, now_ms: TsMs, k: usize) -> Option<StreamEvent> {
        let arc = self.arc.as_mut()?;
        if k < arc.announced || k >= arc.acts.len() {
            return None;
        }
        arc.announced = k + 1;
        let hexes: Vec<&str> = self
            .case
            .hexes
            .iter()
            .zip(&arc.act_of)
            .filter(|(_, a)| **a == k)
            .map(|(h, _)| h.id.as_str())
            .collect();
        Some(StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::ShowCard {
                card_id: format!("arc-act-{}", k + 1),
                title: format!("ACT {}: {}", roman(k + 1), arc.acts[k]),
                body: format!("Now open on the map: {}", hexes.join(", ")),
            },
        })
    }

    /// The clock side of the arc: the card of every act that opened, then the seal card.
    fn arc_tick(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let Some(arc) = &self.arc else {
            return events;
        };
        let elapsed = self.elapsed_ms();
        let act = arc.act_at(elapsed);
        let from = arc.announced;
        for k in from..=act {
            events.extend(self.arc_card(now_ms, k));
        }
        if let Some(arc) = self.arc.as_mut() {
            if arc.sealing(elapsed) && !arc.sealed_announced {
                arc.sealed_announced = true;
                events.push(StreamEvent::Op {
                    ts_ms: now_ms,
                    op: Op::ShowCard {
                        card_id: "arc-seal".into(),
                        title: "THE DOSSIER IS SEALED".into(),
                        body: "Recap of the revealed hexes and their grades.".into(),
                    },
                });
            }
        }
        events
    }

    fn observe(&mut self, now_ms: TsMs) {
        self.started_ms.get_or_insert(now_ms);
        self.last_now_ms = self.last_now_ms.max(now_ms);
    }

    fn remember_msg(&mut self, id: &str) {
        if self.seen.insert(id.to_string()) {
            self.seen_order.push_back(id.to_string());
            if self.seen_order.len() > SEEN_MSG_CAP {
                if let Some(old) = self.seen_order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }

    fn facts(&self) -> GuardFacts {
        GuardFacts {
            foil_active: self.active_foil.is_some(),
            any_revealed: self.case.hexes.iter().any(|h| !h.fog),
        }
    }

    /// Run the table interpreter for one trigger.
    fn fire(&mut self, on: Trigger) {
        if let Some(to) = self.table.next_state(&self.state, on, self.facts()) {
            self.state = to.to_string();
        }
    }

    /// The handover rule in force: the config's thresholds over the table's, its target the
    /// segment that plays scenes when the stream has a Table Read ([`Self::handover_target`]).
    fn handover_rule(&self) -> Option<HandoverRule> {
        self.table.clock.handover.map(|table_rule| {
            let mut rule = self.config.handover.unwrap_or(table_rule);
            rule.to = self.handover_target(rule.to);
            rule
        })
    }

    fn cold_open_rule(&self) -> Option<ColdOpenRule> {
        self.table
            .clock
            .cold_open
            .as_ref()
            .map(|table_rule| self.config.cold_open.as_ref().unwrap_or(table_rule).clone())
    }

    /// The cold-open clock: once past the floor, the room filling (the active chatters held
    /// for the sustain window) ends it; the ceiling ends it regardless. The end is put on the
    /// record as a `show_card` the runtime emits itself, its id naming the reason.
    fn cold_open_tick(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        if !self.in_cold_open() {
            return None;
        }
        let rule = self.cold_open_rule()?;
        let window = i64::from(rule.window_s) * 1000;
        self.chatters.retain(|_, t| now_ms - *t < window);
        let active = u32::try_from(self.chatters.len()).unwrap_or(u32::MAX);
        if active >= rule.active_chatters {
            self.room_since.get_or_insert(now_ms);
        } else {
            self.room_since = None;
        }
        let elapsed = now_ms - self.started_ms.unwrap_or(now_ms);
        let filled = elapsed >= i64::from(rule.min_s) * 1000
            && self
                .room_since
                .is_some_and(|s| now_ms - s >= i64::from(rule.sustain_s) * 1000);
        let ceiling = elapsed >= i64::from(rule.max_s) * 1000;
        if !filled && !ceiling {
            return None;
        }
        let (card_id, why, body) = if filled {
            (
                "cold-open-end-room-filled",
                format!(
                    "room filled: {active} chatters (>= {}) held {} s",
                    rule.active_chatters, rule.sustain_s
                ),
                "The hall has filled: the Improvised Room is open.",
            )
        } else {
            (
                "cold-open-end-ceiling",
                format!(
                    "ceiling: {} s with {active} of {} chatters",
                    rule.max_s, rule.active_chatters
                ),
                "The doors stand open: the Improvised Room begins.",
            )
        };
        self.cold_open_ended = Some((now_ms, why));
        self.chatters.clear();
        self.room_since = None;
        self.state = rule.to;
        Some(StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::ShowCard {
                card_id: card_id.into(),
                title: "THE ROOM IS OPEN".into(),
                body: body.into(),
            },
        })
    }

    fn find_hex(&self, raw: &str) -> Option<usize> {
        let canon = normalize_hex(raw);
        self.case
            .hexes
            .iter()
            .position(|h| h.id == raw || canon.as_deref() == Some(h.id.as_str()))
    }

    /// `want`, or `want-2`, `want-3`... when that id was used already.
    fn fresh_poll_id(&self, want: &str) -> String {
        let mut id = want.to_string();
        let mut n = 1;
        while self.used_poll_ids.contains(&id) {
            n += 1;
            id = format!("{want}-{n}");
        }
        id
    }

    fn open_poll(&mut self, poll: Poll) {
        self.used_poll_ids.insert(poll.id.clone());
        self.poll = Some(poll);
    }

    fn vote_event(&self, now_ms: TsMs, poll: &Poll, closed: bool) -> StreamEvent {
        StreamEvent::Vote {
            ts_ms: now_ms,
            poll_id: poll.id.clone(),
            tallies: poll.tallies(),
            closed,
        }
    }

    /// Close the open poll. A foil vote that closes with `resolve` summons its winner.
    fn close_poll(&mut self, now_ms: TsMs, resolve: bool) -> Vec<StreamEvent> {
        let Some(poll) = self.poll.take() else {
            return Vec::new();
        };
        let mut events = vec![self.vote_event(now_ms, &poll, true)];
        match poll.kind {
            PollKind::Crowd => {
                let result = outcome(&poll.tallies());
                // A crowd vote whose winner names one hex counts as that hex's beat of work.
                // On a tie the host breaks it, so every tied option that names a hex counts:
                // whichever he picks may be revealed.
                if resolve {
                    let chosen: &[String] = match &result {
                        PollOutcome::Winner(w) => std::slice::from_ref(w),
                        PollOutcome::Tie { options, .. } => options,
                        PollOutcome::NoVotes => &[],
                    };
                    for i in chosen.iter().filter_map(|o| target_hex(&self.case, o)) {
                        self.voted.insert(self.case.hexes[i].id.clone());
                    }
                }
                self.last_crowd = Some((poll.id.clone(), result));
                self.fire(Trigger::PollClosed);
            }
            PollKind::Plebiscite => {
                if resolve {
                    if let Some(hex) = poll.subject.clone() {
                        // A tie is the host's to break, like a crowd poll's: an "Annex" option
                        // among the tied ones still counts as the hex's vote.
                        let result = outcome(&poll.tallies());
                        let annex = match &result {
                            PollOutcome::Winner(w) => w.starts_with("Annex "),
                            PollOutcome::Tie { options, .. } => {
                                options.iter().any(|o| o.starts_with("Annex "))
                            }
                            PollOutcome::NoVotes => false,
                        };
                        if annex {
                            self.voted.insert(hex.clone());
                        }
                        self.last_plebiscite = Some((hex, result));
                    }
                }
                self.fire(Trigger::PollClosed);
            }
            PollKind::Foil => {
                if resolve {
                    // A level foil vote summons nobody: the host breaks it with summon_foil.
                    match outcome(&poll.tallies()) {
                        PollOutcome::Winner(foil) => {
                            self.active_foil = Some(foil.clone());
                            events.push(StreamEvent::Op {
                                ts_ms: now_ms,
                                op: Op::SummonFoil { foil },
                            });
                        }
                        PollOutcome::Tie { options, .. } => {
                            self.foil_tie = Some((poll.id.clone(), options));
                        }
                        PollOutcome::NoVotes => {}
                    }
                }
                self.fire(Trigger::FoilVoteClosed);
            }
            PollKind::Scene => {
                // The ballot puts its scene up: the winner, else the first tied (or the first
                // left) in ballot order, and the host is told so. A ballot closed by the
                // ceiling or a sign-off puts nothing up.
                if resolve {
                    let result = outcome(&poll.tallies());
                    if let Some(r) = self.rounds.as_mut() {
                        if let Some(i) = r.choose(&result) {
                            r.last_ballot = Some((poll.id.clone(), result.clone(), i));
                            r.cue(
                                i,
                                now_ms,
                                Cue::Ballot {
                                    poll_id: poll.id.clone(),
                                    outcome: result,
                                },
                            );
                            self.state = SCENE_STATE.to_string();
                        }
                    }
                }
            }
        }
        events
    }

    fn switch_to(&mut self, now_ms: TsMs, to: SegmentId) -> Result<Vec<StreamEvent>, SegmentError> {
        let table = Self::startable_table(to)?;
        let rounds = Self::rounds_for(table, &self.config)?;
        let mut events = self.close_poll(now_ms, false);
        events.push(StreamEvent::SegmentEnd {
            ts_ms: now_ms,
            segment: self.table.id,
            ses: None,
        });
        events.push(StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment: to,
        });
        events.extend(self.case_open(now_ms, to));
        self.table = table;
        self.state = table.initial.clone();
        self.started_ms = Some(now_ms);
        self.ccv_since = None;
        self.handover_pending_since = None;
        self.chatters.clear();
        self.room_since = None;
        self.skip_cold_open();
        events.push(self.screen_start(now_ms));
        if let Some(foil) = self.active_foil.take() {
            self.observer_foil = Some(foil);
        }
        // A segment that plays scenes has its guest as the active foil throughout.
        if let Some(r) = &rounds {
            if self.observer_foil.as_deref() == Some(r.guest.as_str()) {
                self.observer_foil = None;
            }
            self.active_foil = Some(r.guest.clone());
        }
        self.rounds = rounds;
        Ok(events)
    }

    // ----- chat ------------------------------------------------------------------------

    /// Commands and votes from new chat. Returns `vote` events (one running tally per poll
    /// or graded hex touched in this batch) and `moderation` events (stage `input`, action
    /// `ignored`) for every declined command: under-ranked, invalid, or refused by
    /// [`Self::command_gate`]. No command is dropped without a named rule.
    pub fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        let mut poll_dirty = false;
        let mut graded: BTreeSet<String> = BTreeSet::new();
        let cold_open = self.in_cold_open();
        for msg in msgs {
            self.remember_msg(&msg.id);
            if self.signed_off {
                continue;
            }
            // The cold open counts who is talking (ingest time: the runtime's own clock).
            if cold_open {
                self.chatters.insert(msg.viewer.id.clone(), now_ms);
            }
            let Some(cmd) = msg.command.clone().or_else(|| parse_command(&msg.text)) else {
                continue;
            };
            let gate = self.command_gate(&cmd);
            if let Err(rule) = gate {
                self.declined
                    .retain(|(t, _, _)| now_ms - t < REFUSED_SHOWN_MS);
                self.declined.push((now_ms, command_name(&cmd), rule));
                events.push(moderation(
                    now_ms,
                    ModStage::Input,
                    rule,
                    Some(&msg.id),
                    "ignored".into(),
                ));
                continue;
            }
            let rank = msg.viewer.rank;
            let weight = self.config.ranks.vote_weight(rank);
            let viewer = msg.viewer.id.clone();
            let ignored = |rule: &str| {
                moderation(
                    now_ms,
                    ModStage::Input,
                    rule,
                    Some(&msg.id),
                    "ignored".into(),
                )
            };
            match cmd {
                Command::Summon { foil } => {
                    // The gate passed: a foil vote is open, or this summon opens one.
                    if self.poll.is_none() {
                        let ev = self.open_foil_vote(now_ms);
                        events.push(ev);
                    }
                    if let Some(p) = self.poll.as_mut().filter(|p| p.kind == PollKind::Foil) {
                        if let Some(i) = p.option_index(&foil) {
                            p.votes.insert(viewer, (i, weight));
                            poll_dirty = true;
                        }
                    }
                }
                Command::Vote { option } => {
                    // The gate passed: a poll is open and `option` names one of its options.
                    let Some(p) = self.poll.as_mut() else {
                        continue;
                    };
                    let Some(i) = p.option_index(&option) else {
                        continue;
                    };
                    if rank < p.min_rank {
                        events.push(ignored("poll_min_rank"));
                        continue;
                    }
                    p.votes.insert(viewer, (i, weight));
                    poll_dirty = true;
                }
                Command::Propose { text } => {
                    if !self.config.ranks.may_propose(rank) {
                        events.push(ignored("propose_min_rank"));
                        continue;
                    }
                    self.proposals.push(Proposal {
                        msg_id: msg.id.clone(),
                        viewer_id: viewer,
                        rank,
                        text,
                        ts_ms: msg.ts_ms,
                    });
                }
                Command::Hint { hex, text } => {
                    let Some(i) = self.find_hex(&hex) else {
                        events.push(ignored("unknown_hex"));
                        continue;
                    };
                    let id = self.case.hexes[i].id.clone();
                    self.leads.entry(id).or_default().insert(
                        viewer.clone(),
                        Lead {
                            viewer_id: viewer,
                            text,
                            ts_ms: msg.ts_ms,
                        },
                    );
                }
                Command::Grade { hex, grade } => {
                    let Some(i) = self.find_hex(&hex) else {
                        events.push(ignored("unknown_hex"));
                        continue;
                    };
                    let id = self.case.hexes[i].id.clone();
                    self.grades
                        .entry(id.clone())
                        .or_default()
                        .insert(viewer, (grade, weight));
                    graded.insert(id);
                }
            }
        }
        if poll_dirty {
            if let Some(p) = &self.poll {
                events.push(self.vote_event(now_ms, p, false));
            }
        }
        for hex in graded {
            events.push(StreamEvent::Vote {
                ts_ms: now_ms,
                tallies: self.jury_tally(&hex),
                poll_id: format!("grade:{hex}"),
                closed: false,
            });
        }
        events
    }

    /// The chat command gate: whether `cmd` may act now, or the rule that declines it. The
    /// segment and the state open commands (`commands`, `[state_rules.X] commands`); a vote
    /// needs an open poll naming its option; a summon needs a foil vote open or a state that
    /// opens one. Per-viewer checks (rank, a hex that does not exist) stay at the command.
    fn command_gate(&self, cmd: &Command) -> Result<(), &'static str> {
        let name = command_name(cmd);
        if !self.table.allows_command(name) {
            return Err("command_not_in_segment");
        }
        if !self.table.allows_command_in(&self.state, name) {
            return Err("command_closed_in_state");
        }
        match cmd {
            Command::Vote { option } => {
                let p = self.poll.as_ref().ok_or("no_open_poll")?;
                if p.option_index(option).is_none() {
                    return Err("unknown_option");
                }
            }
            Command::Summon { foil } => {
                if !self.config.foils.contains(foil) {
                    return Err("unknown_foil");
                }
                let open = match &self.poll {
                    Some(p) => p.kind == PollKind::Foil,
                    None => self.table.accepts(&self.state, Trigger::FoilVoteOpened),
                };
                if !open {
                    return Err("foil_vote_not_open");
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn open_foil_vote(&mut self, now_ms: TsMs) -> StreamEvent {
        let id = loop {
            self.foil_vote_seq += 1;
            let id = format!("foil-vote-{}", self.foil_vote_seq);
            if !self.used_poll_ids.contains(&id) {
                break id;
            }
        };
        let secs = self.table.clock.foil_vote_secs;
        let question = format!("Which foil shall the {} summon?", self.host_title());
        let options = self.config.foils.clone();
        self.foil_tie = None;
        self.open_poll(Poll {
            id: id.clone(),
            question: question.clone(),
            options: options.clone(),
            min_rank: Rank::Thrall,
            closes_ms: now_ms + i64::from(secs) * 1000,
            kind: PollKind::Foil,
            subject: None,
            votes: BTreeMap::new(),
        });
        self.fire(Trigger::FoilVoteOpened);
        StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::StartPoll {
                poll_id: id,
                question,
                options,
                min_rank: Rank::Thrall,
                secs,
            },
        }
    }

    // ----- research --------------------------------------------------------------------

    /// File the planted records of `hex` the bone-clerks have not brought back yet, and attach
    /// their sources to the hex so the DOC rule sees them.
    /// Returns the ids it filed, in filing order.
    fn file_findings(&mut self, hex_idx: usize) -> Vec<String> {
        let mut new = Vec::new();
        let Some(file) = &self.case_file else {
            return new;
        };
        let hex = &mut self.case.hexes[hex_idx];
        for r in file.records_for(&hex.id) {
            if self.filed.contains(&r.id) {
                continue;
            }
            self.filed.push(r.id.clone());
            new.push(r.id.clone());
            if !hex.sources.contains(&r.source) {
                hex.sources.push(r.source.clone());
            }
        }
        new
    }

    /// The options of `hex`'s Plebiscite: annex it, leave it fogged, or send the bone-clerks
    /// to a linked hex that is still fogged and has searches left (up to two, in map order).
    fn plebiscite_options(&self, hex: &str) -> Vec<String> {
        let mut options = vec![format!("Annex {hex}"), format!("Leave {hex} fogged")];
        let cap = self.table.clock.max_searches_per_hex;
        let room = self
            .table
            .clock
            .max_poll_options
            .saturating_sub(options.len());
        let linked: BTreeSet<&str> = self
            .case
            .links
            .iter()
            .filter_map(|(a, b)| match (a == hex, b == hex) {
                (true, _) => Some(b.as_str()),
                (_, true) => Some(a.as_str()),
                _ => None,
            })
            .collect();
        options.extend(
            self.case
                .hexes
                .iter()
                .filter(|h| h.fog && linked.contains(h.id.as_str()) && self.searches(&h.id) < cap)
                .take(room.min(2))
                .map(|h| format!("Search {}", h.id)),
        );
        options
    }

    /// Open the waiting Plebiscite when a poll can open: SEG-02A, no poll open, and a state
    /// that takes `poll_opened`. Yields its `start_poll` op, then the map's overview.
    fn open_due_plebiscite(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        if self.signed_off
            || self.poll.is_some()
            || !self.table.allows_op("dispatch_research")
            || !self.table.accepts(&self.state, Trigger::PollOpened)
        {
            return Vec::new();
        }
        let Some(hex) = self.plebiscite_due.take() else {
            return Vec::new();
        };
        let base = format!("plebiscite-{hex}");
        let mut id = base.clone();
        let mut n = 1;
        while self.used_poll_ids.contains(&id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        let clock = &self.table.clock;
        let secs = clock.plebiscite_secs;
        let question = format!(
            "{} have searched {hex} {} times. What is the {}'s next move?",
            self.fill("The {researchers}"),
            clock.max_searches_per_hex,
            self.host_title()
        );
        let options = self.plebiscite_options(&hex);
        self.open_poll(Poll {
            id: id.clone(),
            question: question.clone(),
            options: options.clone(),
            min_rank: Rank::Thrall,
            closes_ms: now_ms + i64::from(secs) * 1000,
            kind: PollKind::Plebiscite,
            subject: Some(hex),
            votes: BTreeMap::new(),
        });
        self.fire(Trigger::PollOpened);
        let mut events = vec![StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::StartPoll {
                poll_id: id,
                question,
                options,
                min_rank: Rank::Thrall,
                secs,
            },
        }];
        events.extend(self.poll_screen(now_ms));
        events
    }

    /// Research is withheld while a Plebiscite waits or runs: the next move is the audience's.
    fn research_withheld(&self) -> bool {
        self.plebiscite_hex().is_some()
    }

    // ----- ops -------------------------------------------------------------------------

    /// Validate and apply the brain's ops. An accepted op yields its `op` event followed by
    /// any consequence (a closed `vote`, `segment_end`/`segment_start`...), then the new
    /// state's default screen when that state excludes the screen up. A `show_screen` naming
    /// a record for the first time is preceded by its `record_shown`. A rejected op
    /// yields one `moderation` event (stage `output`, action `rejected:<op>`) whose `rule`
    /// names the failed check. A `dispatch_research` refused at the re-search cap
    /// (`research_cap_reached`) is followed by the `start_poll` of that hex's Plebiscite as
    /// soon as a poll can open.
    pub fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
        self.observe(now_ms);
        // An act that opened since the last clock tick is announced before the ops it unlocks.
        let mut events = if self.signed_off {
            Vec::new()
        } else {
            self.arc_tick(now_ms)
        };
        for op in ops {
            match self.apply_op(now_ms, op) {
                Ok(consequences) => {
                    // A record the op names goes out before the op that shows it.
                    events.append(&mut self.pre_echo);
                    let mut op = op.clone();
                    // A re-minted poll id: the transcript records the id the poll has.
                    if let (Op::StartPoll { poll_id, .. }, Some(p)) = (&mut op, &self.poll) {
                        poll_id.clone_from(&p.id);
                    }
                    events.push(StreamEvent::Op { ts_ms: now_ms, op });
                    events.extend(consequences);
                    events.extend(self.screen_fit(now_ms));
                }
                Err(rule) => {
                    self.pre_echo.clear();
                    self.refused.retain(|(t, _)| now_ms - t < REFUSED_SHOWN_MS);
                    self.refused
                        .push((now_ms, format!("{}: {rule}", op_label(op))));
                    events.push(moderation(
                        now_ms,
                        ModStage::Output,
                        rule,
                        None,
                        format!("rejected:{}", op_tag(op)),
                    ))
                }
            }
            events.extend(self.open_due_plebiscite(now_ms));
        }
        events
    }

    fn require_state(&self, on: Trigger) -> Result<(), &'static str> {
        if self.table.accepts(&self.state, on) {
            Ok(())
        } else {
            Err("not_in_state")
        }
    }

    /// Whether the op `tag` may be applied now, whatever its arguments: the gate's
    /// tag-level rules, in the order [`Self::apply_op`] checks them. The op menu
    /// ([`Self::context`]) is this rule's view, and the state summary's `not now:` line names
    /// each withheld tag with its `Err`. Argument checks (`unknown_hex`, `reveal_not_due`,
    /// `invalid_poll_options`...) stay in `apply_op`, so an op on the menu may still be
    /// refused for what it names.
    fn availability(&self, tag: &str) -> Result<(), &'static str> {
        if self.signed_off {
            return Err("signed_off");
        }
        if !self.table.allows_op(tag) {
            return Err("op_not_allowed");
        }
        // The state narrows the segment's ops (the cold open holds polls and foils back).
        if !self.table.allows_op_in(&self.state, tag) {
            return Err("not_in_state");
        }
        match tag {
            "start_poll" => {
                if self.poll.is_some() {
                    return Err("poll_already_open");
                }
                self.require_state(Trigger::PollOpened)
            }
            "close_poll" if self.poll.is_none() => Err("no_such_poll"),
            "summon_foil" => self.require_state(Trigger::FoilSummoned),
            "dismiss_foil" => {
                if self.active_foil.is_none() {
                    return Err("no_active_foil");
                }
                self.require_state(Trigger::FoilDismissed)
            }
            "reveal_hex" => self.require_state(Trigger::HexRevealed),
            // Research is the audience's move while a Plebiscite waits or runs.
            "dispatch_research" if self.research_withheld() => Err("plebiscite_pending"),
            "pin_link" if !self.table.allows_screen_in(&self.state, ScreenId::Board) => {
                Err("screen_not_allowed")
            }
            // An arc is the whole planned segment: it is not handed over midway.
            "switch_segment" if self.arc.is_some() => Err("arc_in_progress"),
            "sign_off" if self.sign_off_withheld() => Err("dossier_not_sealed"),
            // Scene rounds run at least to their floor.
            "sign_off"
                if self
                    .rounds
                    .as_ref()
                    .is_some_and(|r| self.elapsed_ms() < i64::from(r.times.floor_s) * 1000) =>
            {
                Err("segment_floor")
            }
            _ => Ok(()),
        }
    }

    fn apply_op(&mut self, now_ms: TsMs, op: &Op) -> Result<Vec<StreamEvent>, &'static str> {
        // A fleet-loss sign-off never waits for the seal.
        let fleet_lost = matches!(op, Op::SignOff { reason } if reason.starts_with("fleet_lost"));
        match self.availability(op_tag(op)) {
            Err("dossier_not_sealed" | "segment_floor") if fleet_lost => {}
            Err(rule) => return Err(rule),
            Ok(()) => {}
        }
        match op {
            Op::Highlight { msg_id } => {
                if !self.seen.contains(msg_id) {
                    return Err("unknown_msg");
                }
            }
            Op::StartPoll {
                poll_id,
                question,
                options,
                min_rank,
                secs,
            } => {
                if poll_id.trim().is_empty() {
                    return Err("duplicate_poll_id");
                }
                // A reused id is re-minted (`p3` -> `p3-2`) rather than refused: the model
                // reuses short ids, and close_poll by the id it used still finds the poll.
                let poll_id = &self.fresh_poll_id(poll_id);
                if question.trim().is_empty() {
                    return Err("invalid_poll_question");
                }
                let clock = &self.table.clock;
                let distinct: BTreeSet<String> =
                    options.iter().map(|o| o.trim().to_lowercase()).collect();
                if options.len() < 2
                    || options.len() > clock.max_poll_options
                    || distinct.len() != options.len()
                    || distinct.contains("")
                {
                    return Err("invalid_poll_options");
                }
                if !(clock.poll_min_secs..=clock.poll_max_secs).contains(secs) {
                    return Err("invalid_poll_secs");
                }
                self.open_poll(Poll {
                    id: poll_id.clone(),
                    question: question.clone(),
                    options: options.clone(),
                    min_rank: *min_rank,
                    closes_ms: now_ms + i64::from(*secs) * 1000,
                    kind: PollKind::Crowd,
                    subject: None,
                    votes: BTreeMap::new(),
                });
                self.fire(Trigger::PollOpened);
                return Ok(self.poll_screen(now_ms).into_iter().collect());
            }
            Op::ClosePoll { poll_id } => {
                let named = |p: &Poll| {
                    &p.id == poll_id
                        || (p.kind == PollKind::Crowd
                            && p.id
                                .strip_prefix(poll_id.as_str())
                                .and_then(|r| r.strip_prefix('-'))
                                .is_some_and(|n| n.parse::<u32>().is_ok()))
                };
                if !self.poll.as_ref().is_some_and(named) {
                    return Err("no_such_poll");
                }
                return Ok(self.close_poll(now_ms, true));
            }
            Op::SummonFoil { foil } => {
                if !self.config.foils.contains(foil) {
                    return Err("unknown_foil");
                }
                if self.active_foil.as_deref() == Some(foil) {
                    return Err("foil_already_active");
                }
                self.active_foil = Some(foil.clone());
                self.foil_tie = None;
                self.fire(Trigger::FoilSummoned);
            }
            Op::DismissFoil => {
                self.active_foil = None;
                self.fire(Trigger::FoilDismissed);
            }
            Op::RevealHex { hex, grade } => {
                let Some(i) = self.find_hex(hex) else {
                    return Err("unknown_hex");
                };
                if self.hex_locked(i) {
                    return Err("act_locked");
                }
                let h = &self.case.hexes[i];
                if !h.fog && h.grade == Some(*grade) {
                    return Err("already_revealed");
                }
                // Pacing and the beat of work govern lifting fog; a regrade is not a reveal.
                let lifts_fog = h.fog;
                if lifts_fog {
                    if self.next_reveal_ms().is_some_and(|t| self.elapsed_ms() < t) {
                        return Err("reveal_not_due");
                    }
                    if self.arc.as_ref().is_some_and(|a| a.needs_work) && !self.hex_worked(&h.id) {
                        return Err("reveal_needs_work");
                    }
                }
                if *grade == EvidenceGrade::Doc
                    && !self
                        .config
                        .allowlist
                        .hex_has_primary_source(h, self.case.fictional)
                {
                    return Err("doc_requires_primary_source");
                }
                let h = &mut self.case.hexes[i];
                h.fog = false;
                h.grade = Some(*grade);
                let id = h.id.clone();
                if lifts_fog && self.arc.is_some() {
                    self.reveals.push(self.elapsed_ms());
                }
                self.fire(Trigger::HexRevealed);
                // The board zooms to the card just flipped.
                let screen = self.map_screen();
                return Ok(self
                    .screen_default(now_ms, screen, None, Some(id))
                    .into_iter()
                    .collect());
            }
            Op::DispatchResearch { job_id, brief } => {
                if job_id.trim().is_empty() || brief.trim().is_empty() {
                    return Err("invalid_job");
                }
                if self.jobs.contains(job_id) {
                    return Err("duplicate_job_id");
                }
                // A brief that names no single hex is not counted against any hex.
                let target = target_hex(&self.case, brief);
                if target.is_some_and(|i| self.hex_locked(i)) {
                    return Err("act_locked");
                }
                if let Some(i) = target {
                    let hex = self.case.hexes[i].id.clone();
                    if self.searches(&hex) >= self.table.clock.max_searches_per_hex {
                        // One Plebiscite per hex; later attempts are simply refused.
                        if self.plebiscited.insert(hex.clone()) {
                            self.plebiscite_due = Some(hex);
                        }
                        return Err("research_cap_reached");
                    }
                    *self.searches.entry(hex.clone()).or_default() += 1;
                    let filed = self.file_findings(i);
                    if !filed.is_empty() {
                        self.job_filed.insert(job_id.clone(), filed);
                    }
                    self.jobs.insert(job_id.clone());
                    // The minions set off: the board zooms to where they are going.
                    let screen = self.map_screen();
                    return Ok(self
                        .screen_default(now_ms, screen, None, Some(hex))
                        .into_iter()
                        .collect());
                }
                self.jobs.insert(job_id.clone());
            }
            Op::ShowCard { card_id, title, .. } => {
                if card_id.trim().is_empty() || title.trim().is_empty() {
                    return Err("invalid_card");
                }
            }
            // Appendix (screens and board): the board in the state's allow-list, two distinct
            // case cards, both face up, a returned record filed under one of them, and no
            // string with the same ends and rel.
            Op::PinLink {
                from,
                to,
                rel,
                record_id,
            } => return self.pin_link(now_ms, from, to, *rel, record_id),
            // Appendix (screens and board): the state's allow-list, then a record on file for
            // the browser window, then a case hex to focus. A `record_id` on `host`, `hexmap` or
            // `board` and a `focus` on `host` or `browser` are ignored.
            Op::ShowScreen {
                screen,
                record_id,
                focus,
            } => {
                if !self.table.allows_screen_in(&self.state, *screen) {
                    return Err("screen_not_allowed");
                }
                let record = match screen {
                    ScreenId::Browser | ScreenId::Desk => {
                        let Some(id) = record_id.clone().or_else(|| self.screen_record.clone())
                        else {
                            return Err("no_record");
                        };
                        if !self.filed.contains(&id) {
                            return Err("record_not_filed");
                        }
                        // Filed at dispatch, shown only once the bone-clerks are back.
                        if !self.returned.contains(&id) {
                            return Err("record_not_returned");
                        }
                        Some(id)
                    }
                    ScreenId::Host | ScreenId::Hexmap | ScreenId::Board => None,
                };
                let focus = match (screen, focus) {
                    (ScreenId::Hexmap | ScreenId::Board | ScreenId::Desk, Some(raw)) => {
                        let Some(i) = self.find_hex(raw) else {
                            return Err("unknown_hex");
                        };
                        Some(self.case.hexes[i].id.clone())
                    }
                    // The desk's board shows the record's card unless told otherwise.
                    (ScreenId::Desk, None) => record.as_deref().and_then(|id| self.record_hex(id)),
                    _ => None,
                };
                if let Some(id) = record {
                    let shown = self.record_shown(now_ms, &id);
                    self.pre_echo.extend(shown);
                    self.screen_record = Some(id);
                }
                self.screen = *screen;
                self.screen_focus = focus;
                self.screen_since = now_ms;
                self.llm_screen_ms = Some(now_ms);
            }
            Op::MarkMoment { label } => {
                if label.trim().is_empty() {
                    return Err("invalid_moment");
                }
            }
            Op::SwitchSegment { to } => {
                if *to == self.table.id {
                    return Err("already_in_segment");
                }
                if !self.table.switch_to.contains(to) {
                    return Err("switch_not_allowed");
                }
                if Self::startable_table(*to).is_err() {
                    return Err("segment_descoped");
                }
                // A segment with a handover hands over to its target: tonight's Table Read
                // when the prep has one, else the table's.
                if let Some(rule) = self.handover_rule() {
                    if *to != rule.to {
                        return Err("not_the_handover");
                    }
                }
                if Self::rounds_for(
                    segment_table(*to).map_err(|_| "segment_descoped")?,
                    &self.config,
                )
                .is_err()
                {
                    return Err("no_scenes");
                }
                let mut events = Vec::new();
                if self.handover_pending_since.is_none() {
                    events.push(StreamEvent::Handover {
                        ts_ms: now_ms,
                        from: self.table.id,
                        to: *to,
                        reason: "switch_segment op".into(),
                    });
                }
                events.extend(
                    self.switch_to(now_ms, *to)
                        .map_err(|_| "segment_descoped")?,
                );
                return Ok(events);
            }
            Op::SignOff { .. } => {
                let events = self.close_poll(now_ms, false);
                self.signed_off = true;
                return Ok(events);
            }
        }
        Ok(Vec::new())
    }

    /// Validate and pin one string (`pin_link`): its `link_pinned`, then the board's focus on
    /// the record's card when neither the board nor the desk is up. A record pinned for the
    /// first time goes out as `record_shown` before the op, so the stage and the transcript
    /// hold what the string cites.
    fn pin_link(
        &mut self,
        now_ms: TsMs,
        from: &str,
        to: &str,
        rel: LinkRel,
        record_id: &str,
    ) -> Result<Vec<StreamEvent>, &'static str> {
        let (Some(a), Some(b)) = (self.find_hex(from), self.find_hex(to)) else {
            return Err("unknown_hex");
        };
        if a == b {
            return Err("same_hex");
        }
        if self.case.hexes[a].fog || self.case.hexes[b].fog {
            return Err("hex_not_revealed");
        }
        let record = self
            .case_file
            .as_ref()
            .and_then(|f| f.records.iter().find(|r| r.id == record_id))
            .filter(|r| self.returned.contains(&r.id));
        let Some(record) = record else {
            return Err("record_not_returned");
        };
        let (from, to) = (self.case.hexes[a].id.clone(), self.case.hexes[b].id.clone());
        if record.hex != from && record.hex != to {
            return Err("record_not_on_link");
        }
        let (grade, card) = (record.grade, record.hex.clone());
        if self
            .links
            .iter()
            .any(|l| l.from == from && l.to == to && l.rel == rel)
        {
            return Err("duplicate_link");
        }
        let shown = self.record_shown(now_ms, record_id);
        self.pre_echo.extend(shown);
        self.links.push(PinnedLink {
            from: from.clone(),
            to: to.clone(),
            rel,
            record_id: record_id.to_string(),
            grade,
        });
        let mut events = vec![StreamEvent::LinkPinned {
            ts_ms: now_ms,
            from,
            to,
            rel,
            record_id: record_id.to_string(),
            grade,
        }];
        // A string pinned out of view: the board comes up on the card its record is about.
        if !matches!(self.screen, ScreenId::Board | ScreenId::Desk) {
            events.extend(self.screen_default(now_ms, ScreenId::Board, None, Some(card)));
        }
        Ok(events)
    }

    // ----- screens ---------------------------------------------------------------------

    /// The `record_shown` of a filed record, the first time it is named this session. The
    /// fields are the case file's, copied (a real case's records are scrubbed when staged).
    fn record_shown(&mut self, now_ms: TsMs, id: &str) -> Option<StreamEvent> {
        if self.shown.contains(id) {
            return None;
        }
        let file = self.case_file.as_ref()?;
        let r = file.records.iter().find(|r| r.id == id)?;
        let record = ShownRecord {
            id: r.id.clone(),
            hex: r.hex.clone(),
            grade: r.grade,
            title: r.title.clone(),
            source: r.source.clone(),
            summary: r.summary.clone(),
            retrieved: r.retrieved.clone(),
            via: r.via.clone(),
            fictional: file.fictional,
            date: r.date.clone(),
        };
        self.shown.insert(id.to_string());
        Some(StreamEvent::RecordShown {
            ts_ms: now_ms,
            record,
        })
    }

    /// A `show_screen` the runtime emits itself.
    fn runtime_screen(
        &mut self,
        now_ms: TsMs,
        screen: ScreenId,
        record_id: Option<String>,
        focus: Option<String>,
    ) -> StreamEvent {
        self.screen = screen;
        self.screen_since = now_ms;
        if record_id.is_some() {
            self.screen_record.clone_from(&record_id);
        }
        self.screen_focus.clone_from(&focus);
        StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::ShowScreen {
                screen,
                record_id,
                focus,
            },
        }
    }

    /// The hex a filed record is about.
    fn record_hex(&self, id: &str) -> Option<String> {
        let file = self.case_file.as_ref()?;
        file.records
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.hex.clone())
    }

    /// A default the investigation drives (a dispatch, a reveal, a poll, a dwell): nothing
    /// after a sign-off, for a screen the state does not allow, while the model's own
    /// `show_screen` holds ([`SCREEN_HOLD_MS`]), or when that very view is already up. A
    /// default suppressed by the hold is spent.
    fn screen_default(
        &mut self,
        now_ms: TsMs,
        screen: ScreenId,
        record_id: Option<String>,
        focus: Option<String>,
    ) -> Option<StreamEvent> {
        if self.signed_off
            || !self.table.allows_screen_in(&self.state, screen)
            || self.screen_held(now_ms)
        {
            return None;
        }
        let same_record = record_id.is_none() || record_id == self.screen_record;
        if self.screen == screen && self.screen_focus == focus && same_record {
            return None;
        }
        Some(self.runtime_screen(now_ms, screen, record_id, focus))
    }

    /// A poll opened: the board's overview, to choose the next card.
    fn poll_screen(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        let screen = self.map_screen();
        self.screen_default(now_ms, screen, None, None)
    }

    /// The dwell defaults: [`BROWSER_DWELL_MS`] on `browser` moves to `desk` (the record, and
    /// its card on the board; the overview when the desk is not allowed), and
    /// [`DESK_DWELL_MS`] on `desk` pulls back to the board's overview. Any `show_screen`
    /// restarts the clock.
    fn screen_dwell(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        let up = now_ms - self.screen_since;
        let map = self.map_screen();
        match self.screen {
            ScreenId::Browser if up >= self.config.tuning.browser_dwell_ms => {
                let record = self.screen_record.clone();
                let hex = record.as_deref().and_then(|id| self.record_hex(id));
                if record.is_some() && self.table.allows_screen_in(&self.state, ScreenId::Desk) {
                    self.screen_default(now_ms, ScreenId::Desk, record, hex)
                } else {
                    self.screen_default(now_ms, map, None, None)
                }
            }
            ScreenId::Desk if up >= self.config.tuning.desk_dwell_ms => {
                self.screen_default(now_ms, map, None, None)
            }
            _ => None,
        }
    }

    /// The default screen at `segment_start`, always sent: the new segment's allow-list is not
    /// the old one's, and the model's choice in the old segment does not carry over.
    fn screen_start(&mut self, now_ms: TsMs) -> StreamEvent {
        self.llm_screen_ms = None;
        let screen = self.table.default_screen(&self.state);
        self.runtime_screen(now_ms, screen, None, None)
    }

    /// The state's default screen, when the state just entered excludes the screen up. An
    /// allow-list is a rule, so the hold rule does not apply.
    fn screen_fit(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        if self.table.allows_screen_in(&self.state, self.screen) {
            return None;
        }
        let screen = self.table.default_screen(&self.state);
        Some(self.runtime_screen(now_ms, screen, None, None))
    }

    /// The model chose a screen within the last [`SCREEN_HOLD_MS`].
    fn screen_held(&self, now_ms: TsMs) -> bool {
        self.llm_screen_ms
            .is_some_and(|t| now_ms - t < self.config.tuning.screen_hold_ms)
    }

    /// A research job came back (the brain's `job` event with status `done`). When its
    /// `dispatch_research` was accepted and filed records, those records count as returned
    /// (only returned records may be shown), and when the browser is allowed now the runtime
    /// opens the first of them: its `record_shown` (the first time it is named), then
    /// `show_screen browser`. The screen stays put while the model's own `show_screen` holds
    /// ([`SCREEN_HOLD_MS`]) and after a sign-off. A job the runtime did not accept does
    /// nothing, and a job comes back at most once.
    pub fn job_done(&mut self, job_id: &str, now_ms: TsMs) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let Some(ids) = self.job_filed.remove(job_id) else {
            return Vec::new();
        };
        let Some(id) = ids.first().cloned() else {
            return Vec::new();
        };
        self.returned.extend(ids);
        if self.signed_off
            || !self.table.allows_screen_in(&self.state, ScreenId::Browser)
            || self.screen_held(now_ms)
        {
            return Vec::new();
        }
        let mut events: Vec<StreamEvent> = self.record_shown(now_ms, &id).into_iter().collect();
        events.push(self.runtime_screen(now_ms, ScreenId::Browser, Some(id), None));
        events
    }

    /// The state summary's screen line: what is up and the returned records the browser can
    /// open, newest first.
    fn screen_line(&self) -> String {
        let mut line = format!("Screen: {}", screen_name(self.screen));
        if let (ScreenId::Browser | ScreenId::Desk, Some(id)) = (self.screen, &self.screen_record) {
            line.push_str(&format!(" on {id}"));
        }
        match (self.screen, &self.screen_focus) {
            (ScreenId::Hexmap | ScreenId::Board, Some(hex)) => {
                line.push_str(&format!(" focused on {hex}"))
            }
            (ScreenId::Desk, Some(hex)) => line.push_str(&format!(
                ", {} focused on {hex}",
                if self.map_screen() == ScreenId::Board {
                    "board"
                } else {
                    "map"
                }
            )),
            _ => {}
        }
        let now = self.table.screens_in(&self.state);
        if now.len() < self.table.screens.len() {
            line.push_str(&format!(" (only {} now)", now.join(", ")));
        }
        // Returned records only, ids only: the research block's case-file line already carries
        // each record's hex, grade and title, and this line is paid for every beat.
        let records: Vec<&str> = self
            .returned
            .iter()
            .rev()
            .take(SCREEN_RECORDS_SHOWN)
            .map(String::as_str)
            .collect();
        if records.is_empty() {
            line.push_str(". Records on file: none yet (browser and desk need one)");
        } else {
            line.push_str(&format!(
                ". Records on file, newest first: {}",
                records.join(", ")
            ));
            let more = self.returned.len() - records.len();
            if more > 0 {
                line.push_str(&format!(" +{more} older"));
            }
        }
        line
    }

    /// The state summary's board line: cards face up, strings pinned, and the open leads (a
    /// face-up card with a returned record filed under it and no string yet), newest record
    /// first, at most [`BOARD_LEADS_SHOWN`].
    fn board_line(&self) -> String {
        let up = self.case.hexes.iter().filter(|h| !h.fog).count();
        let mut line = format!(
            "Board: {up} card{} face up, {} string{}.",
            if up == 1 { "" } else { "s" },
            self.links.len(),
            if self.links.len() == 1 { "" } else { "s" }
        );
        let strung = |id: &str| self.links.iter().any(|l| l.from == id || l.to == id);
        let mut leads: Vec<&Hex> = Vec::new();
        let file = self.case_file.as_ref();
        for rid in self.returned.iter().rev() {
            let Some(hex) = file
                .and_then(|f| f.records.iter().find(|r| &r.id == rid))
                .and_then(|r| self.case.hexes.iter().find(|h| h.id == r.hex))
            else {
                continue;
            };
            if !hex.fog && !strung(&hex.id) && !leads.iter().any(|h| h.id == hex.id) {
                leads.push(hex);
            }
        }
        if !leads.is_empty() {
            let named: Vec<String> = leads
                .iter()
                .take(BOARD_LEADS_SHOWN)
                .map(|h| format!("{} ({})", h.id, card_kind(h)))
                .collect();
            line.push_str(&format!(
                " Open leads: {} {} a returned record and no string.",
                named.join(", "),
                if named.len() == 1 { "has" } else { "have" }
            ));
        }
        line
    }

    // ----- clock -----------------------------------------------------------------------

    /// Clock: poll timeouts (a closed `vote`; a foil vote also summons its winner), the cold
    /// open and the handover rule. The cold open ends when the room fills or at its ceiling
    /// (a runtime `show_card`, `cold-open-end-room-filled` / `cold-open-end-ceiling`, then the
    /// state after it). The handover's audience path waits for it; the handover's elapsed
    /// ceiling does not, and cuts a cold open short. When the handover rule fires it
    /// emits `handover` and enters the `handover_pending` state; the brain is expected to emit
    /// `switch_segment`, and after the rule's grace period the clock switches by itself
    /// (`segment_end`, `segment_start`).
    /// A state the clock moved into that excludes the screen up gets its default screen, and
    /// a browser or desk up past its dwell gives way ([`BROWSER_DWELL_MS`], [`DESK_DWELL_MS`]).
    pub fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        let mut events = self.tick_clock(now_ms, ccv);
        if !self.signed_off {
            events.extend(self.screen_fit(now_ms));
            events.extend(self.screen_dwell(now_ms));
        }
        events
    }

    fn tick_clock(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        if self.signed_off {
            return events;
        }
        if self.poll.as_ref().is_some_and(|p| now_ms >= p.closes_ms) {
            events.extend(self.close_poll(now_ms, true));
        }
        events.extend(self.open_due_plebiscite(now_ms));
        events.extend(self.arc_tick(now_ms));
        events.extend(self.cold_open_tick(now_ms));
        events.extend(self.rounds_tick(now_ms));
        let Some(rule) = self.handover_rule() else {
            return events;
        };
        if let Some(since) = self.handover_pending_since {
            if now_ms - since >= i64::from(rule.grace_s) * 1000 {
                if let Ok(evs) = self.switch_to(now_ms, rule.to) {
                    events.extend(evs);
                }
            }
            return events;
        }
        // The audience path waits for the cold open: a room still filling is not a room that
        // held. The segment's own ceiling (`max_elapsed_s`) still applies.
        let cold = self.in_cold_open();
        if ccv >= rule.ccv_min && !cold {
            self.ccv_since.get_or_insert(now_ms);
        } else {
            self.ccv_since = None;
        }
        let started = self.started_ms.unwrap_or(now_ms);
        let sustained = self
            .ccv_since
            .is_some_and(|s| now_ms - s >= i64::from(rule.sustain_s) * 1000);
        let timed_out = now_ms - started >= i64::from(rule.max_elapsed_s) * 1000;
        if sustained || timed_out {
            let reason = if sustained {
                format!("ccv >= {} sustained {} s", rule.ccv_min, rule.sustain_s)
            } else {
                format!("elapsed >= {} s", rule.max_elapsed_s)
            };
            if cold {
                // A handover due before the room filled cuts the cold open short.
                self.cold_open_ended = Some((now_ms, format!("handover: {reason}")));
                self.chatters.clear();
                self.room_since = None;
            }
            events.push(StreamEvent::Handover {
                ts_ms: now_ms,
                from: self.table.id,
                to: rule.to,
                reason,
            });
            self.fire(Trigger::HandoverDue);
            self.handover_pending_since = Some(now_ms);
            if rule.grace_s == 0 {
                if let Ok(evs) = self.switch_to(now_ms, rule.to) {
                    events.extend(evs);
                }
            }
        }
        events
    }

    // ----- scene rounds ----------------------------------------------------------------

    /// The scene rounds' clock ([`crate::rounds`]): the intro and each free talk give way to
    /// the next round, a ballot or a scene left past the ceiling closes the segment, and a
    /// scene nobody performs ends after `scene_max_s`.
    fn rounds_tick(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let Some(r) = &self.rounds else {
            return Vec::new();
        };
        let t = r.times;
        let elapsed = self.elapsed_ms();
        let ceiling = elapsed >= i64::from(t.ceiling_s) * 1000;
        match self.state.as_str() {
            INTRO_STATE if ceiling || elapsed >= i64::from(t.intro_s) * 1000 => {
                self.next_round(now_ms)
            }
            TALK_STATE => {
                let talked = r
                    .talk_since
                    .is_none_or(|s| now_ms - s >= i64::from(t.talk_s) * 1000);
                let floor = elapsed >= i64::from(t.floor_s) * 1000;
                if ceiling || (talked && (!r.left.is_empty() || floor)) {
                    self.next_round(now_ms)
                } else {
                    Vec::new()
                }
            }
            BALLOT_STATE if ceiling => {
                let events = self.close_poll(now_ms, false);
                self.state = CLOSING_STATE.to_string();
                events
            }
            SCENE_STATE => {
                let stale =
                    r.up.as_ref()
                        .filter(|(_, since, _)| now_ms - since >= i64::from(t.scene_max_s) * 1000)
                        .map(|(i, _, _)| r.scenes[*i].id.clone());
                match stale {
                    Some(id) => {
                        self.scene_ended(&id, now_ms);
                        vec![moderation(
                            now_ms,
                            ModStage::Output,
                            SCENE_TIMEOUT_RULE,
                            None,
                            format!("ended:{id}"),
                        )]
                    }
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// The next round: a ballot over the scenes left (two or more), the last scene unvoted,
    /// or, with none left or past the ceiling, the closing.
    fn next_round(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let Some(r) = &self.rounds else {
            return Vec::new();
        };
        let ceiling = self.elapsed_ms() >= i64::from(r.times.ceiling_s) * 1000;
        match r.left.as_slice() {
            [] => {
                self.state = CLOSING_STATE.to_string();
                Vec::new()
            }
            _ if ceiling => {
                self.state = CLOSING_STATE.to_string();
                Vec::new()
            }
            [only] => {
                let only = *only;
                if let Some(r) = self.rounds.as_mut() {
                    r.cue(only, now_ms, Cue::Unvoted);
                }
                self.state = SCENE_STATE.to_string();
                Vec::new()
            }
            _ => self.open_scene_ballot(now_ms).into_iter().collect(),
        }
    }

    /// Open the ballot over the scenes left: their titles in ballot order. Its `start_poll`
    /// goes out like a host poll's, so the stage draws it unchanged. `None` while another
    /// poll is open (the ballot waits for it).
    fn open_scene_ballot(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        if self.poll.is_some() || self.signed_off {
            return None;
        }
        let host = self.host_title().to_string();
        let r = self.rounds.as_mut()?;
        r.ballots += 1;
        let want = format!("scene-ballot-{}", r.ballots);
        let options = r.left_titles();
        let guest = foil_name(&r.guest);
        let clock = &self.table.clock;
        let secs = r
            .times
            .ballot_secs
            .clamp(clock.poll_min_secs, clock.poll_max_secs);
        let id = self.fresh_poll_id(&want);
        let question = format!("Which scene shall the {host} and {guest} perform next?");
        self.open_poll(Poll {
            id: id.clone(),
            question: question.clone(),
            options: options.clone(),
            min_rank: Rank::Thrall,
            closes_ms: now_ms + i64::from(secs) * 1000,
            kind: PollKind::Scene,
            subject: None,
            votes: BTreeMap::new(),
        });
        self.state = BALLOT_STATE.to_string();
        Some(StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::StartPoll {
                poll_id: id,
                question,
                options,
                min_rank: Rank::Thrall,
                secs,
            },
        })
    }

    /// The brain performed the scene up to its last line (its `scene_end` event): the scene
    /// leaves the ballot and free talk starts. A scene that is not the one up does nothing.
    /// Returns the events it causes (none today; kept like [`Self::job_done`]).
    pub fn scene_ended(&mut self, scene_id: &str, now_ms: TsMs) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let ended = self
            .rounds
            .as_mut()
            .is_some_and(|r| r.ended(scene_id, now_ms));
        if ended && self.state == SCENE_STATE {
            self.state = TALK_STATE.to_string();
        }
        Vec::new()
    }

    /// The scene the segment is performing or about to: its id, while one is up.
    pub fn scene_up(&self) -> Option<&str> {
        let r = self.rounds.as_ref()?;
        r.up.as_ref().map(|(i, _, _)| r.scenes[*i].id.as_str())
    }

    /// Scenes played so far, by id, in order.
    pub fn scenes_played(&self) -> Vec<&str> {
        self.rounds.as_ref().map_or_else(Vec::new, |r| {
            r.played.iter().map(|&i| r.scenes[i].id.as_str()).collect()
        })
    }

    /// The state summary's Table Read block: the round, the scene up, the free talk's
    /// material, the closing.
    fn round_lines(&self) -> Vec<String> {
        let Some(r) = &self.rounds else {
            return Vec::new();
        };
        let (k, n) = r.round();
        let mut lines = vec![format!(
            "TABLE READ with {}: round {k} of {n}; scenes left: {}; played: {}",
            r.guest,
            r.quoted(&r.left),
            r.quoted(&r.played)
        )];
        // How the scene up was chosen, while it is up: the room event the host announces
        // before it starts.
        if let (Some((id, result, i)), SCENE_STATE) = (&r.last_ballot, self.state.as_str()) {
            let chosen = r.title(*i);
            lines.push(match result {
                PollOutcome::Winner(w) => format!("last poll {id} closed: winner {w}"),
                PollOutcome::Tie { options, voters } => format!(
                    "last poll {id} closed: TIE between {} ({voters} votes each): you break it \
                     on air, and say you are breaking it, in favour of \"{chosen}\" (first in \
                     the running order), which plays next",
                    PollOutcome::tied(options)
                ),
                PollOutcome::NoVotes => format!(
                    "last poll {id} closed: no votes: you choose \"{chosen}\" (first in the \
                     running order), which plays next"
                ),
            });
        }
        let now = self.last_now_ms;
        match self.state.as_str() {
            SCENE_STATE => {
                if let Some((i, _, how)) = &r.up {
                    let s = &r.scenes[*i];
                    let how = match how {
                        Cue::Unvoted => "the last scene, unvoted",
                        Cue::Ballot { .. } => "chosen by the ballot",
                    };
                    lines.push(format!(
                        "{SCENE_UP_PREFIX}{} \"{}\" ({how}): it is performed now, its lines \
                         scripted for you and {} and played by themselves; add nothing to it",
                        s.id, s.title, r.guest
                    ));
                }
            }
            TALK_STATE => {
                if let Some(&i) = r.played.last() {
                    let s = &r.scenes[i];
                    let left = r.talk_since.map_or(0, |t| {
                        (i64::from(r.times.talk_s) * 1000 - (now - t)).max(0) / 1000
                    });
                    let points = if s.notes.is_empty() {
                        "none written".to_string()
                    } else {
                        s.notes.join("; ")
                    };
                    let next = match r.left.len() {
                        0 => "That was the last scene: the closing follows.",
                        1 => "Then the last scene plays, unvoted.",
                        _ => "Then the next ballot opens by itself.",
                    };
                    lines.push(format!(
                        "FREE TALK after \"{}\" (about {left} s left): talking points: \
                         {points}. {next}",
                        s.title
                    ));
                }
            }
            CLOSING_STATE => lines.push(format!(
                "The Table Read is over: thank {}, give your honest verdict, then sign_off.",
                r.guest
            )),
            _ => {}
        }
        lines
    }

    // ----- context ---------------------------------------------------------------------

    /// What the brain sees this tick. `rules_prompt` is stable within a segment (table text
    /// plus config facts); `state_summary` carries the live state. Untrusted chat text
    /// (leads, proposals) is summarised as counts only, and the current state's table line
    /// (`[state_rules.<state>] prompt`) leads the summary. `allowed_ops` is the gate's view:
    /// the table's ops that [`Self::availability`] admits now (none once signed off); each
    /// withheld op is named with its reason on the summary's `not now:` line.
    pub fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        SegmentContext {
            segment: self.table.id,
            rules_prompt: self.rules_prompt(),
            state_summary: self.state_summary(),
            state: self.state.clone(),
            allowed_ops: self
                .table
                .allowed_ops
                .iter()
                .filter(|o| self.availability(o).is_ok())
                .cloned()
                .collect(),
            ccv,
            elapsed_s,
            hexes: if self.table.id == SegmentId::Seg02aHexmap {
                self.case().hexes.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// The character's words this runtime fills its prompts from.
    pub fn persona(&self) -> &Persona {
        &self.config.persona
    }

    /// A persona template filled with the character's words.
    fn fill(&self, template: &str) -> String {
        self.config.persona.fill(template)
    }

    /// How a poll question names the host: the title ("the Arch-Lich"), else the name.
    fn host_title(&self) -> &str {
        let p = &self.config.persona;
        if p.title.is_empty() {
            &p.name
        } else {
            &p.title
        }
    }

    /// The package's flavour for this segment, if any.
    fn flavour(&self) -> Option<&SegmentFlavour> {
        self.config.flavour.get(&self.table.id)
    }

    fn rules_prompt(&self) -> String {
        let t = self.table;
        let mut p = self.fill(t.rules_prompt.trim_end());
        if let Some(extra) = self.flavour().and_then(|f| f.rules_prompt.as_deref()) {
            let extra = self.fill(extra.trim());
            if !extra.is_empty() {
                p.push('\n');
                p.push_str(&extra);
            }
        }
        if t.allows_op("summon_foil") || t.allows_command("summon") {
            if self.config.foils.is_empty() {
                p.push_str("\nNo foils are available tonight.");
            } else {
                p.push_str(&format!(
                    "\nFoils you may summon: {}.",
                    self.config.foils.join(", ")
                ));
            }
        }
        if let (Some(rule), Some(tr)) = (self.handover_rule(), &self.config.table_read) {
            if t.clock.handover.is_some_and(|h| h.to != rule.to) {
                let title = segment_table(rule.to).map_or("", |s| s.title.as_str());
                p.push_str(&format!(
                    "\nTonight the handover goes to {} {title}, with {} as the guest.",
                    segment_name(rule.to),
                    tr.guest
                ));
            }
        }
        if let Some(r) = &self.rounds {
            let order: Vec<String> = r
                .scenes
                .iter()
                .map(|s| format!("\"{}\" ({})", s.title, s.pitch))
                .collect();
            p.push_str(&format!(
                "\nTonight's guest: {}. Tonight's scenes, in running order: {}.",
                r.guest,
                order.join("; ")
            ));
        }
        if t.allows_op("reveal_hex") {
            p.push_str(&format!(
                "\nCase: {} ({}), {} hexes.",
                self.case.title,
                self.case.id,
                self.case.hexes.len()
            ));
            if self.case.fictional {
                p.push_str(
                    " THIS CASE IS FICTIONAL DEMO DATA: say so when asked and never present \
                     it as a real investigation.",
                );
            } else {
                p.push_str(
                    " THIS IS A REAL CASE FROM THE PUBLIC RECORD. Name only the companies, \
                     schemes and regulators on the map. People named in the records are \
                     spoken of by role only (\"the founder\", \"an officer\", \"a sales \
                     agent\"), never by name, and never mocked for anything outside the record. \
                     A DOC record documents what a regulator or a court filed, alleged, found or \
                     ordered: a complaint's charges are the regulator's allegations (\"the SEC \
                     alleged\") until an order, judgment or plea settles them. Victims of the \
                     scheme are never the joke.",
                );
            }
            if let Some(arc) = &self.arc {
                let acts: Vec<String> = arc
                    .acts
                    .iter()
                    .enumerate()
                    .map(|(k, title)| {
                        format!(
                            "Act {} {title} (from minute {})",
                            roman(k + 1),
                            minutes(arc.opens_ms(k))
                        )
                    })
                    .collect();
                p.push_str(&format!(
                    "\nArc: this investigation runs {} minutes in {} acts: {}. Hexes staged for \
                     a later act stay locked (no reveal, no research) until their act opens. \
                     When an act's map is done early, deepen it: jury polls, the {}' \
                     leads, the records on file, a recap for newcomers. The last {} minutes are \
                     the seal window: recap the dossier (every revealed hex with its grade, \
                     alleged below DOC), then sign_off. sign_off is refused before it, and the \
                     investigation is the whole segment: no switch_segment.",
                    minutes(arc.planned_ms),
                    arc.acts.len(),
                    acts.join("; "),
                    self.config.persona.followers,
                    minutes(arc.planned_ms - arc.seal_ms)
                ));
                if arc.pacing {
                    let slots: Vec<String> = (0..arc.acts.len())
                        .map(|k| {
                            format!(
                                "act {} {} hexes, one every ~{} min",
                                roman(k + 1),
                                arc.staged[k],
                                round_minutes(arc.slot_ms(k))
                            )
                        })
                        .collect();
                    p.push_str(&format!(
                        "\nReveal pacing: each act's reveals are spread across the act, one reveal \
                         slot per hex staged for it ({}). A reveal before its slot is refused \
                         (reveal_not_due); the state summary says when the next one unlocks. Do \
                         not burn an act's map in its first minutes.",
                        slots.join("; ")
                    ));
                }
                if arc.needs_work {
                    p.push_str(&self.fill(
                        " A hex is revealed only after a beat of work on it: a {researcher} search \
                         (dispatch_research naming it) or a closed crowd poll whose winning option \
                         names it (reveal_needs_work).",
                    ));
                }
                if arc.pacing {
                    p.push_str(
                        " Between reveals, make the wait count: search the next candidate hex, \
                         poll the crowd on which hex to crack next (or call for !hint \
                         nominations), read a record already on file aloud (quote it; alleged \
                         below DOC), and work the chat.",
                    );
                    if arc.recap_cooldown_s > 0 {
                        p.push_str(&format!(
                            " Do not loop the act recap: at most one recap every {} minutes.",
                            round_minutes(i64::from(arc.recap_cooldown_s) * 1000)
                        ));
                    }
                }
            }
        }
        if t.allows_op("dispatch_research") {
            p.push_str(&format!(
                "\nResearch: name exactly one hex in every dispatch_research brief (\"HEX-04: \
                 trace the treasury wallet\"). The {} search a hex at most {} times \
                 per stream; after that its next move goes to the Plebiscite, which opens by \
                 itself. Never re-dispatch a spent hex: annex it with the evidence you hold, \
                 or let the vote decide.",
                self.config.persona.researchers, t.clock.max_searches_per_hex
            ));
            match &self.case_file {
                Some(f) if f.fictional => p.push_str(
                    " The case file holds planted FICTIONAL demo records: quote them as the \
                     demo's records, never as real findings.",
                ),
                Some(_) => p.push_str(&self.fill(
                    " The {researchers} read the case's public records, staged off-stream from \
                     regulator and court sites: quote a record at its grade and never as more \
                     than it says. A hex with nothing on file stays unproven.",
                )),
                None => {}
            }
        }
        p
    }

    /// The research block of the state summary: searches per hex against the cap, what the
    /// case file has yielded, spent hexes and the Plebiscite. Planted records are fixed data,
    /// never chat, so their titles may enter the prompt.
    fn research_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.searches.is_empty() {
            return lines;
        }
        let cap = self.table.clock.max_searches_per_hex;
        let counts: Vec<String> = self
            .searches
            .iter()
            .map(|(h, n)| {
                let spent = if *n >= cap { " SPENT" } else { "" };
                format!("{h} {n}/{cap}{spent}")
            })
            .collect();
        lines.push(format!(
            "{} searches: {}",
            self.config.persona.researcher,
            counts.join(", ")
        ));
        if let Some(file) = &self.case_file {
            let found: Vec<String> = self
                .filed
                .iter()
                .filter_map(|id| file.records.iter().find(|r| &r.id == id))
                .map(|r| format!("{} {} {}: {}", r.hex, r.id, grade_name(r.grade), r.title))
                .collect();
            if !found.is_empty() {
                lines.push(format!(
                    "case file ({}): {}",
                    if file.fictional {
                        "planted FICTIONAL demo records"
                    } else {
                        "public records"
                    },
                    found.join("; ")
                ));
            }
            let empty: Vec<&str> = self
                .searches
                .keys()
                .filter(|h| file.records_for(h).next().is_none())
                .map(String::as_str)
                .collect();
            if !empty.is_empty() {
                lines.push(format!("searched, nothing on file: {}", empty.join(", ")));
            }
        }
        let spent: Vec<&str> = self
            .searches
            .iter()
            .filter(|(_, n)| **n >= cap)
            .map(|(h, _)| h.as_str())
            .collect();
        if !spent.is_empty() {
            lines.push(format!(
                "SPENT: {} searched {cap} times. Do NOT dispatch_research on them again; annex \
                 with the evidence you hold, or leave them fogged.",
                spent.join(", ")
            ));
        }
        if let Some(hex) = self.plebiscite_hex() {
            lines.push(format!(
                "PLEBISCITE on {hex}: the audience decides its next move. No research until \
                 it closes; campaign for your favourite option."
            ));
        } else if let Some((hex, verdict)) = &self.last_plebiscite {
            lines.push(match verdict {
                PollOutcome::Winner(option) => format!(
                    "PLEBISCITE on {hex} decided: \"{option}\". Carry it out; do not search \
                     {hex} again."
                ),
                PollOutcome::Tie { options, voters } => format!(
                    "PLEBISCITE on {hex} closed: TIE between {} ({voters} votes each): you \
                     break it, and say you are breaking it. Do not search {hex} again.",
                    PollOutcome::tied(options)
                ),
                PollOutcome::NoVotes => format!(
                    "PLEBISCITE on {hex} drew no votes: your call, but {hex} is not searched \
                     again."
                ),
            });
        }
        lines
    }

    /// The arc block of the state summary: the act, its hexes, what is locked, and what to do.
    fn arc_lines(&self) -> Vec<String> {
        let Some(arc) = &self.arc else {
            return Vec::new();
        };
        let elapsed = self.elapsed_ms();
        let act = arc.act_at(elapsed);
        let n = arc.acts.len();
        let mut lines = vec![format!(
            "ARC act {}/{n} \"{}\" (minutes {}-{}), minute {} now",
            act + 1,
            arc.acts[act],
            minutes(arc.opens_ms(act)),
            minutes(if act + 1 < n {
                arc.opens_ms(act + 1)
            } else {
                arc.planned_ms
            }),
            minutes(elapsed)
        )];
        let ids = |pred: &dyn Fn(usize, &shadowrunr_types::Hex) -> bool| -> Vec<&str> {
            self.case
                .hexes
                .iter()
                .enumerate()
                .filter(|(i, h)| pred(*i, h))
                .map(|(_, h)| h.id.as_str())
                .collect()
        };
        let open_fog = ids(&|i, h| h.fog && arc.act_of[i] <= act);
        let act_fog = ids(&|i, h| h.fog && arc.act_of[i] == act);
        lines.push(format!(
            "fogged and open: {}",
            if open_fog.is_empty() {
                "none".to_string()
            } else {
                open_fog.join(", ")
            }
        ));
        if act + 1 < n {
            let locked = ids(&|i, _| arc.act_of[i] > act);
            lines.push(format!(
                "LOCKED until act {} opens at minute {}: {}",
                act + 2,
                minutes(arc.opens_ms(act + 1)),
                locked.join(", ")
            ));
            if act_fog.is_empty() {
                let recap = if arc.recap_cooldown_s > 0 {
                    format!(
                        "one short recap for newcomers at most every {} minutes",
                        round_minutes(i64::from(arc.recap_cooldown_s) * 1000)
                    )
                } else {
                    "a recap for newcomers".to_string()
                };
                lines.push(format!(
                    "Act {} map is complete: until act {} opens, deepen it (a jury poll on a \
                     disputed hex, the {}' leads, the records on file, {recap}). Do not \
                     stall and do not sign off.",
                    act + 1,
                    act + 2,
                    self.config.persona.followers
                ));
            }
        } else if arc.sealing(elapsed) {
            lines.push(self.fill(
                "SEAL WINDOW: recap the dossier now (every revealed hex with its grade, alleged \
                 below DOC), thank the {followers}, then sign_off.",
            ));
        } else {
            lines.push(format!(
                "Final act: the seal window opens at minute {}; sign_off is refused before it.",
                minutes(arc.seal_ms)
            ));
        }
        if !arc.sealing(elapsed) && !open_fog.is_empty() {
            lines.extend(self.pace_lines(arc, act, elapsed, &open_fog));
        }
        lines
    }

    /// The reveal pacing block: when the next reveal unlocks, which hexes have had their beat
    /// of work, and what to do with the wait.
    fn pace_lines(
        &self,
        arc: &ArcPlan,
        act: usize,
        elapsed: TsMs,
        open_fog: &[&str],
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let next = self.next_reveal_ms();
        let waiting = next.is_some_and(|t| elapsed < t);
        if let Some(t) = next {
            let head = format!(
                "REVEAL PACE: act {} has {} hexes, one reveal slot every ~{} min",
                act + 1,
                arc.staged[act],
                round_minutes(arc.slot_ms(act))
            );
            lines.push(if waiting {
                format!(
                    "{head}. The next reveal unlocks at minute {} (in {} s); reveal_hex before \
                     it is refused (reveal_not_due).",
                    minutes(t),
                    (t - elapsed + 999) / 1000
                )
            } else {
                format!("{head}. A reveal slot is OPEN now: reveal one worked hex.")
            });
        }
        let (worked, unworked): (Vec<&str>, Vec<&str>) =
            open_fog.iter().copied().partition(|h| self.hex_worked(h));
        if arc.needs_work {
            if !worked.is_empty() {
                lines.push(format!(
                    "ready to reveal (searched or voted): {}",
                    worked.join(", ")
                ));
            }
            if !unworked.is_empty() {
                lines.push(format!(
                    "needs a beat of work before its reveal (a {} search on it, or a \
                     closed crowd poll that picks it): {}",
                    self.config.persona.researcher,
                    unworked.join(", ")
                ));
            }
        }
        if waiting {
            let candidate = unworked
                .first()
                .or(open_fog.first())
                .copied()
                .unwrap_or_default();
            let recap = if arc.recap_cooldown_s > 0 {
                format!(
                    " Do NOT repeat the act recap: at most one recap every {} minutes.",
                    round_minutes(i64::from(arc.recap_cooldown_s) * 1000)
                )
            } else {
                String::new()
            };
            lines.push(format!(
                "BETWEEN REVEALS make the wait count: dispatch_research on the next candidate \
                 ({candidate}), open a poll on which hex to crack next (or call for !hint \
                 nominations), read a record already on file aloud (quote it; alleged below \
                 DOC), and play with the chat.{recap}"
            ));
        }
        lines
    }

    fn state_summary(&self) -> String {
        let mut lines = vec![format!(
            "segment {} ({}), state {}",
            segment_name(self.table.id),
            self.table.title,
            self.state
        )];
        if let Some(line) = self.table.state_prompt(&self.state) {
            let mut line = self.fill(line);
            if let Some(extra) = self.flavour().and_then(|f| f.states.get(&self.state)) {
                let extra = self.fill(extra.trim());
                if !extra.is_empty() {
                    line.push(' ');
                    line.push_str(&extra);
                }
            }
            lines.push(line);
        } else if let Some(extra) = self.flavour().and_then(|f| f.states.get(&self.state)) {
            lines.push(self.fill(extra.trim()));
        }
        if self.signed_off {
            lines.push("signed off: the show is over".into());
        }
        if let Some(f) = &self.active_foil {
            lines.push(format!("active foil: {f}"));
        }
        if let Some(f) = &self.observer_foil {
            lines.push(format!("skeptical observer: {f}"));
        }
        match &self.poll {
            Some(p) => lines.push(p.summary(self.last_now_ms)),
            None => lines.push(
                "no poll open: never tell chat a poll is open or counting until start_poll is accepted".into(),
            ),
        }
        if let Some((id, result)) = &self.last_crowd {
            lines.push(match result {
                PollOutcome::Winner(w) => format!("last poll {id} closed: winner {w}"),
                PollOutcome::Tie { options, voters } => {
                    let hexes: Vec<&str> = options
                        .iter()
                        .filter_map(|o| target_hex(&self.case, o))
                        .map(|i| self.case.hexes[i].id.as_str())
                        .collect();
                    let voted = if hexes.is_empty() || !self.table.allows_op("reveal_hex") {
                        String::new()
                    } else {
                        format!(" (each tied hex counts as voted: {})", hexes.join(", "))
                    };
                    format!(
                        "last poll {id} closed: TIE between {} ({voters} votes each): you break \
                         it, and say you are breaking it{voted}",
                        PollOutcome::tied(options)
                    )
                }
                PollOutcome::NoVotes => format!("last poll {id} closed: no votes, no winner"),
            });
        }
        if let (Some((id, foils)), None) = (&self.foil_tie, &self.active_foil) {
            lines.push(format!(
                "foil vote {id} closed: TIE between {}: nobody was summoned. You break it with \
                 summon_foil, and say you are breaking it",
                PollOutcome::tied(foils)
            ));
        }
        let now = self.last_now_ms;
        let refused: Vec<&str> = self
            .refused
            .iter()
            .filter(|(t, _)| now - t < REFUSED_SHOWN_MS)
            .map(|(_, r)| r.as_str())
            .collect();
        if !refused.is_empty() {
            lines.push(format!(
                "REFUSED lately, so these did NOT happen (never tell chat they did): {}",
                refused.join("; ")
            ));
        }
        // Chat's declined commands, counted per command and rule (never the chat text), so
        // the host can acknowledge in-world that a command is closed rather than broken.
        let mut declined: Vec<((&str, &str), u32)> = Vec::new();
        for (_, cmd, rule) in self
            .declined
            .iter()
            .filter(|(t, _, _)| now - t < REFUSED_SHOWN_MS)
        {
            match declined.iter_mut().find(|(k, _)| *k == (*cmd, *rule)) {
                Some((_, n)) => *n += 1,
                None => declined.push(((*cmd, *rule), 1)),
            }
        }
        if !declined.is_empty() && !self.signed_off {
            let parts: Vec<String> = declined
                .iter()
                .map(|((cmd, rule), n)| format!("!{cmd} x{n} ({})", decline_reason(rule)))
                .collect();
            lines.push(format!("declined lately: {}", parts.join(", ")));
        }
        // The ops withheld from the menu, with the gate's reason: the brain drops an op off
        // its menu itself, so the refusal is named here instead.
        if !self.signed_off {
            let not_now: Vec<String> = self
                .table
                .allowed_ops
                .iter()
                .filter_map(|o| self.availability(o).err().map(|r| format!("{o} ({r})")))
                .collect();
            if !not_now.is_empty() {
                lines.push(format!("not now: {}", not_now.join(", ")));
            }
        }
        if self.handover_pending_since.is_some() {
            if let Some(rule) = self.handover_rule() {
                let lead = match segment_table(rule.to) {
                    Ok(t) if t.plays_scenes() => {
                        format!("announce the {} with tonight's guest", t.title)
                    }
                    _ => "let the foil dare you".to_string(),
                };
                lines.push(format!(
                    "HANDOVER DUE: {lead}, then emit switch_segment to {}",
                    segment_name(rule.to)
                ));
            }
        }
        lines.extend(self.round_lines());
        if self.table.allows_op("reveal_hex") {
            let revealed: Vec<String> = self
                .case
                .hexes
                .iter()
                .filter(|h| !h.fog)
                .map(|h| {
                    let grade = h.grade.map_or("?", grade_name);
                    let alleged = if h.grade.is_some_and(crate::requires_alleged) {
                        " ALLEGED"
                    } else {
                        ""
                    };
                    let jury = self
                        .jury_grade(&h.id)
                        .map(|g| format!(", jury {}", grade_name(g)))
                        .unwrap_or_default();
                    format!("{} {grade}{alleged}{jury}: {}", h.id, h.label)
                })
                .collect();
            let fogged: Vec<&str> = self
                .case
                .hexes
                .iter()
                .filter(|h| h.fog)
                .map(|h| h.id.as_str())
                .collect();
            lines.push(format!(
                "revealed {}/{}: {}",
                revealed.len(),
                self.case.hexes.len(),
                if revealed.is_empty() {
                    "none".to_string()
                } else {
                    revealed.join("; ")
                }
            ));
            match &self.arc {
                None => lines.push(format!("fogged: {}", fogged.join(", "))),
                Some(_) => lines.extend(self.arc_lines()),
            }
            let mut leads: Vec<(usize, &String)> =
                self.leads.iter().map(|(h, m)| (m.len(), h)).collect();
            leads.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            if !leads.is_empty() {
                let top: Vec<String> = leads
                    .iter()
                    .take(5)
                    .map(|(n, h)| format!("{h} x{n}"))
                    .collect();
                lines.push(format!(
                    "{} leads: {}",
                    self.config.persona.follower,
                    top.join(", ")
                ));
            }
            if !self.jobs.is_empty() {
                lines.push(format!("research jobs dispatched: {}", self.jobs.len()));
            }
            if self.table.allows_op("dispatch_research") {
                lines.extend(self.research_lines());
            }
        }
        if self.table.allows_op("show_screen") && !self.signed_off {
            lines.push(self.screen_line());
        }
        if self.table.allows_op("pin_link") && !self.signed_off {
            lines.push(self.board_line());
        }
        if !self.proposals.is_empty() {
            lines.push(format!("proposals waiting: {}", self.proposals.len()));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::SEGMENT_TABLE_SOURCES;

    /// SEG-02A with its jury poll narrowed to the board and the room: no table ships a state
    /// that excludes a screen the segment allows, so the test leaks one.
    fn narrowed() -> SegmentRuntime {
        narrowed_to("[\"board\", \"host\"]")
    }

    /// SEG-02A whose jury poll shows only the screens `list` (a TOML array) names.
    fn narrowed_to(list: &str) -> SegmentRuntime {
        let src = format!(
            "{}\n[state_rules.jury_poll]\nscreens = {list}\n",
            SEGMENT_TABLE_SOURCES[1].1
        );
        let table = SegmentTable::from_toml_str("narrowed", &src).unwrap();
        let mut rt = SegmentRuntime::new(SegmentId::Seg02aHexmap);
        rt.table = Box::leak(Box::new(table));
        rt.start(0);
        rt
    }

    fn poll() -> Op {
        Op::StartPoll {
            poll_id: "p1".into(),
            question: "Which hex next?".into(),
            options: vec!["HEX-01".into(), "HEX-02".into()],
            min_rank: Rank::Thrall,
            secs: 60,
        }
    }

    fn screen(screen: ScreenId, record_id: Option<&str>) -> Op {
        Op::ShowScreen {
            screen,
            record_id: record_id.map(str::to_string),
            focus: None,
        }
    }

    fn refused(evs: &[StreamEvent]) -> Vec<&str> {
        evs.iter()
            .filter_map(|e| match e {
                StreamEvent::Moderation { rule, .. } => Some(rule.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_state_that_excludes_the_screen_gets_its_default_despite_the_hold() {
        let mut rt = narrowed();
        rt.apply_ops(
            0,
            &[Op::DispatchResearch {
                job_id: "r1".into(),
                brief: "HEX-04: trace the treasury wallet".into(),
            }],
        );
        // The job comes back: the runtime opens DOC-03, and the model shows it again.
        assert!(!rt.job_done("r1", 500).is_empty());
        rt.apply_ops(1_000, &[screen(ScreenId::Browser, Some("DOC-03"))]);
        assert_eq!(rt.screen(), ScreenId::Browser);
        // The poll moves the segment into jury_poll, which has no browser: the board goes up
        // right after the op, inside the model's hold (an allow-list is a rule).
        let evs = rt.apply_ops(2_000, &[poll()]);
        let last = evs.last().unwrap();
        assert!(
            matches!(
                last,
                StreamEvent::Op {
                    op: Op::ShowScreen {
                        screen: ScreenId::Board,
                        record_id: None,
                        focus: None
                    },
                    ..
                }
            ),
            "{evs:?}"
        );
        assert_eq!(rt.state(), "jury_poll");
        assert_eq!(rt.screen(), ScreenId::Board);
        assert!(rt
            .context(0, 2)
            .state_summary
            .contains("Screen: board (only board, host now)"));
    }

    #[test]
    fn a_screen_outside_the_states_list_is_refused() {
        let mut rt = narrowed();
        rt.apply_ops(
            0,
            &[Op::DispatchResearch {
                job_id: "r1".into(),
                brief: "HEX-04: trace the treasury wallet".into(),
            }],
        );
        rt.apply_ops(1_000, &[poll()]);
        let evs = rt.apply_ops(2_000, &[screen(ScreenId::Desk, Some("DOC-03"))]);
        assert_eq!(refused(&evs), ["screen_not_allowed"]);
        assert!(!evs
            .iter()
            .any(|e| matches!(e, StreamEvent::RecordShown { .. })));
        assert!(refused(&rt.apply_ops(2_000, &[screen(ScreenId::Host, None)])).is_empty());
        // The job comes back while the browser is not allowed: no default.
        assert!(rt.job_done("r1", 30_000).is_empty());
        assert_eq!(rt.screen(), ScreenId::Host);
    }

    #[test]
    fn a_pin_while_the_board_is_not_allowed_is_refused() {
        let mut rt = narrowed_to("[\"host\", \"browser\"]");
        rt.apply_ops(
            0,
            &[Op::DispatchResearch {
                job_id: "r1".into(),
                brief: "HEX-04: trace the treasury wallet".into(),
            }],
        );
        rt.job_done("r1", 500);
        for hex in ["HEX-04", "HEX-09"] {
            rt.apply_ops(
                1_000,
                &[Op::RevealHex {
                    hex: hex.into(),
                    grade: EvidenceGrade::Clm,
                }],
            );
        }
        let pin = Op::PinLink {
            from: "HEX-04".into(),
            to: "HEX-09".into(),
            rel: LinkRel::Paid,
            record_id: "CLM-02".into(),
        };
        rt.apply_ops(2_000, &[poll()]);
        assert_eq!(rt.state(), "jury_poll");
        let evs = rt.apply_ops(3_000, std::slice::from_ref(&pin));
        assert_eq!(refused(&evs), ["screen_not_allowed"]);
        assert!(rt.links().is_empty());
        // The poll closes: the board is allowed again, and so is the same string.
        rt.apply_ops(
            4_000,
            &[Op::ClosePoll {
                poll_id: "p1".into(),
            }],
        );
        assert!(refused(&rt.apply_ops(5_000, &[pin])).is_empty());
        assert_eq!(rt.links().len(), 1);
    }
}
