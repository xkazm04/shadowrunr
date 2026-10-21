//! Live `sim.start` runs: the simulation runs on a blocking thread (its brain need not be
//! `Send`), and its transcript writer hands each event, as it writes the line, to a sink that
//! fans it out to `/events` subscribers. Subscribers that attach late get the run's history first, then the
//! new events, with nothing lost or duplicated.
//!
//! Every event carries `seq`, its 1-based index in the run's transcript, so a client resumes
//! with `since`. Fan-out is a bounded broadcast ([`BROADCAST_CAPACITY`]): a subscriber that
//! falls further behind loses the overflow and is told so (see `server`), never slowing the
//! run or growing memory. The run is supervised: a panic in the sim (or in its watcher)
//! becomes an `engine` (or `internal`) error frame and the daemon keeps serving.
//!
//! Both live publishers have one shape: a sim run's `SimOptions::live_sink` ([`publisher`])
//! and a sandbox session's event sink (`sandbox`) push each event into the `LiveRun` the
//! moment the transcript writer assigns its line, so a live seq is the transcript line by
//! construction (a mismatch is logged, never silently skipped).
//! The file write stays buffered: subscribers are served from the run's memory, never from
//! the file.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use shadowrunr_types::reconcile::StreamBrief;
use shadowrunr_types::{ErrorCode, SimRunSpec, StreamEvent};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinError;

use crate::preflight;
use crate::rpc::RpcError;
use crate::runs;
use crate::server::AppState;

/// A frame for `/events` subscribers: a raw `StreamEvent` JSON line (or an error frame).
pub type Frame = Arc<str>;

/// Events a subscriber may fall behind before it loses the overflow.
pub const BROADCAST_CAPACITY: usize = 1024;

/// What a live run broadcasts.
#[derive(Debug, Clone)]
pub enum LiveItem {
    /// An event and its seq.
    Event(u64, Frame),
    /// The run ended, with its error frame when it failed.
    End(Option<Frame>),
}

/// A subscriber's view at attach time.
pub struct Attach {
    /// Events after `since`, with their seq.
    pub history: Vec<(u64, Frame)>,
    /// What follows; `None` once the run is done.
    pub rx: Option<broadcast::Receiver<LiveItem>>,
    /// The run's error frame, when it is done and failed.
    pub error: Option<Frame>,
}

struct LiveInner {
    history: Vec<Frame>,
    error: Option<Frame>,
    done: bool,
}

pub struct LiveRun {
    pub run_id: String,
    inner: Mutex<LiveInner>,
    tx: broadcast::Sender<LiveItem>,
}

impl LiveRun {
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            inner: Mutex::new(LiveInner {
                history: Vec::new(),
                error: None,
                done: false,
            }),
            tx: broadcast::Sender::new(BROADCAST_CAPACITY),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LiveInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Append one event and return its seq. The lock is held across the broadcast so an
    /// attach sees each event exactly once (in its history or on its receiver).
    pub fn push(&self, frame: Frame) -> u64 {
        let mut g = self.lock();
        g.history.push(frame.clone());
        let seq = g.history.len() as u64;
        let _ = self.tx.send(LiveItem::Event(seq, frame));
        seq
    }

    /// Events pushed so far: the seq of the last one.
    pub fn len(&self) -> u64 {
        self.lock().history.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Subscribers currently attached to what follows.
    pub fn subscribers(&self) -> usize {
        self.tx.receiver_count()
    }

    /// End the run. `error` reaches current subscribers as a final `{"error": {...}}` frame.
    pub fn finish(&self, error: Option<RpcError>) {
        let mut g = self.lock();
        g.done = true;
        g.error = error.map(|e| Frame::from(e.body().to_string()));
        let _ = self.tx.send(LiveItem::End(g.error.clone()));
    }

    /// The events after `since` plus a receiver for what follows.
    pub fn attach(&self, since: u64) -> Attach {
        let g = self.lock();
        let history = g
            .history
            .iter()
            .enumerate()
            .skip(usize::try_from(since).unwrap_or(usize::MAX))
            .map(|(i, f)| (i as u64 + 1, f.clone()))
            .collect();
        Attach {
            history,
            rx: (!g.done).then(|| self.tx.subscribe()),
            error: g.error.clone(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.lock().done
    }
}

/// Runs in flight, plus the most recent one for `{"subscribe": "live"}`.
pub struct LiveRegistry {
    runs: Mutex<HashMap<String, Arc<LiveRun>>>,
    latest: watch::Sender<Option<Arc<LiveRun>>>,
}

impl Default for LiveRegistry {
    fn default() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            latest: watch::Sender::new(None),
        }
    }
}

impl LiveRegistry {
    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<LiveRun>>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// An active run by id.
    pub fn get(&self, run_id: &str) -> Option<Arc<LiveRun>> {
        self.runs().get(run_id).cloned()
    }

