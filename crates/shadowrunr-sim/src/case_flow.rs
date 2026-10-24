//! FLOW-2: the investigation drives itself. A scripted SEG-02A run with the real brain and the
//! real segment runtime (the demo case and its planted records), keyless: chat is busy every
//! beat, and the core model is [`CaseModel`].
//!
//! The judged show (sbx-20261022-093002-10m) ran SEG-02A for 5.5 minutes with zero
//! `dispatch_research`, `reveal_hex` or `pin_link`: every beat answered chat, voiced a prepared
//! bit or followed a through-line bridge, and no loop owned "advance the case". The bench show
//! the day before (sbx-20261021-143530-10m) dispatched and revealed on the beats that had
//! nothing else to say ("nothing prepared fits: improvised"). [`CaseModel`] is written from
//! those two transcripts: it answers what the beat picked, voices what it was handed, and takes
//! a case action only when the beat names one or when the beat is a bare silence to fill.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use shadowrunr_brain::orchestrate::case::CASE_BEAT_MS;
use shadowrunr_brain::{load_character, Brain, BrainConfig, BrainTuning};
use shadowrunr_segments::{SegmentConfig, SegmentRuntime};
use shadowrunr_types::{
    ChatMsg, Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, ModelTier,
    Op, Platform, Rank, Role, SegmentId, StreamEvent, TsMs, Usage, Viewer, WarmSession,
};

use crate::casefile::CaseFileResearch;
use crate::runner::tests::repo;

/// The body of the per-turn section titled `title` (`# <title>`), if the prompt has one.
fn section<'a>(prompt: &'a str, title: &str) -> Option<&'a str> {
    let head = format!("# {title}\n");
    let at = if prompt.starts_with(&head) {
        0
    } else {
        prompt.find(&format!("\n{head}"))? + 1
    };
    let body = &prompt[at + head.len()..];
    Some(match body.find("\n# ") {
        Some(end) => &body[..end],
        None => body,
    })
}

/// The ids a turn shows in brackets after each card's or record's name (`Name [HEX-01]`), in
/// order.
fn ids(text: &str) -> Vec<&str> {
    text.split('[')
        .skip(1)
        .filter_map(|p| p.split_once(']').map(|(id, _)| id))
        .filter(|id| id.contains('-') && !id.contains(' '))
        .collect()
}

/// The first bracketed id in `text`.
fn first_id(text: &str) -> Option<&str> {
    ids(text).into_iter().next()
}

/// The picked chat of a prompt: (msg id, viewer name), from the fenced chat block.
fn picks(prompt: &str) -> Vec<(String, String)> {
    let Some(chat) = section(prompt, "Chat") else {
        return Vec::new();
    };
    chat.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix('[')?;
            let (id, rest) = rest.split_once("] ")?;
            let (name, _) = rest.split_once(" (")?;
            Some((id.to_string(), name.to_string()))
        })
        .collect()
}

/// A keyless core model written from the judged transcripts (see the module doc). It answers up
/// to two picks by name; copies the `OP:` lines of a beat's case move; voices prepared material
/// and through-line bridges as plain lines; and only on a bare silence (the generic quiet-beat
/// line, nothing else handed to it) advances the case on its own, from the state summary.
pub(crate) struct CaseModel {
    pub prompts: Mutex<Vec<String>>,
    lines: AtomicU32,
}

impl CaseModel {
    pub(crate) fn new() -> Self {
        Self {
            prompts: Mutex::new(Vec::new()),
            lines: AtomicU32::new(0),
        }
    }

    fn line(&self, text: &str) -> String {
        let n = self.lines.fetch_add(1, Ordering::Relaxed);
        format!("{text} ({n})")
    }

