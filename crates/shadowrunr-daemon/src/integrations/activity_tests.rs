//! Activity forwarding against a local fake of the Personas webhook server, which verifies
//! each signature with the verifier ported from Personas `engine/webhook.rs`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use serde_json::json;

use super::*;
use crate::integrations::signing::tests::personas_verify;

const SECRET: &str = "test-secret-not-real-7c1d";

#[derive(Default)]
struct Fake {
    /// Bodies of calls whose signature verified.
    accepted: Mutex<Vec<Value>>,
    hits: AtomicU64,
    bad_signatures: AtomicU64,
    /// Status to answer per verified call, in order; then 200.
    script: Mutex<VecDeque<u16>>,
}

async fn fake_post(State(f): State<Arc<Fake>>, headers: HeaderMap, body: Bytes) -> StatusCode {
    f.hits.fetch_add(1, Ordering::SeqCst);
    let sig = headers
        .get("x-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !personas_verify(SECRET, &body, sig) {
        f.bad_signatures.fetch_add(1, Ordering::SeqCst);
        return StatusCode::UNAUTHORIZED;
    }
    let next = f.script.lock().unwrap().pop_front().unwrap_or(200);
    if next == 200 {
        f.accepted
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap());
    }
    StatusCode::from_u16(next).unwrap()
}

/// Serve the fake on a free port; returns `http://127.0.0.1:<port>`.
async fn serve(f: Arc<Fake>) -> String {
    let app = Router::new()
        .route(
            "/webhook/{id}",
            post(fake_post).get(|| async { StatusCode::OK }),
        )
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    format!("http://{addr}")
}

fn closed_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", l.local_addr().unwrap())
}

fn fcfg(base: &str) -> ForwarderConfig {
    ForwarderConfig {
        endpoint: format!("{base}/webhook/trig-1"),
        secret: Secret::new(SECRET),
        filter: Filter::from_names(&super::super::settings::DEFAULT_EVENTS.map(String::from)),
        project_id: Some("proj-1".into()),
        queue: 16,
        max_attempts: 3,
        backoff: Duration::from_millis(40),
        max_backoff: Duration::from_millis(400),
        timeout: Duration::from_secs(2),
        down_cooldown: Duration::from_secs(30),
    }
}

fn stream_end(run: &str) -> ActivityEvent {
    ActivityEvent::from_stream_event(
        run,
        9,
        &json!({ "type": "stream_end", "ts_ms": 5, "reason": "sim_complete" }),
    )
    .unwrap()
}

#[test]
fn the_filter_keeps_defaults_and_always_safety() {
    let f = Filter::from_names(&super::super::settings::DEFAULT_EVENTS.map(String::from));
    let ev = |v: Value| ActivityEvent::from_stream_event("r", 1, &v);
    assert!(f.wants(&stream_end("r")));
    assert!(f.wants(&ev(json!({"type": "segment_end", "segment": "SEG-01"})).unwrap()));
    assert!(f.wants(
        &ev(json!({"type": "handover", "from": "SEG-01", "to": "SEG-02A", "reason": "clock"}))
            .unwrap()
    ));
    assert!(ev(json!({"type": "chat", "msg": {}})).is_none());
    assert!(ev(json!({"type": "say", "text": "x"})).is_none());

    let noisy = ev(json!({"type": "moderation", "rule": "flood_cluster", "stage": "input", "action": "dropped"})).unwrap();
    assert!(!f.wants(&noisy));
    let none = Filter::from_names(&[]);
    for safety in [
        json!({"type": "moderation", "rule": "safety:builtin-self-harm-disclosure", "stage": "input", "action": "priority:safety"}),
        json!({"type": "moderation", "rule": "character_break", "stage": "output", "action": "replaced"}),
        json!({"type": "moderation", "rule": "persona_leak", "stage": "output", "action": "replaced"}),
    ] {
        let e = ev(safety).unwrap();
        assert!(e.is_safety());
        assert!(none.wants(&e), "safety moderation is always forwarded");
    }
    assert!(Filter::from_names(&["moderation".into()]).wants(&noisy));
}