    pub fn latest(&self) -> watch::Receiver<Option<Arc<LiveRun>>> {
        self.latest.subscribe()
    }

    /// Whether any run is in flight.
    pub fn any_in_flight(&self) -> bool {
        !self.runs().is_empty()
    }

    pub(crate) fn register(&self, run_id: &str, max: usize) -> Result<Arc<LiveRun>, RpcError> {
        let mut runs = self.runs();
        if runs.contains_key(run_id) {
            return Err(RpcError::new(
                ErrorCode::Conflict,
                format!("run {run_id} is already running"),
            ));
        }
        if runs.len() >= max.max(1) {
            return Err(RpcError::new(
                ErrorCode::Busy,
                format!("{} live runs already in flight (max_live_runs)", runs.len()),
            ));
        }
        let run = Arc::new(LiveRun::new(run_id.to_string()));
        runs.insert(run_id.to_string(), run.clone());
        drop(runs);
        self.latest.send_replace(Some(run.clone()));
        Ok(run)
    }

    pub(crate) fn remove(&self, run_id: &str) {
        self.runs().remove(run_id);
    }
}

/// The sink that publishes a sim run's transcript into `run` as each line is written.
fn publisher(run: Arc<LiveRun>) -> shadowrunr_sim::LiveSink {
    shadowrunr_sim::LiveSink(Arc::new(move |seq: u64, ev: &StreamEvent| {
        let Ok(text) = serde_json::to_string(ev) else {
            return;
        };
        let pushed = run.push(Frame::from(text));
        if pushed != seq {
            eprintln!(
                "sim.start {}: event seq {seq} broadcast as {pushed}",
                run.run_id
            );
        }
    }))
}

fn validate(spec: &mut SimRunSpec) -> Result<(), RpcError> {
    runs::check_id("character", &spec.character)?;
    runs::check_id("tone_variant", &spec.tone_variant)?;
    if shadowrunr_segments::segment_is_descoped(spec.segment) {
        return Err(RpcError::invalid(format!(
            "segment {} is descoped",
            shadowrunr_sim::wire(&spec.segment)
        )));
    }
    if spec.duration_s == 0 || spec.audience_size == 0 {
        return Err(RpcError::invalid(
            "duration_s and audience_size must be positive",
        ));
    }
    if spec.run_id.trim().is_empty() {
        spec.run_id = format!(
            "{}.{}",
            shadowrunr_sim::matrix::run_id(
                &spec.tone_variant,
                spec.core_tier,
                spec.segment,
                spec.audience_size,
                spec.seed
            ),
            runs::now_ms()
        );
    }
    runs::check_id("run_id", &spec.run_id)
}

/// The brief a stream starting now receives (WP15): `<data>/next_plan.json` plus the active
/// directives of `<data>/directives.json`, through the one builder
/// (`shadowrunr_brain::build_stream_brief`). Proposed and inactive directives never reach it.
/// A corrupt file refuses the start (`io`): a stream never starts on a half-read brief.
pub fn stream_brief(data_dir: &std::path::Path) -> Result<Option<StreamBrief>, RpcError> {
    shadowrunr_brain::load_stream_brief(data_dir).map_err(|e| RpcError::io(e.to_string()))
}

/// The one-line log of what a stream received.
pub fn brief_line(run_id: &str, brief: Option<&StreamBrief>) -> String {
    format!(
        "sim.start {run_id}: brief: {}",
        shadowrunr_brain::brief_summary(brief)
    )
}

/// `preflight_red`, naming every red check, when pre-flight is red. `detail.red` carries
/// each red row as `{check, detail, remedy?}`, so a client shows the fix beside the red.
pub async fn refuse_if_red(state: &AppState) -> Result<(), RpcError> {
    let rows = preflight::run_all_checked(&state.cfg, &state.engine).await;
    let red: Vec<&preflight::Checked> = rows
        .iter()
        .filter(|c| c.report.state == shadowrunr_types::HealthState::Red)
        .collect();
    if red.is_empty() {
        return Ok(());
    }
    let message = red
        .iter()
        .map(|c| format!("{} {}", c.report.check, c.report.detail))
        .collect::<Vec<_>>()
        .join("; ");
    let detail: Vec<serde_json::Value> = red
        .iter()
        .map(|c| {
            let mut row = serde_json::json!({ "check": c.report.check, "detail": c.report.detail });
            if let Some(r) = &c.remedy {
                row["remedy"] = serde_json::to_value(r).unwrap_or_default();
            }
            row
        })
        .collect();
    Err(RpcError::new(
        ErrorCode::PreflightRed,
        format!("pre-flight is red: {message}"),
    )
    .with_detail(serde_json::json!({ "red": detail })))
}

/// `sim.start`: refuse while a pre-flight check is red or a sandbox session runs, then run in
/// the background and stream on `/events`. The run lands in `<runs>/sim/<adhoc_dir>/<run_id>/`.
/// The brain gets the brief from `<data>`; the run's `stream_start` event carries it, so On Air shows what
/// Malakor actually received.
pub async fn start_sim(
    state: Arc<AppState>,
    mut spec: SimRunSpec,
    judge: bool,
) -> Result<String, RpcError> {
    validate(&mut spec)?;
    if let Some(id) = state.sandbox.running() {
        return Err(RpcError::new(
            ErrorCode::Conflict,
            format!("sandbox session {id} is running"),
        ));
    }
    refuse_if_red(&state).await?;
    let run_id = spec.run_id.clone();
    let brief = stream_brief(&state.cfg.paths.data)?;
    eprintln!("{}", brief_line(&run_id, brief.as_ref()));
    let run = state
        .live
        .register(&run_id, state.cfg.daemon.max_live_runs)?;
    let out_dir = state.cfg.paths.sim_runs().join(&state.cfg.sim.adhoc_dir);
    let run_dir = out_dir.join(&run_id);
    if let Err(e) = runs::move_aside(&run_dir, &format!("attempt-{}", runs::now_ms())) {
        state.live.remove(&run_id);
        run.finish(None);
        return Err(RpcError::io(format!("{}: {e}", run_dir.display())));
    }

    let engine = state.engine.engine.clone();
    let mut opts = state.cfg.sim_options(judge);
    opts.stream_prep = crate::prep::for_sim(&state.cfg, &spec.character);
    opts.data_dir = state.cfg.paths.data.clone();
    opts.brief = match brief {
        Some(b) => shadowrunr_sim::BriefSource::Inline(Box::new(b)),
        None => shadowrunr_sim::BriefSource::None,
    };
    opts.live_sink = Some(publisher(run.clone()));
    let handle = tokio::runtime::Handle::current();
    let sim = tokio::task::spawn_blocking(move || {
        handle.block_on(shadowrunr_sim::run_one_with(spec, out_dir, engine, &opts))
    });
    let watcher = tokio::spawn(async move { sim_outcome(sim.await) });
    let registry = state.clone();
    tokio::spawn(async move {
        let error = match watcher.await {
            Ok(outcome) => outcome.err(),
            Err(e) => Some(panic_error(ErrorCode::Internal, "run watcher", e)),
        };
        if let Some(e) = &error {
            eprintln!("sim.start {}: {e}", run.run_id);
        }
        registry.live.remove(&run.run_id);
        run.finish(error);
    });
    Ok(run_id)
}

/// A sim error or a panicking sim is an `engine` error; the daemon is unaffected.
pub fn sim_outcome<T, E: std::fmt::Display>(
    result: Result<Result<T, E>, JoinError>,
) -> Result<(), RpcError> {
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(RpcError::new(ErrorCode::Engine, e.to_string())
            .with_detail(serde_json::json!({ "reason": "sim_failed" }))),
        Err(e) => Err(panic_error(ErrorCode::Engine, "sim", e)),
    }
}