    fn core(&self, prompt: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        if let Some(case) = section(prompt, "The case") {
            // The first op line, its blanks filled as a model would: the first card offered,
            // and a relation. Cards are named first with their ids in brackets (HEX-NAMES).
            let card = case
                .split_once("connects it to (one of: ")
                .and_then(|(_, r)| first_id(r))
                .unwrap_or_default();
            let from = case
                .split_once("before any chat: ")
                .and_then(|(_, r)| first_id(r))
                .unwrap_or_default();
            let op = case
                .lines()
                .find_map(|l| l.trim().strip_prefix("OP: "))
                .map(|o| {
                    let o = o
                        .replace("<card that acts>", card)
                        .replace("<card acted on>", from);
                    match (o.find("\"rel\":\"<"), o.find(">\",\"record_id\"")) {
                        (Some(a), Some(b)) => {
                            format!("{}\"rel\":\"supports{}", &o[..a], &o[b + 1..])
                        }
                        _ => o,
                    }
                });
            if let Some(op) = op {
                out.push(self.line("The bone-clerks have their orders"));
                out.push(format!("OP: {op}"));
            }
        }
        let picked = picks(prompt);
        for (id, name) in picked.iter().take(2) {
            out.push(self.line(&format!("{name}, the Ledger has you")));
            out.push(format!(r#"OP: {{"op":"highlight","msg_id":"{id}"}}"#));
        }
        if picked.is_empty() && out.is_empty() {
            let chat = section(prompt, "Chat").unwrap_or_default();
            if section(prompt, "Tonight's material").is_some() {
                out.push(self.line("A line of tonight's material"));
            } else if chat.contains("Fill the silence with something new") {
                // A bare silence: the bench model worked the case here.
                out.push(self.line("Back to the board"));
                out.extend(self.improvised_move(prompt));
            } else {
                out.push(self.line("A step further along the thread"));
            }
        }
        out.join("\n")
    }

    /// The case move a model improvises from the state summary: research the first fogged hex
    /// not searched yet.
    fn improvised_move(&self, prompt: &str) -> Option<String> {
        let seg = section(prompt, "Segment SEG-02A")?;
        let searched = seg
            .lines()
            .find(|l| l.contains(" searches: "))
            .unwrap_or_default();
        let fogged = seg
            .lines()
            .find_map(|l| {
                l.strip_prefix("fogged: ")
                    .or_else(|| l.strip_prefix("fogged and open: "))
            })
            .unwrap_or_default();
        let hex = ids(fogged)
            .into_iter()
            .find(|h| !searched.contains(&format!("[{h}]")))?;
        Some(format!(
            r#"OP: {{"op":"dispatch_research","job_id":"imp","brief":"{hex}: what is on file"}}"#
        ))
    }
}

fn reply(text: String) -> EngineReply {
    EngineReply {
        text,
        engine: EngineKind::Scripted,
        account: None,
        model: "case-model".into(),
        first_text_ms: Some(0),
        duration_ms: 0,
        usage: Usage::default(),
        cost_usd: None,
        fallback_reason: None,
        session_id: None,
    }
}

#[async_trait]
impl Engine for CaseModel {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        Ok(reply(match req.role {
            Role::CoreThread => {
                self.prompts.lock().unwrap().push(req.prompt.clone());
                self.core(&req.prompt)
            }
            _ => "ok".into(),
        }))
    }
    async fn open_session(&self, _t: EngineRequest) -> Result<Box<dyn WarmSession>, EngineError> {
        Err(EngineError::Unavailable {
            reason: "no sessions".into(),
        })
    }
    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

/// Distinct questions to the host, so no two collapse into a wave.
const QUESTIONS: &[&str] = &[
    "@Malakor what do the bone-clerks eat on a night shift?",
    "@Malakor is the throne heated or is that just the dread?",
    "@Malakor how many candles does an audit really burn?",
    "@Malakor who irons the cape on a Tuesday?",
    "@Malakor would you rather fight one goose or ten ducks?",
    "@Malakor what was your worst tax season in the crypt?",
    "@Malakor does the moat have a lifeguard?",
    "@Malakor which minion is your favourite tonight?",
    "@Malakor have you ever lost a staring contest?",
    "@Malakor can a lich catch a cold in this weather?",
    "@Malakor what's the dress code for the Dread Council?",
    "@Malakor do skeletons get ticklish ribs?",
    "@Malakor what music plays in the archive at night?",
    "@Malakor is the rented tray still on loan?",
    "@Malakor who keeps the ledger when you sleep?",
    "@Malakor how old is the oldest grudge on file?",
    "@Malakor do you ever get bored of being evil?",
    "@Malakor what's the worst gift a minion ever sent?",
];

fn viewer(i: usize) -> Viewer {
    let ranks = [Rank::Thrall, Rank::Acolyte, Rank::Cultist, Rank::Warlock];
    Viewer {
        id: format!("busy-{i}"),
        name: format!("BusyMinion{i}"),
        platform: Platform::Sim,
        rank: ranks[i % ranks.len()],
        sub_tier: None,
        months: 0,
        gifted: 0,
        is_mod: false,
    }
}

/// Three fresh questions every beat, from twelve viewers.
fn busy_chat(t: u32) -> Vec<ChatMsg> {
    (0..3usize)
        .map(|k| {
            let n = (t as usize / 5) * 3 + k;
            let ts = i64::from(t) * 1000 - 900 + k as i64 * 100;
            ChatMsg {
                id: format!("busy-{t}-{k}"),
                ts_ms: ts,
                viewer: viewer(n % 12),
                text: QUESTIONS[n % QUESTIONS.len()].to_string(),
                command: None,
            }
        })
        .collect()
}

/// What a scripted SEG-02A run produced: every event (brain and stage), each beat's prompt.
pub(crate) struct CaseRun {
    pub events: Vec<StreamEvent>,
    pub prompts: Vec<String>,
    pub stage: SegmentRuntime,
}

/// SEG-02A for `seconds` with the real brain (the sample prep) and runtime, busy chat every
/// 5 s beat, and [`CaseModel`].
pub(crate) async fn run_seg02a(seconds: u32) -> CaseRun {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = load_character(&repo().join("characters/malakor"), "unhinged-roastmaster").unwrap();
    let mut stage = SegmentRuntime::with_config(
        SegmentId::Seg02aHexmap,
        SegmentConfig {
            start_ms: Some(0),
            planned_s: Some(seconds),
            persona: pkg.persona.clone(),
            tuning: pkg.tuning,
            ..SegmentConfig::default()
        },
    )
    .unwrap();
    let model = Arc::new(CaseModel::new());
    let engine = CaseFileResearch::wrap(model.clone(), &stage);
    let tuning = BrainTuning {
        warm_session: false,
        ..BrainTuning::for_character(&pkg)
    };
    let mut brain = Brain::new_with(
        BrainConfig {
            character: pkg,
            memory_dir: tmp.path().to_path_buf(),
            stream_id: "case-flow".into(),
            core_tier: ModelTier::Sonnet,
            pool_size: 2,
        },
        engine,
        tuning,
    )
    .await
    .unwrap();
    let prep = std::fs::read_to_string(repo().join("characters/malakor/prep.sample.json")).unwrap();
    brain.set_prep(serde_json::from_str(&prep).unwrap());
    let mut events = stage.start(0);
    let mut t = 5;
    while t <= seconds {
        let now: TsMs = i64::from(t) * 1000;
        let inbox = busy_chat(t);
        events.extend(inbox.iter().map(|m| StreamEvent::Chat {
            ts_ms: m.ts_ms,
            msg: m.clone(),
        }));
        events.extend(stage.ingest_chat(now, &inbox));
        let ctx = stage.context(30, t);
        let evs = brain.tick(now, inbox, &ctx).await.unwrap();
        let ops: Vec<Op> = evs
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Op { op, .. } => Some(op.clone()),
                _ => None,
            })
            .collect();
        let done = crate::runner::jobs_done(&evs);
        events.extend(evs);
        if !ops.is_empty() {
            let staged = stage.apply_ops(now, &ops);
            events.extend(crate::runner::drop_op_echo(&ops, staged));
        }
        for job in done {
            events.extend(stage.job_done(&job, now));
        }
        events.extend(stage.tick(now, 30));
        // Let the research jobs run between beats (they answer from the case file at once).
        tokio::time::sleep(Duration::from_millis(15)).await;
        t += 5;
    }
    let prompts = model.prompts.lock().unwrap().clone();
    CaseRun {
        events,
        prompts,
        stage,
    }
}

