//! The case as an owned loop: in an investigation segment, "advance the case" is one of the
//! orchestrator's open loops, and at least one beat in every [`CASE_BEAT_MS`] carries its next
//! move ahead of ordinary chat.
//!
//! The judged show (sbx-20261022-093002-10m) ran SEG-02A for 5.5 minutes with 41 `highlight`s
//! and no `dispatch_research`, `reveal_hex`, `show_screen` or `pin_link` from the model: every
//! beat was chat-led, a prepared bit or a through-line bridge, and the state summary's "make the
//! wait count" advice lost to all of them. The bench show the day before moved the case only on
//! the beats nothing else claimed ("nothing prepared fits: improvised"); the through-line's
//! bridges took half of those (a scripted replay: 8 improvised case moves at 5c38732, 4 at
//! 7a3f12c), and a busy room has none in either.
//!
//! The next move, in this order, from the segment context (the case's cards, the op menu and the
//! state summary's research, board and screen lines):
//! - **reveal** a face-down card whose research came back with a record on file, at the grade of
//!   its strongest record (one step down when the runtime refused a DOC for want of a primary
//!   source), when no reveal is waiting on the pacing clock;
//! - **pin** a string from a face-up card with a returned record and no string to a face-up card
//!   that record names (the model chooses among them and states the relation, citing only what
//!   the record says); with the record's text unknown, to any face-up card it connects to;
//! - **dispatch** research when none is out: the lead chat hinted most (`!hint`), else a
//!   face-down card a returned record names (the trail: the next string's other end), else the
//!   next face-down card that was never searched; never a locked, spent or empty-searched one;
//! - **show** a returned record that was never on screen.
//!
//! The directive names the op line exactly (the pin's card and relation are the model's to fill),
//! and names cards and records first with their ids in brackets ([`CaseNames`]): ids are for ops.
//! A move the model declines is parked so the case never loops on it (the first voiced run asked
//! for the same string eight times while the model put the record up instead each time,
//! sbx-20261022-102618-10m): a string answered with its record on screen waits until another
//! card is face up; a move asked twice with no case op in reply waits [`CASE_BEAT_MS`]. The
//! second run pinned nothing: every string it was offered pointed at cards its record never
//! named (sbx-20261022-104558-10m), hence the named cards and the trail.
//! The state summary is parsed as the runtime writes it today; a structured case state in
//! `SegmentContext` is the planned replacement (as for votes and room events).
//!
//! The timings are assumptions to tune against voiced sandbox runs: [`CASE_BEAT_MS`],
//! [`CASE_PAYOFF_MS`], [`CASE_RETRY_MS`].

use std::collections::{HashMap, HashSet};

use shadowrunr_types::{EvidenceGrade, Op, SegmentContext, TsMs};

use crate::case_names::CaseNames;

/// At least one beat in this long carries the case's next move while one is open (assumption:
/// eight beats, so a 10-minute investigation makes about a dozen moves and the room keeps the
/// beats between).
pub const CASE_BEAT_MS: i64 = 40_000;
/// A payoff (a reveal of research that came back, a string) comes this long after the last case
/// move at the earliest (assumption: three beats).
pub const CASE_PAYOFF_MS: i64 = 15_000;
/// A case beat whose reply carried no case op is asked again after this long (assumption: two
/// beats).
pub const CASE_RETRY_MS: i64 = 10_000;
/// Heading of the per-turn case section.
pub const CASE_HEADING: &str = "The case";
/// The beat note of a case beat that answers no chat.
pub const CASE_NOTE: &str = "(No chat to answer this beat: make the case move above.)";
/// The line after the directive of a case beat that also answers one message.
pub const CASE_WITH_CHAT: &str = "Then answer the picked message below in your other line, by \
name and rank title.";
/// The ops a case move may use: a segment offers the loop only with one of them on its menu
/// and the case's cards in its context.
const CASE_OPS: &[&str] = &["dispatch_research", "reveal_hex", "pin_link", "show_screen"];
/// The relations a string may state (`LinkRel`), for the pin directive.
const RELATIONS: &str =
    "paid|owns|controls|promoted|processed|regulates|claims|supports|contradicts";

/// The next investigative move.
#[derive(Debug, Clone, PartialEq)]
pub enum CaseMove {
    /// Research a face-down card: a hinted lead, the trail, or the next unsearched card.
    Dispatch {
        hex: String,
        label: String,
        why: Why,
    },
    /// Turn a card face up at the grade of the record that came back.
    Reveal {
        hex: String,
        grade: EvidenceGrade,
        record: String,
    },
    /// Pin a string from a face-up card, citing its returned record, to one of `face_up`.
    Pin {
        from: String,
        record: String,
        face_up: Vec<String>,
    },
    /// Put a returned record up on the browser.
    Show { record: String },
}

