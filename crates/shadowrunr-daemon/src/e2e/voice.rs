//! `/voice` and `/avatar` over real sockets. The voice speaks through the fake renderer: no
//! sidecar, no Python, no engine.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::{repo, start_state, test_config};
use crate::engine::EngineHandle;
use crate::live::Frame;
use crate::server::AppState;
use crate::voice::renderer::fake::FakeRenderer;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn state(tmp: &std::path::Path, enabled: bool) -> Arc<AppState> {
    let mut cfg = test_config(tmp, repo().join("characters"));
    cfg.voice.enabled = enabled;
    AppState::new(cfg, EngineHandle::scripted())
}

/// A state whose voice speaks through `fake`.
fn voiced(tmp: &std::path::Path, fake: Arc<FakeRenderer>) -> Arc<AppState> {
    let st = state(tmp, true);
    st.voice.attach(fake, st.shutdown_signal());
    st
}

fn say(text: &str) -> Frame {
    Frame::from(json!({"type":"say","ts_ms":1,"speaker":"malakor","text":text}).to_string())
}

async fn connect(addr: &str) -> Ws {
    tokio_tungstenite::connect_async(format!("ws://{addr}/voice"))
        .await
        .unwrap()
        .0
}

async fn next(ws: &mut Ws) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("a frame in time")
        {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(&t).unwrap(),
            Some(Ok(Message::Close(c))) => {
                return json!({ "closed": c.map(|c| c.reason.to_string()) })
            }
            Some(Ok(_)) => {}
            other => panic!("socket ended: {other:?}"),
        }
    }
}

async fn follow(ws: &mut Ws, run_id: Option<&str>) {
    let f = match run_id {
        Some(id) => json!({ "follow": { "run_id": id } }),
        None => json!({ "follow": {} }),
    };
    ws.send(Message::text(f.to_string())).await.unwrap();
}

async fn until_followed(st: &AppState, run_id: &str) {
    for _ in 0..1000 {
        if st.voice.followed(run_id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("{run_id} was never followed");
}

#[tokio::test]
async fn a_disabled_voice_says_off_and_voices_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state(tmp.path(), false);
    let addr = start_state(st.clone()).await;
    let mut ws = connect(&addr).await;
    let off = next(&mut ws).await;
    assert_eq!(off["type"], "status");
    assert_eq!(off["state"], "off");
    assert!(off.get("voice").is_none(), "{off}");
    let run = st.live.register("run-a", 2).unwrap();
    follow(&mut ws, None).await;
    ws.send(Message::text("{\"subscribe\":\"live\"}"))
        .await
        .unwrap();
    run.push(say("Nobody hears this."));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), ws.next())
            .await
            .is_err(),
        "no frames while off"
    );

    // Same origin rule as /events.
    let mut req = format!("ws://{addr}/voice").into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
}

#[tokio::test]
async fn voice_follows_the_newest_run_then_a_named_one() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeRenderer::new());
    let st = voiced(tmp.path(), fake.clone());
    let addr = start_state(st.clone()).await;
    let mut ws = connect(&addr).await;
    let ready = next(&mut ws).await;
    assert_eq!(
        ready,
        json!({"type":"status","state":"ready","voice":"af_heart","visemes":true,"provider":"kokoro"})
    );

    let a = st.live.register("run-a", 2).unwrap();
    follow(&mut ws, None).await;
    until_followed(&st, "run-a").await;
    a.push(say("I am Malakor."));
    let u = next(&mut ws).await;
    assert_eq!(u["type"], "utterance");
    assert_eq!(
        (
            u["id"].as_str(),
            u["run_id"].as_str(),
            u["seq"].as_u64(),
            u["part"].as_u64()
        ),
        (Some("run-a:1:0"), Some("run-a"), Some(1), Some(0))
    );
    assert_eq!(u["last"], true);
    assert_eq!(u["duration_ms"], 100);
    assert_eq!(u["visemes"][0], json!({"v":"aa","start_ms":0,"end_ms":100}));
    assert!(!super::has_null(&u));

    // A new run starts: the follow moves to it, from its first line.
    let b = st.live.register("run-b", 2).unwrap();
    b.push(say("A new stream."));
    let u = next(&mut ws).await;
    assert_eq!(
        (u["run_id"].as_str(), u["seq"].as_u64()),
        (Some("run-b"), Some(1))
    );
    assert!(
        !st.voice.followed("run-a"),
        "run-a's feeder stopped with its last follower"
    );

    // Named: run-a only, even though run-b is the newest.
    follow(&mut ws, Some("run-a")).await;
    until_followed(&st, "run-a").await;
    b.push(say("Not voiced for this client."));
    a.push(say("Back to the first."));
    let u = next(&mut ws).await;
    assert_eq!(
        (u["run_id"].as_str(), u["seq"].as_u64()),
        (Some("run-a"), Some(2))
    );

    // Malformed frames are ignored and the socket stays open: the next follow still works
    // (frames are read in order, so run-c's line below proves both were read).
    ws.send(Message::text("not json")).await.unwrap();
    ws.send(Message::text("{\"follow\":7}")).await.unwrap();

    // A run name that is not live yet is picked up when it starts.
    follow(&mut ws, Some("run-c")).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    a.finish(None);
    st.live.remove("run-a");
    let c = st.live.register("run-c", 3).unwrap();
    c.push(say("Late arrival."));
    let u = next(&mut ws).await;
    assert_eq!(u["run_id"], "run-c");

    // Shutdown closes the socket with the /events reason.
    st.shutdown();
    let closed = next(&mut ws).await;
    assert_eq!(closed["closed"], crate::events::SHUTDOWN_REASON);
}