/// When each op the brain sent matching `pred` went out.
fn ops_at(events: &[StreamEvent], pred: impl Fn(&Op) -> bool) -> Vec<TsMs> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Op { ts_ms, op } if pred(op) => Some(*ts_ms),
            _ => None,
        })
        .collect()
}

/// The beats whose plan carried the case's next move (reason `case: ...`), with the move.
fn case_beats(events: &[StreamEvent]) -> Vec<(TsMs, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::BeatPlan { ts_ms, reason, .. } if reason.starts_with("case: ") => {
                let mv = reason.split(" with chat").next().unwrap_or_default();
                Some((*ts_ms, mv.trim_start_matches("case: ").to_string()))
            }
            _ => None,
        })
        .collect()
}

/// Busy chat every beat for five minutes: the case still dispatches within `CASE_BEAT_MS`,
/// reveals the card once the job is back, pins a string, and carries a move at least once in
/// every `CASE_BEAT_MS`; the room still gets answered on the case beats. Before FLOW-2 the same
/// run dispatched nothing (at 7a3f12c and at 5c38732 alike): no beat was a bare silence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_busy_seg02a_run_still_works_the_case() {
    let run = run_seg02a(300).await;
    let ev = &run.events;
    let dispatched = ops_at(ev, |o| matches!(o, Op::DispatchResearch { .. }));
    let revealed = ops_at(ev, |o| matches!(o, Op::RevealHex { .. }));
    let beats = case_beats(ev);
    eprintln!(
        "case beats at {beats:?}; dispatches at {dispatched:?}; reveals at {revealed:?}; cards \
         face up {}; strings {}",
        run.stage.case().hexes.iter().filter(|h| !h.fog).count(),
        run.stage.links().len()
    );
    assert!(
        dispatched.first().is_some_and(|t| *t <= CASE_BEAT_MS),
        "no dispatch_research within CASE_BEAT_MS of a busy SEG-02A: {dispatched:?}"
    );
    // The first job came back, then its card was turned face up.
    let done_at = ev
        .iter()
        .find_map(|e| match e {
            StreamEvent::Job {
                ts_ms,
                job_id,
                status,
                ..
            } if job_id == "case-1" && status == "done" => Some(*ts_ms),
            _ => None,
        })
        .expect("the first job came back");
    assert!(
        revealed.iter().any(|t| *t >= done_at),
        "no reveal after the job returned at {done_at}: {revealed:?}"
    );
    let face_up: Vec<&str> = run
        .stage
        .case()
        .hexes
        .iter()
        .filter(|h| !h.fog)
        .map(|h| h.id.as_str())
        .collect();
    assert!(face_up.contains(&"HEX-01"), "{face_up:?}");
    assert!(
        ev.iter()
            .any(|e| matches!(e, StreamEvent::LinkPinned { .. })),
        "no string pinned"
    );
    assert!(!run.stage.links().is_empty());
    // A move at least once in every CASE_BEAT_MS (plus a beat), from the start.
    let mut last = 0;
    for t in beats.iter().map(|(t, _)| *t).chain([300_000]) {
        assert!(t - last <= CASE_BEAT_MS + 5_000, "{last} -> {t}: {beats:?}");
        last = t;
    }
    // A case beat folds in at most one message (segment-led beats: off-topic chat is answered
    // once per 30 s at most), and its prompt named the move.
    for (t, _) in &beats {
        let picks = ev
            .iter()
            .find_map(|e| match e {
                StreamEvent::BeatPlan { ts_ms, picks, .. } if ts_ms == t => Some(*picks),
                _ => None,
            })
            .unwrap();
        assert!(picks <= 1, "case beat at {t} took {picks} picks");
    }
    assert!(run.prompts.iter().any(|p| p.contains("\n# The case\n")));
    // No refusals of the moves it was handed.
    let refused: Vec<String> = ev
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Moderation { action, rule, .. } if action.starts_with("rejected:") => {
                Some(format!("{rule} {action}"))
            }
            _ => None,
        })
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
}