/// Why research goes to a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The lead chat hinted most.
    Lead,
    /// A returned record names it.
    Trail,
    /// The next face-down card never searched.
    Next,
}

/// The name a record would use for a card: its label up to a parenthesis or a comma
/// ("Mirecrown Trust (fictional)" -> "mirecrown trust"), lowercase; `None` when too short to
/// match safely.
fn core_label(label: &str) -> Option<String> {
    let core = label.split(['(', ',']).next().unwrap_or_default().trim();
    (core.chars().count() >= 5).then(|| core.to_lowercase())
}

/// The id of the card a research summary is about: the first word, when it is a card.
fn summary_card<'a>(summary: &str, known: &HashSet<&'a str>) -> Option<&'a str> {
    let first = summary.split_whitespace().next()?;
    known.get(first).copied()
}

fn grade_name(g: EvidenceGrade) -> &'static str {
    match g {
        EvidenceGrade::Doc => "DOC",
        EvidenceGrade::Rep => "REP",
        EvidenceGrade::Clm => "CLM",
        EvidenceGrade::Spc => "SPC",
    }
}

fn parse_grade(s: &str) -> Option<EvidenceGrade> {
    Some(match s.trim_end_matches(':') {
        "DOC" => EvidenceGrade::Doc,
        "REP" => EvidenceGrade::Rep,
        "CLM" => EvidenceGrade::Clm,
        "SPC" => EvidenceGrade::Spc,
        _ => return None,
    })
}

/// Strongest first.
fn rank(g: EvidenceGrade) -> u8 {
    match g {
        EvidenceGrade::Doc => 0,
        EvidenceGrade::Rep => 1,
        EvidenceGrade::Clm => 2,
        EvidenceGrade::Spc => 3,
    }
}

fn op_line(op: &Op) -> String {
    format!("OP: {}", serde_json::to_string(op).unwrap_or_default())
}

impl CaseMove {
    /// A short tag for the `beat_plan` reason ("dispatch HEX-03 (lead)", "reveal HEX-01 DOC").
    pub fn tag(&self) -> String {
        match self {
            CaseMove::Dispatch { hex, why, .. } => format!(
                "dispatch {hex}{}",
                match why {
                    Why::Lead => " (lead)",
                    Why::Trail => " (trail)",
                    Why::Next => "",
                }
            ),
            CaseMove::Reveal { hex, grade, .. } => format!("reveal {hex} {}", grade_name(*grade)),
            CaseMove::Pin { from, record, .. } => format!("pin {from} by {record}"),
            CaseMove::Show { record } => format!("show {record}"),
        }
    }

    /// A reveal or a string: the payoff of research that came back.
    pub fn is_payoff(&self) -> bool {
        matches!(self, CaseMove::Reveal { .. } | CaseMove::Pin { .. })
    }

    /// The per-turn directive, with the exact op line (`job` names a dispatch's job). Cards and
    /// records are named first and their ids follow in brackets (`names`): ids are for the op.
    pub fn directive(&self, job: &str, names: &CaseNames) -> String {
        let head = "Advance the case this beat, before any chat:";
        let n = |id: &str| names.tagged(id);
        match self {
            CaseMove::Dispatch { hex, label, why } => format!(
                "{head} send research to {}, {}. Say so in one short line, by name, then put \
                 this action on its own line, exactly:\n{}",
                n(hex),
                match why {
                    Why::Lead => "the lead chat hinted most",
                    Why::Trail => "a card a returned record names",
                    Why::Next => "the next face-down card",
                },
                op_line(&Op::DispatchResearch {
                    job_id: job.to_string(),
                    brief: format!("{hex}: what the records say about {label}"),
                })
            ),
            CaseMove::Reveal { hex, grade, record } => format!(
                "{head} the research on {} came back ({}, {}). Turn the card face up at that \
                 grade and say what the record shows in one short line{}; then put this action \
                 on its own line, exactly:\n{}",
                n(hex),
                n(record),
                grade_name(*grade),
                if *grade == EvidenceGrade::Doc {
                    ""
                } else {
                    " (below DOC: say alleged)"
                },
                op_line(&Op::RevealHex {
                    hex: hex.clone(),
                    grade: *grade,
                })
            ),
            CaseMove::Pin {
                from,
                record,
                face_up,
            } => format!(
                "{head} {} is face up with {} on file and no string. Pin a string between that \
                 card and the face-up card the record connects it to (one of: {}), citing only \
                 what the record says, and name the link in one short line. The string reads \
                 as a sentence, from -> rel -> to: \"from\" is the card that acts (the regulator, \
                 the payer, the owner, the promoter), \"to\" the card it acts on. Put the action on \
                 its own line, filled in with the cards' ids:\nOP: \
                 {{\"op\":\"pin_link\",\"from\":\"<card that acts>\",\"to\":\"<card acted on>\",\
                 \"rel\":\"<{RELATIONS}>\",\"record_id\":\"{record}\"}}\nIf it connects to none of \
                 them, put the record up instead:\n{}",
                n(from),
                n(record),
                face_up.iter().map(|h| n(h)).collect::<Vec<_>>().join(", "),
                op_line(&Op::ShowScreen {
                    screen: shadowrunr_types::ScreenId::Browser,
                    record_id: Some(record.clone()),
                    focus: None,
                })
            ),
            CaseMove::Show { record } => format!(
                "{head} put {} up on the browser and read from it in one short line; put this \
                 action on its own line, exactly:\n{}",
                n(record),
                op_line(&Op::ShowScreen {
                    screen: shadowrunr_types::ScreenId::Browser,
                    record_id: Some(record.clone()),
                    focus: None,
                })
            ),
        }
    }
}