#[test]
fn the_payload_omits_absent_fields() {
    let body = payload(
        &ActivityEvent::tournament_complete("pilot-abc", json!({"runs": 4})),
        None,
        0,
        7,
    );
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["kind"], "tournament_complete");
    assert_eq!(v["tournament_id"], "pilot-abc");
    assert_eq!(v["source"], "shadowrunr");
    for absent in ["project_id", "run_id", "seq", "report_id"] {
        assert!(v.get(absent).is_none(), "{absent} must be omitted: {v}");
    }
    assert!(!crate::e2e::has_null(&v));
}

#[tokio::test]
async fn events_arrive_signed_and_verified() {
    let fake = Arc::new(Fake::default());
    let base = serve(fake.clone()).await;
    let f = Forwarder::spawn(fcfg(&base)).unwrap();
    assert_eq!(f.forward(stream_end("run-a")), Forwarded::Queued);
    assert_eq!(
        f.forward(ActivityEvent::report_ready(
            "rep-1",
            json!({"plan_options": 3})
        )),
        Forwarded::Queued
    );
    let chat = ActivityEvent::from_stream_event(
        "run-a",
        2,
        &json!({"type": "moderation", "rule": "flood_cluster", "stage": "input", "action": "dropped"}),
    )
    .unwrap();
    assert_eq!(f.forward(chat), Forwarded::Filtered);
    assert!(f.drain(Duration::from_secs(5)).await);

    let s = f.stats();
    assert_eq!((s.accepted, s.delivered, s.dropped()), (2, 2, 0), "{s:?}");
    assert_eq!(fake.bad_signatures.load(Ordering::SeqCst), 0);
    let got = fake.accepted.lock().unwrap().clone();
    assert_eq!(got[0]["kind"], "stream_end");
    assert_eq!(got[0]["run_id"], "run-a");
    assert_eq!(got[0]["seq"], 9);
    assert_eq!(got[0]["project_id"], "proj-1");
    assert_eq!(got[0]["detail"]["reason"], "sim_complete");
    assert_eq!(got[1]["kind"], "report_ready");
    assert_eq!(got[1]["report_id"], "rep-1");
    let text = serde_json::to_string(&got).unwrap();
    assert!(!text.contains(SECRET), "the secret never travels in a body");
}

#[tokio::test]
async fn a_wrong_secret_is_rejected_not_retried() {
    let fake = Arc::new(Fake::default());
    let base = serve(fake.clone()).await;
    let mut c = fcfg(&base);
    c.secret = Secret::new("the-wrong-secret");
    let f = Forwarder::spawn(c).unwrap();
    f.forward(stream_end("r"));
    assert!(f.drain(Duration::from_secs(5)).await);
    let s = f.stats();
    assert_eq!((s.rejected, s.delivered, s.failed), (1, 0, 0), "{s:?}");
    assert_eq!(fake.hits.load(Ordering::SeqCst), 1, "401 is not retried");
}

#[tokio::test]
async fn transient_failures_retry_with_backoff() {
    let fake = Arc::new(Fake::default());
    fake.script.lock().unwrap().extend([503, 429]);
    let base = serve(fake.clone()).await;
    let f = Forwarder::spawn(fcfg(&base)).unwrap();
    let t0 = Instant::now();
    f.forward(stream_end("r"));
    assert!(f.drain(Duration::from_secs(5)).await);
    let s = f.stats();
    assert_eq!((s.delivered, s.failed), (1, 0), "{s:?}");
    assert_eq!(fake.hits.load(Ordering::SeqCst), 3);
    // 40 ms then 80 ms of backoff before the third attempt.
    assert!(
        t0.elapsed() >= Duration::from_millis(120),
        "{:?}",
        t0.elapsed()
    );
}

#[tokio::test]
async fn when_personas_is_down_events_are_dropped_and_counted() {
    let mut c = fcfg(&closed_url());
    c.max_attempts = 2;
    c.backoff = Duration::from_millis(10);
    let f = Forwarder::spawn(c).unwrap();
    for i in 0..4 {
        f.forward(stream_end(&format!("r{i}")));
    }
    assert!(f.drain(Duration::from_secs(10)).await);
    let s = f.stats();
    // The first exhausts its attempts and marks Personas down; the rest are dropped unsent.
    assert_eq!((s.failed, s.dropped_down, s.delivered), (1, 3, 0), "{s:?}");
    assert!(s.down);
    assert_eq!(s.dropped(), 4);
}

