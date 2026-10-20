//! `SegmentRuntime`: drives the active segment's table from chat, the brain's ops and the
//! clock. Deterministic given its inputs: time only ever comes from `now_ms` arguments, and
//! every collection that affects output order is ordered.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use serde::Serialize;
use shadowrunr_types::{
    Case, ChatMsg, Command, EvidenceGrade, HexKind, ModStage, Op, Rank, SegmentContext, SegmentId,
    StreamEvent, TsMs, VoteTally,
};

use crate::case::{demo_case, validate_case};
use crate::cases::{StagedCase, StreamKind};
use crate::command::{normalize_hex, parse_command};
use crate::evidence::{ResearchAllowlist, SourceAllowlist};
use crate::ranks::RankTable;
use crate::research::{target_hex, CaseFile};
use crate::table::{
    op_tag, segment_name, segment_table, ArcRules, GuardFacts, HandoverRule, SegmentTable, Trigger,
};
use crate::SegmentError;

/// How many chat message ids the runtime remembers for `highlight` validation.
const SEEN_MSG_CAP: usize = 10_000;

/// How long a refused op stays named in the state summary.
const REFUSED_SHOWN_MS: TsMs = 60_000;

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
    /// Segment start time. `None`: the first `now_ms` the runtime sees.
    pub start_ms: Option<TsMs>,
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
    /// The arc for `case`, when the table has one and the plan is long enough. Acts come from
    /// the case file's `arc` (research staged them); a hex it does not name, or every hex of a
    /// case without one, is dealt into the acts in kind order (entities and claims first,
    /// then domains, shells and wallets), an even share each.
    fn new(rules: &ArcRules, planned_s: u32, case: &Case, file: Option<&CaseFile>) -> Option<Self> {
        if planned_s < rules.min_planned_s || rules.acts.len() < 2 {
            return None;
        }
        let n = rules.acts.len();
        let mut act_of: Vec<Option<usize>> = vec![None; case.hexes.len()];
        if let Some(f) = file {
            for (k, ids) in f.arc.iter().enumerate() {
                for id in ids {
                    if let Some(i) = case.hexes.iter().position(|h| &h.id == id) {
                        act_of[i] = Some(k.min(n - 1));
                    }
                }
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
        Some(Self {
            acts: rules.acts.clone(),
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
        };
        format!(
            "open {kind} {}: \"{}\" | {tallies} | closes in {left} s | min rank {:?}",
            self.id, self.question, self.min_rank
        )
    }
}

/// Highest weight wins; ties go to more voters, then to the earlier option. No votes, no
/// winner.
fn winner(tallies: &[VoteTally]) -> Option<String> {
    let mut best: Option<&VoteTally> = None;
    for t in tallies.iter().filter(|t| t.voters > 0) {
        let better = best.is_none_or(|b| {
            t.weight > b.weight + 1e-9
                || ((t.weight - b.weight).abs() <= 1e-9 && t.voters > b.voters)
        });
        if better {
            best = Some(t);
        }
    }
    best.map(|t| t.option.clone())
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

fn grade_name(g: EvidenceGrade) -> &'static str {
    match g {
        EvidenceGrade::Doc => "DOC",
        EvidenceGrade::Rep => "REP",
        EvidenceGrade::Clm => "CLM",
        EvidenceGrade::Spc => "SPC",
    }
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
    /// The last crowd poll closed: its id and winner (`None` on a tie or no votes).
    last_crowd: Option<(String, Option<String>)>,
    /// Ops refused lately, newest last: (when, "close_poll p3: no_such_poll"). The state
    /// summary names them so the model never tells chat a refused op happened: the 3 h case
    /// run (2026-10-20) narrated polls as open after 16 refused start_polls.
    refused: Vec<(TsMs, String)>,
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
    /// The last closed Plebiscite: (hex, winning option; `None` when nobody voted).
    last_plebiscite: Option<(String, Option<String>)>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    signed_off: bool,
    /// The multi-act arc, when the stream is planned long enough (SEG-02A).
    arc: Option<ArcPlan>,
    /// Elapsed ms of every fog-lifting reveal under the arc, in order (the reveal pacing).
    reveals: Vec<TsMs>,
    /// Hexes a closed crowd vote chose (a poll's winner naming it, or an "Annex" Plebiscite).
    voted: BTreeSet<String>,
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
        Ok(Self {
            started_ms: config.start_ms,
            config,
            table,
            state: table.initial.clone(),
            case,
            last_now_ms: 0,
            poll: None,
            used_poll_ids: BTreeSet::new(),
            last_crowd: None,
            refused: Vec::new(),
            foil_vote_seq: 0,
            active_foil: None,
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
        })
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
    /// in SEG-02A by `case_open` carrying the case being audited.
    /// Optional: without it the start time is the first `now_ms` seen.
    pub fn start(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        self.started_ms = Some(now_ms);
        self.last_now_ms = now_ms;
        let mut events = vec![StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment: self.table.id,
        }];
        events.extend(self.case_open(now_ms, self.table.id));
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

    /// The jury's leading grade for a hex: advice for the brain, never proof.
    pub fn jury_grade(&self, hex: &str) -> Option<EvidenceGrade> {
        let w = winner(&self.jury_tally(hex))?;
        GRADES.iter().copied().find(|g| grade_name(*g) == w)
    }

    pub fn is_signed_off(&self) -> bool {
        self.signed_off
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

    // ----- internals -------------------------------------------------------------------

    /// Milliseconds since the segment started, at the latest time seen.
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

    fn handover_rule(&self) -> Option<HandoverRule> {
        self.table
            .clock
            .handover
            .map(|table_rule| self.config.handover.unwrap_or(table_rule))
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
                self.last_crowd = Some((poll.id.clone(), winner(&poll.tallies())));
                // A crowd vote whose winner names one hex counts as that hex's beat of work.
                if resolve {
                    if let Some(i) =
                        winner(&poll.tallies()).and_then(|w| target_hex(&self.case, &w))
                    {
                        self.voted.insert(self.case.hexes[i].id.clone());
                    }
                }
                self.fire(Trigger::PollClosed);
            }
            PollKind::Plebiscite => {
                if resolve {
                    if let Some(hex) = poll.subject.clone() {
                        let won = winner(&poll.tallies());
                        if won.as_deref().is_some_and(|w| w.starts_with("Annex ")) {
                            self.voted.insert(hex.clone());
                        }
                        self.last_plebiscite = Some((hex, won));
                    }
                }
                self.fire(Trigger::PollClosed);
            }
            PollKind::Foil => {
                if resolve {
                    if let Some(foil) = winner(&poll.tallies()) {
                        self.active_foil = Some(foil.clone());
                        events.push(StreamEvent::Op {
                            ts_ms: now_ms,
                            op: Op::SummonFoil { foil },
                        });
                    }
                }
                self.fire(Trigger::FoilVoteClosed);
            }
        }
        events
    }

    fn switch_to(&mut self, now_ms: TsMs, to: SegmentId) -> Result<Vec<StreamEvent>, SegmentError> {
        let table = Self::startable_table(to)?;
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
        if let Some(foil) = self.active_foil.take() {
            self.observer_foil = Some(foil);
        }
        Ok(events)
    }

    // ----- chat ------------------------------------------------------------------------

    /// Commands and votes from new chat. Returns `vote` events (one running tally per poll
    /// or graded hex touched in this batch) and `moderation` events (stage `input`) for
    /// under-ranked or invalid commands. Commands the segment does not take are ignored.
    pub fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        let mut poll_dirty = false;
        let mut graded: BTreeSet<String> = BTreeSet::new();
        for msg in msgs {
            self.remember_msg(&msg.id);
            if self.signed_off {
                continue;
            }
            let Some(cmd) = msg.command.clone().or_else(|| parse_command(&msg.text)) else {
                continue;
            };
            if !self.table.allows_command(command_name(&cmd)) {
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
                    if !self.config.foils.contains(&foil) {
                        events.push(ignored("unknown_foil"));
                        continue;
                    }
                    if self.poll.is_none()
                        && self.table.accepts(&self.state, Trigger::FoilVoteOpened)
                    {
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

    fn open_foil_vote(&mut self, now_ms: TsMs) -> StreamEvent {
        let id = loop {
            self.foil_vote_seq += 1;
            let id = format!("foil-vote-{}", self.foil_vote_seq);
            if !self.used_poll_ids.contains(&id) {
                break id;
            }
        };
        let secs = self.table.clock.foil_vote_secs;
        let question = "Which foil shall the Arch-Lich summon?".to_string();
        let options = self.config.foils.clone();
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
    fn file_findings(&mut self, hex_idx: usize) {
        let Some(file) = &self.case_file else {
            return;
        };
        let hex = &mut self.case.hexes[hex_idx];
        for r in file.records_for(&hex.id) {
            if self.filed.contains(&r.id) {
                continue;
            }
            self.filed.push(r.id.clone());
            if !hex.sources.contains(&r.source) {
                hex.sources.push(r.source.clone());
            }
        }
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
    /// that takes `poll_opened`. Yields its `start_poll` op.
    fn open_due_plebiscite(&mut self, now_ms: TsMs) -> Option<StreamEvent> {
        if self.signed_off
            || self.poll.is_some()
            || !self.table.allows_op("dispatch_research")
            || !self.table.accepts(&self.state, Trigger::PollOpened)
        {
            return None;
        }
        let hex = self.plebiscite_due.take()?;
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
            "The bone-clerks have searched {hex} {} times. What is the Arch-Lich's next move?",
            clock.max_searches_per_hex
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

    /// Research is withheld while a Plebiscite waits or runs: the next move is the audience's.
    fn research_withheld(&self) -> bool {
        self.plebiscite_hex().is_some()
    }

    // ----- ops -------------------------------------------------------------------------

    /// Validate and apply the brain's ops. An accepted op yields its `op` event followed by
    /// any consequence (a closed `vote`, `segment_end`/`segment_start`...). A rejected op
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
                    let mut op = op.clone();
                    // A re-minted poll id: the transcript records the id the poll has.
                    if let (Op::StartPoll { poll_id, .. }, Some(p)) = (&mut op, &self.poll) {
                        poll_id.clone_from(&p.id);
                    }
                    events.push(StreamEvent::Op { ts_ms: now_ms, op });
                    events.extend(consequences);
                }
                Err(rule) => {
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

    fn apply_op(&mut self, now_ms: TsMs, op: &Op) -> Result<Vec<StreamEvent>, &'static str> {
        if self.signed_off {
            return Err("signed_off");
        }
        if !self.table.allows_op(op_tag(op)) {
            return Err("op_not_allowed");
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
                if self.poll.is_some() {
                    return Err("poll_already_open");
                }
                self.require_state(Trigger::PollOpened)?;
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
                self.require_state(Trigger::FoilSummoned)?;
                if self.active_foil.as_deref() == Some(foil) {
                    return Err("foil_already_active");
                }
                self.active_foil = Some(foil.clone());
                self.fire(Trigger::FoilSummoned);
            }
            Op::DismissFoil => {
                if self.active_foil.is_none() {
                    return Err("no_active_foil");
                }
                self.require_state(Trigger::FoilDismissed)?;
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
                self.require_state(Trigger::HexRevealed)?;
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
                if lifts_fog && self.arc.is_some() {
                    self.reveals.push(self.elapsed_ms());
                }
                self.fire(Trigger::HexRevealed);
            }
            Op::DispatchResearch { job_id, brief } => {
                if job_id.trim().is_empty() || brief.trim().is_empty() {
                    return Err("invalid_job");
                }
                if self.jobs.contains(job_id) {
                    return Err("duplicate_job_id");
                }
                if self.research_withheld() {
                    return Err("plebiscite_pending");
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
                    *self.searches.entry(hex).or_default() += 1;
                    self.file_findings(i);
                }
                self.jobs.insert(job_id.clone());
            }
            Op::ShowCard { card_id, title, .. } => {
                if card_id.trim().is_empty() || title.trim().is_empty() {
                    return Err("invalid_card");
                }
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
                // An arc is the whole planned segment: it is not handed over midway.
                if self.arc.is_some() {
                    return Err("arc_in_progress");
                }
                if !self.table.switch_to.contains(to) {
                    return Err("switch_not_allowed");
                }
                if Self::startable_table(*to).is_err() {
                    return Err("segment_descoped");
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
            Op::SignOff { reason } => {
                if self.sign_off_withheld() && !reason.starts_with("fleet_lost") {
                    return Err("dossier_not_sealed");
                }
                let events = self.close_poll(now_ms, false);
                self.signed_off = true;
                return Ok(events);
            }
        }
        Ok(Vec::new())
    }

    // ----- clock -----------------------------------------------------------------------

    /// Clock: poll timeouts (a closed `vote`; a foil vote also summons its winner) and the
    /// handover rule. When the rule fires it emits `handover` and enters the
    /// `handover_pending` state; the brain is expected to emit `switch_segment`, and after
    /// the rule's grace period the clock switches by itself (`segment_end`, `segment_start`).
    pub fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
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
        if ccv >= rule.ccv_min {
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

    // ----- context ---------------------------------------------------------------------

    /// What the brain sees this tick. `rules_prompt` is stable within a segment (table text
    /// plus config facts); `state_summary` carries the live state. Untrusted chat text
    /// (leads, proposals) is summarised as counts only. While a Plebiscite waits or runs,
    /// `dispatch_research` is withheld from `allowed_ops`.
    pub fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        SegmentContext {
            segment: self.table.id,
            rules_prompt: self.rules_prompt(),
            state_summary: self.state_summary(),
            state: self.state.clone(),
            allowed_ops: if self.signed_off {
                Vec::new()
            } else {
                let withheld = self.research_withheld();
                let unsealed = self.sign_off_withheld();
                self.table
                    .allowed_ops
                    .iter()
                    .filter(|o| !(withheld && o.as_str() == "dispatch_research"))
                    .filter(|o| !(unsealed && o.as_str() == "sign_off"))
                    .filter(|o| !(self.arc.is_some() && o.as_str() == "switch_segment"))
                    .cloned()
                    .collect()
            },
            ccv,
            elapsed_s,
            hexes: if self.table.id == SegmentId::Seg02aHexmap {
                self.case().hexes.clone()
            } else {
                Vec::new()
            },
        }
    }

    fn rules_prompt(&self) -> String {
        let t = self.table;
        let mut p = t.rules_prompt.trim_end().to_string();
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
                     When an act's map is done early, deepen it: jury polls, the minions' \
                     leads, the records on file, a recap for newcomers. The last {} minutes are \
                     the seal window: recap the dossier (every revealed hex with its grade, \
                     alleged below DOC), then sign_off. sign_off is refused before it, and the \
                     investigation is the whole segment: no switch_segment.",
                    minutes(arc.planned_ms),
                    arc.acts.len(),
                    acts.join("; "),
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
                    p.push_str(
                        " A hex is revealed only after a beat of work on it: a bone-clerk search \
                         (dispatch_research naming it) or a closed crowd poll whose winning option \
                         names it (reveal_needs_work).",
                    );
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
                 trace the treasury wallet\"). The bone-clerks search a hex at most {} times \
                 per stream; after that its next move goes to the Plebiscite, which opens by \
                 itself. Never re-dispatch a spent hex: annex it with the evidence you hold, \
                 or let the vote decide.",
                t.clock.max_searches_per_hex
            ));
            match &self.case_file {
                Some(f) if f.fictional => p.push_str(
                    " The case file holds planted FICTIONAL demo records: quote them as the \
                     demo's records, never as real findings.",
                ),
                Some(_) => p.push_str(
                    " The bone-clerks read the case's public records, staged off-stream from \
                     regulator and court sites: quote a record at its grade and never as more \
                     than it says. A hex with nothing on file stays unproven.",
                ),
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
        lines.push(format!("bone-clerk searches: {}", counts.join(", ")));
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
                Some(option) => format!(
                    "PLEBISCITE on {hex} decided: \"{option}\". Carry it out; do not search \
                     {hex} again."
                ),
                None => format!(
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
                     disputed hex, the minions' leads, the records on file, {recap}). Do not \
                     stall and do not sign off.",
                    act + 1,
                    act + 2
                ));
            }
        } else if arc.sealing(elapsed) {
            lines.push(
                "SEAL WINDOW: recap the dossier now (every revealed hex with its grade, alleged \
                 below DOC), thank the minions, then sign_off."
                    .into(),
            );
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
                    "needs a beat of work before its reveal (a bone-clerk search on it, or a \
                     closed crowd poll that picks it): {}",
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
        if let Some((id, won)) = &self.last_crowd {
            lines.push(match won {
                Some(w) => format!("last poll {id} closed: winner {w}"),
                None => format!("last poll {id} closed: no winner (tie or no votes)"),
            });
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
        if self.handover_pending_since.is_some() {
            if let Some(rule) = self.handover_rule() {
                lines.push(format!(
                    "HANDOVER DUE: let the foil dare you, then emit switch_segment to {}",
                    segment_name(rule.to)
                ));
            }
        }
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
                lines.push(format!("minion leads: {}", top.join(", ")));
            }
            if !self.jobs.is_empty() {
                lines.push(format!("research jobs dispatched: {}", self.jobs.len()));
            }
            if self.table.allows_op("dispatch_research") {
                lines.extend(self.research_lines());
            }
        }
        if !self.proposals.is_empty() {
            lines.push(format!("proposals waiting: {}", self.proposals.len()));
        }
        lines.join("\n")
    }
}