/// The case as the segment context shows it this beat.
#[derive(Debug, Default)]
struct View {
    /// Face-down cards the next move may take, in the runtime's order.
    open: Vec<String>,
    face_up: Vec<String>,
    /// Hinted leads, most hinted first.
    leads: Vec<String>,
    /// Cards searched at least once, and the spent ones.
    searched: HashSet<String>,
    spent: HashSet<String>,
    /// Records on file, in filing order: (card, record, grade).
    records: Vec<(String, String, EvidenceGrade)>,
    /// Cards searched with nothing on file.
    empty: HashSet<String>,
    /// The pacing clock holds the next reveal.
    reveal_waiting: bool,
    /// Face-up cards with a returned record and no string.
    open_leads: Vec<String>,
    /// The record on screen, if any.
    on_screen: Option<String>,
    /// The runtime's "REFUSED lately" line.
    refused: String,
    /// A Plebiscite holds research.
    plebiscite: bool,
}

/// The ids in a `, `-separated list that are cards of the case (the first word of each part).
fn cards_in(list: &str, known: &HashSet<&str>) -> Vec<String> {
    list.split(", ")
        .filter_map(|p| p.split_whitespace().next())
        .filter(|id| known.contains(id))
        .map(str::to_string)
        .collect()
}

impl View {
    fn read(ctx: &SegmentContext) -> Self {
        let known: HashSet<&str> = ctx.hexes.iter().map(|h| h.id.as_str()).collect();
        let mut v = View {
            face_up: ctx
                .hexes
                .iter()
                .filter(|h| !h.fog)
                .map(|h| h.id.clone())
                .collect(),
            ..View::default()
        };
        let mut locked: HashSet<String> = HashSet::new();
        let mut open_in_arc: Option<Vec<String>> = None;
        let mut leads: Vec<(u32, String)> = Vec::new();
        for line in ctx.state_summary.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("fogged and open: ") {
                open_in_arc = Some(cards_in(rest, &known));
            } else if let Some(rest) = l.strip_prefix("LOCKED until act ") {
                if let Some((_, list)) = rest.split_once(": ") {
                    locked.extend(cards_in(list, &known));
                }
            } else if let Some(rest) = l.strip_prefix("Board: ") {
                if let Some((_, after)) = rest.split_once("Open leads: ") {
                    let list = after
                        .split(" has a returned")
                        .next()
                        .unwrap_or_default()
                        .split(" have a returned")
                        .next()
                        .unwrap_or_default();
                    v.open_leads = cards_in(list, &known);
                }
            } else if let Some(rest) = l.strip_prefix("Screen: ") {
                for p in ["browser on ", "desk on "] {
                    if let Some(r) = rest.strip_prefix(p) {
                        v.on_screen = r
                            .split(|c: char| c.is_whitespace() || c == ',' || c == '.')
                            .next()
                            .map(str::to_string);
                    }
                }
            } else if l.starts_with("REFUSED lately") {
                v.refused = l.to_string();
            } else if l.starts_with("PLEBISCITE on ") && l.contains("the audience decides") {
                v.plebiscite = true;
            } else if l.contains("The next reveal unlocks at minute") {
                v.reveal_waiting = true;
            } else if let Some(rest) = l.strip_prefix("searched, nothing on file: ") {
                v.empty.extend(cards_in(rest, &known));
            } else if let Some((_, rest)) = l.split_once("case file (") {
                let Some((_, list)) = rest.split_once("): ") else {
                    continue;
                };
                for entry in list.split("; ") {
                    let mut w = entry.split_whitespace();
                    let (Some(hex), Some(rec), Some(g)) = (w.next(), w.next(), w.next()) else {
                        continue;
                    };
                    if let (true, Some(g)) = (known.contains(hex), parse_grade(g)) {
                        v.records.push((hex.to_string(), rec.to_string(), g));
                    }
                }
            } else if let Some((_, rest)) = l.split_once(" searches: ") {
                for part in rest.split(", ") {
                    let mut w = part.split_whitespace();
                    if let Some(hex) = w.next().filter(|h| known.contains(h)) {
                        v.searched.insert(hex.to_string());
                        if part.ends_with("SPENT") {
                            v.spent.insert(hex.to_string());
                        }
                    }
                }
            } else if let Some((head, rest)) = l.split_once(" leads: ") {
                if head.contains(':') {
                    continue;
                }
                for part in rest.split(", ") {
                    let Some((hex, n)) = part.split_once(" x") else {
                        continue;
                    };
                    if known.contains(hex) {
                        leads.push((n.trim().parse().unwrap_or(1), hex.to_string()));
                    }
                }
            }
        }
        // Most hinted first; the runtime lists them so already.
        leads.sort_by_key(|l| std::cmp::Reverse(l.0));
        v.leads = leads.into_iter().map(|(_, h)| h).collect();
        let fogged: Vec<String> = ctx
            .hexes
            .iter()
            .filter(|h| h.fog)
            .map(|h| h.id.clone())
            .collect();
        v.open = open_in_arc
            .unwrap_or(fogged)
            .into_iter()
            .filter(|h| !locked.contains(h))
            .collect();
        v
    }
}