#[tokio::test]
async fn forwarding_never_blocks_and_a_full_queue_drops() {
    let mut c = fcfg(&closed_url());
    c.queue = 1;
    c.max_attempts = 3;
    c.backoff = Duration::from_millis(300);
    let f = Forwarder::spawn(c).unwrap();
    let t0 = Instant::now();
    let results: Vec<Forwarded> = (0..50)
        .map(|i| f.forward(stream_end(&format!("r{i}"))))
        .collect();
    assert!(
        t0.elapsed() < Duration::from_millis(50),
        "{:?}",
        t0.elapsed()
    );
    let dropped = results.iter().filter(|r| **r == Forwarded::Dropped).count();
    assert!(dropped >= 47, "{dropped} of 50 dropped");
    assert_eq!(f.stats().dropped_full, dropped as u64);
}

#[tokio::test]
async fn a_daemon_live_run_forwards_its_stream_end() {
    let fake = Arc::new(Fake::default());
    let base = serve(fake.clone()).await;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = crate::e2e::test_config(tmp.path(), crate::e2e::repo().join("characters"));
    let state = AppState::new(cfg, crate::engine::EngineHandle::scripted());
    let f = Arc::new(Forwarder::spawn(fcfg(&base)).unwrap());
    tokio::spawn(watch_live(state.clone(), f.clone()));

    let req = shadowrunr_types::rpc::RpcRequest {
        method: "sim.start".into(),
        params: json!({ "spec": crate::e2e::spec("act-1"), "judge": false }),
    };
    crate::rpc::dispatch(&state, req).await.unwrap();
    let found = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let got = fake.accepted.lock().unwrap().clone();
            if let Some(end) = got.iter().find(|v| v["kind"] == "stream_end") {
                return (end.clone(), got);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("stream_end forwarded in time");
    let (end, all) = found;
    assert_eq!(end["run_id"], "act-1");
    assert!(all.iter().any(|v| v["kind"] == "segment_end"), "{all:?}");
    assert!(all.iter().all(|v| v["kind"] != "chat"));
    state.shutdown();
}

fn write_config(dir: &std::path::Path, activity: &str) -> Config {
    let path = dir.join("shadowrunr.toml");
    std::fs::write(&path, format!("[personas]\n{activity}")).unwrap();
    Config::load(Some(&path), dir).unwrap()
}

#[tokio::test]
async fn preflight_line_is_absent_off_and_amber_when_unreachable() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(preflight_line(&write_config(tmp.path(), ""))
        .await
        .is_none());

    // SAFETY-free on edition 2021; the variable name is unique to this test.
    std::env::set_var("SHADOWRUNR_TEST_PF09_SECRET", SECRET);
    let down = write_config(
        tmp.path(),
        &format!(
            "[personas.activity]\nwebhook_url = \"{}\"\ntrigger_id = \"t1\"\nsecret_env = \"SHADOWRUNR_TEST_PF09_SECRET\"\n",
            closed_url()
        ),
    );
    let r = preflight_line(&down).await.unwrap();
    assert_eq!((r.check.as_str(), r.state), ("PF-09", HealthState::Amber));
    assert!(r.detail.contains("unreachable"), "{}", r.detail);
    assert!(!r.detail.contains(SECRET));

    let fake = Arc::new(Fake::default());
    let base = serve(fake).await;
    let up = write_config(
        tmp.path(),
        &format!(
            "[personas.activity]\nwebhook_url = \"{base}\"\ntrigger_id = \"t1\"\nsecret_env = \"SHADOWRUNR_TEST_PF09_SECRET\"\n"
        ),
    );
    let r = preflight_line(&up).await.unwrap();
    assert_eq!(r.state, HealthState::Green, "{}", r.detail);

    let missing = write_config(
        tmp.path(),
        &format!(
            "[personas.activity]\nwebhook_url = \"{base}\"\ntrigger_id = \"t1\"\nsecret_env = \"SHADOWRUNR_TEST_PF09_UNSET\"\n"
        ),
    );
    let r = preflight_line(&missing).await.unwrap();
    assert_eq!(r.state, HealthState::Amber);
    assert!(
        r.detail.contains("SHADOWRUNR_TEST_PF09_UNSET is not set"),
        "{}",
        r.detail
    );
}