#[tokio::test]
async fn a_slow_client_drops_frames_without_blocking_the_voice() {
    let tmp = tempfile::tempdir().unwrap();
    // About 1.3 MB of base64 per sentence.
    let fake = Arc::new(FakeRenderer::new().with_pcm(1_000_000));
    let st = voiced(tmp.path(), fake);
    let addr = start_state(st.clone()).await;
    let run = st.live.register("run-a", 2).unwrap();

    let mut slow = connect(&addr).await;
    let mut fast = connect(&addr).await;
    assert_eq!(next(&mut slow).await["state"], "ready");
    assert_eq!(next(&mut fast).await["state"], "ready");
    follow(&mut slow, None).await;
    follow(&mut fast, None).await;
    // Both sessions have attached.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The fast client gets every line while the slow one reads nothing.
    const LINES: u64 = 48;
    for seq in 1..=LINES {
        run.push(say(&format!("Line {seq}.")));
        let u = next(&mut fast).await;
        assert_eq!(
            (u["type"].as_str(), u["seq"].as_u64()),
            (Some("utterance"), Some(seq))
        );
    }

    // The slow client now reads: in order, with the dropped lines named as queue_full.
    let mut seen = Vec::new();
    let read_all = async {
        while seen.len() < LINES as usize {
            let f = next(&mut slow).await;
            seen.push((
                f["type"].as_str().unwrap().to_string(),
                f["seq"].as_u64().unwrap(),
                f,
            ));
        }
    };
    tokio::time::timeout(Duration::from_secs(60), read_all)
        .await
        .expect("the slow client caught up");
    let skipped: Vec<&Value> = seen
        .iter()
        .filter(|(t, _, _)| t == "skipped")
        .map(|(_, _, f)| f)
        .collect();
    assert!(!skipped.is_empty(), "a stalled client lost frames");
    for s in &skipped {
        assert_eq!(s["reason"], "queue_full");
        assert_eq!(s["part"], 0);
    }
    let order: Vec<u64> = seen.iter().map(|(_, s, _)| *s).collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "frames stay in line order");
    assert_eq!(
        sorted,
        (1..=LINES).collect::<Vec<_>>(),
        "every line accounted for once"
    );
}

#[tokio::test]
async fn the_avatar_rig_is_served_safely() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state(tmp.path(), false);
    let addr = start_state(st.clone()).await;
    let get = |path: &str| {
        let url = format!("http://{addr}/avatar/{path}");
        async move { reqwest::get(url).await.unwrap() }
    };
    // No rig yet.
    assert_eq!(get("layers.json").await.status(), 404);

    let rig = crate::voice::rig_dir(&st.cfg.paths.data);
    std::fs::create_dir_all(&rig).unwrap();
    std::fs::write(rig.join("layers.json"), br#"{"rig_type":"glow"}"#).unwrap();
    std::fs::write(rig.join("mouth_aa.png"), [0x89, b'P', b'N', b'G']).unwrap();
    std::fs::write(st.cfg.paths.data.join("secret.json"), b"{}").unwrap();

    let r = get("layers.json").await;
    assert_eq!(r.status(), 200);
    let h = r.headers().clone();
    assert_eq!(h["content-type"], "application/json");
    assert_eq!(h["cache-control"], "no-cache");
    assert_eq!(r.text().await.unwrap(), r#"{"rig_type":"glow"}"#);
    let png = get("mouth_aa.png").await;
    assert_eq!(png.headers()["content-type"], "image/png");

    for bad in [
        "..%2Fsecret.json",
        "..%2F..%2Fsecret.json",
        "%2E%2E%5Csecret.json",
        "..",
        "missing.png",
        "layers.txt",
    ] {
        assert_eq!(get(bad).await.status(), 404, "{bad}");
    }
}

#[tokio::test]
async fn the_avatar_rig_is_the_character_on_air_s_own_else_the_shared_one() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state(tmp.path(), false);
    let addr = start_state(st.clone()).await;
    let get = |path: &str| {
        let url = format!("http://{addr}/avatar/{path}");
        async move { reqwest::get(url).await.unwrap().text().await.unwrap() }
    };
    let data = &st.cfg.paths.data;
    let shared = crate::voice::rig_dir(data);
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("layers.json"), "shared").unwrap();
    let own = |id: &str| data.join("characters").join(id).join("avatar").join("rig");
    // The configured character (malakor) has no rig of its own: the shared one.
    assert_eq!(get("layers.json").await, "shared");
    std::fs::create_dir_all(own("malakor")).unwrap();
    std::fs::write(own("malakor").join("layers.json"), "malakor").unwrap();
    assert_eq!(get("layers.json").await, "malakor");
    // A run in flight for another character serves that character's rig.
    std::fs::create_dir_all(own("keeper")).unwrap();
    std::fs::write(own("keeper").join("layers.json"), "keeper").unwrap();
    let run = st.live.register("run-k", 2).unwrap();
    run.push(Frame::from(
        json!({"type":"stream_start","ts_ms":0,"stream_id":"run-k","character":"keeper",
               "tone_variant":"calm","core_tier":"haiku","audience_size":3})
        .to_string(),
    ));
    assert_eq!(get("layers.json").await, "keeper");
    run.finish(None);
    assert_eq!(get("layers.json").await, "malakor");
}