/// The case loop's state across beats.
#[derive(Debug, Default, Clone)]
pub struct CaseDesk {
    /// Research jobs out (queued or running).
    out: HashSet<String>,
    /// When the model last made a case move (an op of [`CASE_OPS`] in a delivered beat).
    last_move_ms: Option<TsMs>,
    /// When a beat last carried the directive.
    last_ask_ms: Option<TsMs>,
    /// Records named on screen so far.
    shown: HashSet<String>,
    /// Directives issued (the dispatch job ids).
    asks: u32,
    /// The move the last directive asked for, and the cards face up then, until a reply settles
    /// it.
    pending: Option<(CaseMove, usize)>,
    /// Directives a move got with no case op in reply, by tag.
    ignored: HashMap<String, u32>,
    /// Moves the model declined or ignored, by tag: skipped while parked.
    parked: HashMap<String, Parked>,
    /// The summaries of research that came back, lowercase: what the records name.
    results: Vec<String>,
}

/// How long a declined or ignored move waits.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Parked {
    /// Until this time.
    Until(TsMs),
    /// While no more than this many cards are face up.
    FaceUp(usize),
}

impl CaseDesk {
    /// Whether the segment runs a case: its cards are in the context and a case op is on the
    /// menu.
    pub fn applies(ctx: &SegmentContext) -> bool {
        !ctx.hexes.is_empty()
            && ctx
                .allowed_ops
                .iter()
                .any(|o| CASE_OPS.contains(&o.as_str()))
    }

    /// A background job changed state; a finished one's summary is what its records name.
    pub fn note_job(&mut self, job_id: &str, status: &str, summary: Option<&str>) {
        match status {
            "queued" | "running" | "progress" => {
                self.out.insert(job_id.to_string());
            }
            _ => {
                self.out.remove(job_id);
                if let Some(s) = summary.filter(|_| status == "done") {
                    self.results.push(s.to_lowercase());
                }
            }
        }
    }

    /// What the research said about `record` (lowercase), when a summary lists it
    /// (`<record> [<grade>] <text>`, ` | `-separated).
    fn record_text(&self, record: &str) -> Option<&str> {
        let tag = format!("{} [", record.to_lowercase());
        self.results
            .iter()
            .flat_map(|s| s.split(" | "))
            .find_map(|part| part.find(&tag).map(|at| &part[at..]))
    }