fn panic_error(code: ErrorCode, what: &str, e: JoinError) -> RpcError {
    if !e.is_panic() {
        return RpcError::new(code, format!("{what} task: {e}"));
    }
    let payload = e.into_panic();
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic payload".into());
    RpcError::new(code, format!("{what} panicked: {msg}"))
        .with_detail(serde_json::json!({ "reason": "panic" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panicking_sim_is_an_engine_error() {
        let res: Result<Result<(), String>, JoinError> =
            tokio::task::spawn_blocking(|| panic!("boom")).await;
        let e = sim_outcome(res).unwrap_err();
        assert_eq!(e.code, ErrorCode::Engine);
        assert!(e.message.contains("sim panicked: boom"), "{e}");
        let failed: Result<Result<(), String>, JoinError> = Ok(Err("quota".into()));
        assert_eq!(sim_outcome(failed).unwrap_err().code, ErrorCode::Engine);
    }

    #[test]
    fn attach_resumes_after_since_without_gaps() {
        let run = LiveRun::new("r".into());
        for i in 0..5 {
            run.push(Frame::from(format!("e{i}")));
        }
        let a = run.attach(3);
        let seqs: Vec<u64> = a.history.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, [4, 5]);
        let mut rx = a.rx.expect("run in flight");
        run.push(Frame::from("e5"));
        match rx.try_recv().unwrap() {
            LiveItem::Event(seq, f) => assert_eq!((seq, f.as_ref()), (6, "e5")),
            other => panic!("{other:?}"),
        }
        run.finish(None);
        assert!(matches!(rx.try_recv().unwrap(), LiveItem::End(None)));
        assert!(run.attach(0).rx.is_none());
    }

    #[test]
    fn the_live_brief_reads_plan_and_directives_and_excludes_proposals() {
        use shadowrunr_reconcile::DirectiveStore;
        use shadowrunr_types::reconcile::{DirectiveKind, NextStreamPlan, PlanSegment};
        use shadowrunr_types::SegmentId;

        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            stream_brief(d.path()).unwrap(),
            None,
            "no plan, no directives"
        );
        assert_eq!(brief_line("r1", None), "sim.start r1: brief: no brief");

        // The stores as the owner and the Reconciler write them.
        let store = DirectiveStore::new(d.path());
        let kept = store
            .add(DirectiveKind::Prevent, "No coin-flip bits.", None, 10)
            .unwrap();
        let scoped = store
            .add(
                DirectiveKind::Try,
                "Roll call the top minions.",
                Some(SegmentId::Seg01ImprovisedRoom),
                11,
            )
            .unwrap();
        store
            .propose(DirectiveKind::Try, "A proposal nobody kept.", None, 12)
            .unwrap();
        let retired = store
            .add(DirectiveKind::Prevent, "Retired rule.", None, 13)
            .unwrap();
        store.deactivate(&retired.id).unwrap();
        let plan = NextStreamPlan {
            plan_id: "plan-1".into(),
            created_ms: 20,
            lord_dispatch: "Read the ledger of doom aloud.".into(),
            segments: vec![PlanSegment {
                segment: SegmentId::Seg01ImprovisedRoom,
                minutes: 30,
                notes: "roll call first".into(),
            }],
            topics: vec!["socks".into()],
            directives: Vec::new(),
            report_id: None,
            option_ids: Vec::new(),
        };
        std::fs::write(
            d.path().join("next_plan.json"),
            serde_json::to_string(&plan).unwrap(),
        )
        .unwrap();

        let b = stream_brief(d.path()).unwrap().unwrap();
        assert_eq!(
            b.lord_dispatch.as_deref(),
            Some("Read the ledger of doom aloud.")
        );
        assert_eq!(b.segments, plan.segments);
        assert_eq!(b.topics, plan.topics);
        assert_eq!(
            b.directives,
            vec![kept, scoped],
            "active only: no proposal, no retired"
        );
        assert_eq!(
            brief_line("r1", Some(&b)),
            "sim.start r1: brief: dispatch yes, 1 prevent, 1 try, 1 segments, 1 topics"
        );

        // A corrupt store refuses the start.
        std::fs::write(d.path().join("directives.json"), "{").unwrap();
        assert_eq!(stream_brief(d.path()).unwrap_err().code, ErrorCode::Io);
    }
}