    /// The cards among `cards` whose name `text` mentions.
    fn named<'a>(text: &str, cards: &[&'a shadowrunr_types::Hex]) -> Vec<&'a str> {
        cards
            .iter()
            .filter(|h| core_label(&h.label).is_some_and(|c| text.contains(&c)))
            .map(|h| h.id.as_str())
            .collect()
    }

    /// An op from a delivered beat: a case op is a move, and settles the move asked for. A
    /// string answered by putting its record up instead is declined: it is parked until another
    /// card is face up.
    pub fn note_op(&mut self, op: &Op, now_ms: TsMs) {
        if let Some((mv, face_up)) = self.pending.clone() {
            let declined = matches!(
                (&mv, op),
                (CaseMove::Pin { record, .. }, Op::ShowScreen { record_id: Some(r), .. })
                    if r == record
            );
            if declined {
                self.parked.insert(mv.tag(), Parked::FaceUp(face_up));
            }
            if declined || Self::answers(&mv, op) {
                self.ignored.remove(&mv.tag());
                self.pending = None;
            }
        }
        match op {
            Op::DispatchResearch { .. } | Op::RevealHex { .. } | Op::PinLink { .. } => {
                self.last_move_ms = Some(now_ms);
            }
            Op::ShowScreen {
                record_id: Some(r), ..
            } => {
                self.shown.insert(r.clone());
                self.last_move_ms = Some(now_ms);
            }
            _ => {}
        }
    }

    /// Whether `op` carries out `mv` (or another move of its kind: a dispatch elsewhere still
    /// moves the case).
    fn answers(mv: &CaseMove, op: &Op) -> bool {
        match (mv, op) {
            (CaseMove::Dispatch { .. }, Op::DispatchResearch { .. }) => true,
            (CaseMove::Reveal { hex, .. }, Op::RevealHex { hex: h, .. }) => h == hex,
            (CaseMove::Pin { record, .. }, Op::PinLink { record_id, .. }) => record_id == record,
            (
                CaseMove::Show { record },
                Op::ShowScreen {
                    record_id: Some(r), ..
                },
            ) => r == record,
            _ => false,
        }
    }

    /// The case's next move this beat, if the segment runs a case and one is open: the first
    /// candidate (in the module's order) that is not parked. A directive the last reply left
    /// unanswered counts as ignored here; ignored twice, its move is parked [`CASE_BEAT_MS`].
    pub fn next_move(&mut self, ctx: &SegmentContext, now_ms: TsMs) -> Option<CaseMove> {
        if !Self::applies(ctx) {
            return None;
        }
        if let Some((mv, _)) = self.pending.take() {
            let n = self.ignored.entry(mv.tag()).or_insert(0);
            *n += 1;
            if *n >= 2 {
                self.ignored.remove(&mv.tag());
                self.parked
                    .insert(mv.tag(), Parked::Until(now_ms.saturating_add(CASE_BEAT_MS)));
            }
        }
        let face_up = ctx.hexes.iter().filter(|h| !h.fog).count();
        self.parked.retain(|_, p| match p {
            Parked::Until(t) => now_ms < *t,
            Parked::FaceUp(n) => face_up <= *n,
        });
        let parked = self.parked.clone();
        self.candidates(ctx)
            .into_iter()
            .find(|m| !parked.contains_key(&m.tag()))
    }

    /// Every open move, in order: reveals, strings, research, records to show.
    fn candidates(&mut self, ctx: &SegmentContext) -> Vec<CaseMove> {
        let mut out = Vec::new();
        let v = View::read(ctx);
        if let Some(r) = &v.on_screen {
            self.shown.insert(r.clone());
        }
        let menu = |t: &str| ctx.allowed_ops.iter().any(|o| o == t);
        let fogged: HashSet<&str> = ctx
            .hexes
            .iter()
            .filter(|h| h.fog)
            .map(|h| h.id.as_str())
            .collect();
        let open: HashSet<&str> = v.open.iter().map(String::as_str).collect();
        // A card whose research came back.
        if menu("reveal_hex") && !v.reveal_waiting {
            let mut seen: Vec<&str> = Vec::new();
            for (hex, _, _) in &v.records {
                if seen.contains(&hex.as_str()) || !open.contains(hex.as_str()) {
                    continue;
                }
                seen.push(hex);
                if v.refused.contains(&format!("reveal_hex {hex}: "))
                    && !v
                        .refused
                        .contains(&format!("reveal_hex {hex}: doc_requires"))
                {
                    continue;
                }
                let mut on_file: Vec<(&String, EvidenceGrade)> = v
                    .records
                    .iter()
                    .filter(|(h, _, _)| h == hex)
                    .map(|(_, r, g)| (r, *g))
                    .collect();
                on_file.sort_by_key(|(_, g)| rank(*g));
                let no_doc = v
                    .refused
                    .contains(&format!("reveal_hex {hex}: doc_requires_primary_source"));
                if let Some((record, grade)) = on_file
                    .into_iter()
                    .find(|(_, g)| !(no_doc && *g == EvidenceGrade::Doc))
                {
                    out.push(CaseMove::Reveal {
                        hex: hex.clone(),
                        grade,
                        record: record.clone(),
                    });
                }
            }
        }
        // A string a returned record supports: to the face-up cards it names, or, with its text
        // unknown, to any face-up card.
        if menu("pin_link") && v.face_up.len() >= 2 && !v.refused.contains("pin_link") {
            for hex in &v.open_leads {
                for (_, record, _) in v.records.iter().filter(|(h, _, _)| h == hex) {
                    let others: Vec<&shadowrunr_types::Hex> = ctx
                        .hexes
                        .iter()
                        .filter(|h| !h.fog && &h.id != hex)
                        .collect();
                    let face_up: Vec<String> = match self.record_text(record) {
                        Some(text) => Self::named(text, &others)
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                        None => others.iter().map(|h| h.id.clone()).collect(),
                    };
                    if !face_up.is_empty() {
                        out.push(CaseMove::Pin {
                            from: hex.clone(),
                            record: record.clone(),
                            face_up,
                        });
                    }
                }
            }
        }
        // Research on a lead, else the next face-down card.
        if menu("dispatch_research") && self.out.is_empty() && !v.plebiscite {
            let fresh = |h: &str| {
                fogged.contains(h)
                    && open.contains(h)
                    && !v.spent.contains(h)
                    && !v.empty.contains(h)
                    && !v.records.iter().any(|(r, _, _)| r == h)
            };
            let known: HashSet<&str> = ctx.hexes.iter().map(|h| h.id.as_str()).collect();
            let down: Vec<&shadowrunr_types::Hex> = ctx.hexes.iter().filter(|h| h.fog).collect();
            let mut trail: Vec<String> = Vec::new();
            for s in &self.results {
                let own = summary_card(&s.to_uppercase(), &known).map(str::to_string);
                for id in Self::named(s, &down) {
                    if own.as_deref() != Some(id) && !trail.iter().any(|t| t == id) {
                        trail.push(id.to_string());
                    }
                }
            }
            let leads = v.leads.iter().filter(|h| fresh(h)).map(|h| (h, Why::Lead));
            let trail = trail.iter().filter(|h| fresh(h)).map(|h| (h, Why::Trail));
            let next = v
                .open
                .iter()
                .filter(|h| fresh(h) && !v.searched.contains(h.as_str()))
                .map(|h| (h, Why::Next));
            let mut seen: HashSet<&String> = HashSet::new();
            for (hex, why) in leads.chain(trail).chain(next) {
                if !seen.insert(hex) {
                    continue;
                }
                let label = ctx
                    .hexes
                    .iter()
                    .find(|h| &h.id == hex)
                    .map(|h| h.label.clone())
                    .unwrap_or_default();
                out.push(CaseMove::Dispatch {
                    hex: hex.clone(),
                    label,
                    why,
                });
            }
        }
        // A returned record never on screen.
        if menu("show_screen") {
            for (_, record, _) in v.records.iter().filter(|(_, r, _)| !self.shown.contains(r)) {
                out.push(CaseMove::Show {
                    record: record.clone(),
                });
            }
        }
        out
    }

    /// Whether `mv` is due at `now_ms`: [`CASE_BEAT_MS`] since the last case move
    /// ([`CASE_PAYOFF_MS`] for a payoff), and [`CASE_RETRY_MS`] since the last directive.
    pub fn due(&self, mv: &CaseMove, now_ms: TsMs) -> bool {
        let since = |t: Option<TsMs>| t.map_or(i64::MAX, |t| now_ms.saturating_sub(t));
        let wait = if mv.is_payoff() {
            CASE_PAYOFF_MS
        } else {
            CASE_BEAT_MS
        };
        since(self.last_move_ms) >= wait && since(self.last_ask_ms) >= CASE_RETRY_MS
    }

    /// A beat carries `mv` with `face_up` cards up: its directive, staged (a failed beat rolls it
    /// back with the rest).
    pub fn ask(
        &mut self,
        mv: &CaseMove,
        face_up: usize,
        names: &CaseNames,
        now_ms: TsMs,
    ) -> String {
        self.asks += 1;
        self.last_ask_ms = Some(now_ms);
        self.pending = Some((mv.clone(), face_up));
        mv.directive(&format!("case-{}", self.asks), names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::{Hex, HexKind, SegmentId};

    fn hex(id: &str, fog: bool) -> Hex {
        Hex {
            id: id.into(),
            q: 0,
            r: 0,
            kind: HexKind::Entity,
            label: format!("{id} label"),
            grade: None,
            fog,
            sources: Vec::new(),
            role: None,
            date: None,
        }
    }

    fn nm(d: &mut CaseDesk, c: &SegmentContext) -> Option<CaseMove> {
        d.next_move(c, 0)
    }

    fn ctx(hexes: Vec<Hex>, summary: &str) -> SegmentContext {
        SegmentContext {
            segment: SegmentId::Seg02aHexmap,
            rules_prompt: String::new(),
            state_summary: summary.into(),
            state: "investigating".into(),
            allowed_ops: [
                "highlight",
                "reveal_hex",
                "dispatch_research",
                "show_screen",
                "pin_link",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            ccv: 30,
            elapsed_s: 60,
            hexes,
        }
    }

    #[test]
    fn the_moves_follow_the_case_as_the_summary_shows_it() {
        let mut d = CaseDesk::default();
        let four = |fog1: bool, fog2: bool| {
            vec![
                hex("HEX-01", fog1),
                hex("HEX-02", fog2),
                hex("HEX-03", true),
                hex("HEX-04", true),
            ]
        };
        // Nothing yet: the most hinted lead.
        let c = ctx(
            four(true, true),
            "fogged: HEX-01, HEX-02, HEX-03, HEX-04\nminion leads: HEX-03 x2, HEX-04 x1",
        );
        let mv = nm(&mut d, &c).unwrap();
        assert_eq!(mv.tag(), "dispatch HEX-03 (lead)");
        let line = d.ask(&mv, 0, &CaseNames::read(&c), 0);
        assert!(
            line.contains(
                r#"OP: {"op":"dispatch_research","job_id":"case-1","brief":"HEX-03: what the records say about HEX-03 label"}"#
            ),
            "{line}"
        );
        // Research out: no second dispatch; no lead: the next card never searched.
        d.note_job("case-1", "queued", None);
        assert_eq!(nm(&mut d, &ctx(four(true, true), "fogged: HEX-01")), None);
        d.note_job("case-1", "done", None);
        let c = ctx(
            four(true, true),
            "bone-clerk searches: HEX-01 1/2\nsearched, nothing on file: HEX-01",
        );
        assert_eq!(nm(&mut d, &c).unwrap().tag(), "dispatch HEX-02");
        // It came back: reveal at the strongest grade; a refused DOC steps down.
        let filed = "bone-clerk searches: HEX-01 1/2\ncase file (planted FICTIONAL demo records): \
                     HEX-01 REP-01 REP: news; HEX-01 DOC-01 DOC: order";
        assert_eq!(
            nm(&mut d, &ctx(four(true, true), filed)).unwrap().tag(),
            "reveal HEX-01 DOC"
        );
        let refused = format!(
            "{filed}\nREFUSED lately, so these did NOT happen (never tell chat they did): \
             reveal_hex HEX-01: doc_requires_primary_source"
        );
        assert_eq!(
            nm(&mut d, &ctx(four(true, true), &refused)).unwrap().tag(),
            "reveal HEX-01 REP"
        );
        // The pacing clock holds reveals: the next move is research.
        let waiting = format!("{filed}\nREVEAL PACE: ... The next reveal unlocks at minute 9");
        assert_eq!(
            nm(&mut d, &ctx(four(true, true), &waiting)).unwrap().tag(),
            "dispatch HEX-02"
        );
        // Two cards up and an open lead: the string, with the other face-up cards named.
        let board = format!(
            "{filed}\nBoard: 2 cards face up, 0 strings. Open leads: HEX-01 (company) has a \
             returned record and no string."
        );
        let c = ctx(four(false, false), &board);
        let mv = nm(&mut d, &c).unwrap();
        assert_eq!(mv.tag(), "pin HEX-01 by REP-01");
        // Named first, the ids in brackets for the op line.
        let line = mv.directive("x", &CaseNames::read(&c));
        assert!(
            line.contains("HEX-01 label [HEX-01] is face up with news [REP-01] on file"),
            "{line}"
        );
        assert!(
            line.contains("connects it to (one of: HEX-02 label [HEX-02])"),
            "{line}"
        );
        // Nothing else open: a returned record never on screen; once shown, none.
        let quiet = format!(
            "{filed}\nScreen: browser on REP-01. Records on file, newest first: DOC-01, REP-01\n\
             Board: 2 cards face up, 1 string."
        );
        let c = ctx(four(false, false), &quiet);
        d.note_job("r9", "running", None);
        assert_eq!(nm(&mut d, &c).unwrap().tag(), "show DOC-01");
        d.note_op(
            &Op::ShowScreen {
                screen: shadowrunr_types::ScreenId::Browser,
                record_id: Some("DOC-01".into()),
                focus: None,
            },
            1_000,
        );
        assert_eq!(nm(&mut d, &c), None);
    }

    #[test]
    fn a_move_is_due_every_case_beat_a_payoff_sooner_and_an_ignored_ask_again() {
        let mut d = CaseDesk::default();
        let dispatch = CaseMove::Dispatch {
            hex: "HEX-01".into(),
            label: "x".into(),
            why: Why::Next,
        };
        let reveal = CaseMove::Reveal {
            hex: "HEX-01".into(),
            grade: EvidenceGrade::Rep,
            record: "REP-01".into(),
        };
        assert!(d.due(&dispatch, 0), "the first move is due at once");
        d.ask(&dispatch, 0, &CaseNames::default(), 0);
        assert!(!d.due(&dispatch, 5_000));
        assert!(d.due(&dispatch, CASE_RETRY_MS), "ignored: asked again");
        d.note_op(
            &Op::DispatchResearch {
                job_id: "j".into(),
                brief: "b".into(),
            },
            CASE_RETRY_MS,
        );
        assert!(!d.due(&dispatch, CASE_RETRY_MS + CASE_PAYOFF_MS));
        assert!(d.due(&reveal, CASE_RETRY_MS + CASE_PAYOFF_MS));
        assert!(d.due(&dispatch, CASE_RETRY_MS + CASE_BEAT_MS));
    }

    #[test]
    fn a_segment_without_cards_or_case_ops_runs_no_case() {
        let mut d = CaseDesk::default();
        let mut c = ctx(vec![hex("HEX-01", true)], "fogged: HEX-01");
        c.allowed_ops = vec!["highlight".into(), "start_poll".into()];
        assert_eq!(nm(&mut d, &c), None);
        let mut c = ctx(Vec::new(), "");
        c.segment = SegmentId::Seg01ImprovisedRoom;
        assert_eq!(nm(&mut d, &c), None);
    }

    #[test]
    fn a_declined_string_waits_for_another_card_and_an_ignored_move_for_a_case_beat() {
        let mut d = CaseDesk::default();
        let cards = |up: usize| -> Vec<Hex> {
            (1..=4).map(|i| hex(&format!("HEX-0{i}"), i > up)).collect()
        };
        let board = "case file (planted FICTIONAL demo records): HEX-02 CLM-01 CLM: snapshot
                     Board: 2 cards face up, 0 strings. Open leads: HEX-02 (company) has a                      returned record and no string.";
        let c = ctx(cards(2), board);
        let mv = d.next_move(&c, 0).unwrap();
        assert_eq!(mv.tag(), "pin HEX-02 by CLM-01");
        d.ask(&mv, 2, &CaseNames::read(&c), 0);
        // The model put the record up instead: the string waits; research is next.
        d.note_op(
            &Op::ShowScreen {
                screen: shadowrunr_types::ScreenId::Browser,
                record_id: Some("CLM-01".into()),
                focus: None,
            },
            0,
        );
        assert_eq!(d.next_move(&c, 5_000).unwrap().tag(), "dispatch HEX-03");
        // Another card face up: the string is offered again.
        let c3 = ctx(cards(3), board);
        assert_eq!(
            d.next_move(&c3, 6_000).unwrap().tag(),
            "pin HEX-02 by CLM-01"
        );
        // A dispatch asked twice with no case op in reply is parked for a case beat.
        let c = ctx(cards(2), "");
        let mv = d.next_move(&c, 10_000).unwrap();
        assert_eq!(mv.tag(), "dispatch HEX-03");
        d.ask(&mv, 2, &CaseNames::read(&c), 10_000);
        assert_eq!(d.next_move(&c, 20_000).unwrap().tag(), "dispatch HEX-03");
        d.ask(&mv, 2, &CaseNames::read(&c), 20_000);
        assert_eq!(d.next_move(&c, 30_000).unwrap().tag(), "dispatch HEX-04");
        assert_eq!(
            d.next_move(&c, 30_000 + CASE_BEAT_MS).unwrap().tag(),
            "dispatch HEX-03"
        );
    }
}
