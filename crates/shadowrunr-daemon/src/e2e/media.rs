//! `/media/...` over a real socket: the named files only, ranges for the player, the origin
//! rule, and no way out of the clip or session directory.

use serde_json::json;

use super::{has_null, repo, rpc, start_state, test_config};
use crate::engine::EngineHandle;
use crate::server::AppState;

const CLIP: &str = "clip-0123456789abcdef";
const SESSION: &str = "sbx-20261021-085027-5m";

#[tokio::test]
async fn media_is_served_in_ranges_and_never_outside_its_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let st = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle::scripted(),
    );
    let clip_dir = st.cfg.clip_store().dir(CLIP);
    std::fs::create_dir_all(&clip_dir).unwrap();
    let video: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    std::fs::write(clip_dir.join("cut-9x16.mp4"), &video).unwrap();
    std::fs::write(clip_dir.join("candidate.json"), b"{}").unwrap();
    let run = st.cfg.paths.sandbox_runs().join(SESSION);
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(run.join("recording.mp4"), &video[..100]).unwrap();
    std::fs::write(run.join("recording.json"), br#"{"stream_offset_ms":-542}"#).unwrap();
    std::fs::write(run.join("transcript.jsonl"), b"{}\n").unwrap();
    std::fs::write(st.cfg.paths.data.join("secret.json"), b"{}").unwrap();
    let addr = start_state(st.clone()).await;
    let http = reqwest::Client::new();
    let get = |path: String, range: Option<&'static str>, origin: Option<&'static str>| {
        let mut r = http.get(format!("http://{addr}{path}"));
        if let Some(v) = range {
            r = r.header("range", v);
        }
        if let Some(o) = origin {
            r = r.header("origin", o);
        }
        async move { r.send().await.unwrap() }
    };

    let whole = get(format!("/media/clips/{CLIP}/cut-9x16.mp4"), None, None).await;
    assert_eq!(whole.status(), 200);
    assert_eq!(whole.headers()["content-type"], "video/mp4");
    assert_eq!(whole.headers()["accept-ranges"], "bytes");
    assert_eq!(whole.bytes().await.unwrap().as_ref(), video.as_slice());

    let part = get(
        format!("/media/clips/{CLIP}/cut-9x16.mp4"),
        Some("bytes=100-199"),
        Some("tauri://localhost"),
    )
    .await;
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 100-199/1000");
    assert_eq!(part.headers()["content-length"], "100");
    assert_eq!(part.bytes().await.unwrap().as_ref(), &video[100..200]);

    let tail = get(
        format!("/media/runs/sandbox/{SESSION}/recording.mp4"),
        Some("bytes=-10"),
        None,
    )
    .await;
    assert_eq!(tail.status(), 206);
    assert_eq!(tail.bytes().await.unwrap().as_ref(), &video[90..100]);
    let local = get(
        format!("/media/runs/sandbox/{SESSION}/recording.mp4"),
        None,
        Some("http://localhost:1420"),
    )
    .await;
    assert_eq!(local.status(), 200);

    let beyond = get(
        format!("/media/clips/{CLIP}/cut-9x16.mp4"),
        Some("bytes=5000-"),
        None,
    )
    .await;
    assert_eq!(beyond.status(), 416);
    assert_eq!(beyond.headers()["content-range"], "bytes */1000");

    let foreign = get(
        format!("/media/clips/{CLIP}/cut-9x16.mp4"),
        None,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(foreign.status(), 403);

    for bad in [
        format!("/media/clips/{CLIP}/candidate.json"),
        format!("/media/clips/{CLIP}/..%2F..%2Fsecret.json"),
        format!("/media/clips/{CLIP}/%2E%2E%5Csecret.json"),
        format!("/media/clips/{CLIP}/cut-16x9.mp4"),
        "/media/clips/..%2F..%2Fsecret.json/cut-9x16.mp4".to_string(),
        "/media/clips/clip-XYZ/cut-9x16.mp4".to_string(),
        format!("/media/runs/sandbox/{SESSION}/transcript.jsonl"),
        format!("/media/runs/sandbox/{SESSION}/recording.json"),
        format!("/media/runs/sandbox/{SESSION}/..%2F..%2F..%2Fdata%2Fsecret.json"),
        "/media/runs/sandbox/..%2F..%2Fdata/recording.json".to_string(),
        "/media/runs/sandbox/run-1/recording.mp4".to_string(),
        "/media/runs/sim/t/r/recording.mp4".to_string(),
    ] {
        let r = get(bad.clone(), None, None).await;
        assert_eq!(r.status(), 404, "{bad}");
    }
}

#[tokio::test]
async fn a_sessions_recording_and_heard_lines_come_over_rpc() {
    let tmp = tempfile::tempdir().unwrap();
    let st = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle::scripted(),
    );
    let run = st.cfg.paths.sandbox_runs().join(SESSION);
    std::fs::create_dir_all(&run).unwrap();
    let addr = start_state(st.clone()).await;
    let none = rpc(&addr, "clips.recording", json!({ "session_id": SESSION })).await;
    assert_eq!(none["error"]["code"], "not_found", "{none}");
    let bad = rpc(&addr, "clips.recording", json!({ "session_id": "../data" })).await;
    assert_eq!(bad["error"]["code"], "invalid_params", "{bad}");

    std::fs::write(run.join("recording.mp4"), b"mp4").unwrap();
    std::fs::write(
        run.join("recording.json"),
        json!({"tool": "obs", "file": "elsewhere/recording.mp4", "started_wall_ms": 1,
               "stream_offset_ms": -542, "ended_wall_ms": 2, "width": 1920, "height": 1080,
               "fps": 30, "stage_url": "http://localhost:5178"})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        run.join("voice.jsonl"),
        concat!(
            r#"{"ts_ms":10643,"seq":4,"event":"rendered","part":0,"duration_ms":4626}"#,
            "
",
            r#"{"ts_ms":10720,"seq":4,"event":"played","part":0}"#,
            "
",
            r#"{"ts_ms":10790,"seq":5,"event":"skipped","part":0,"reason":"late"}"#,
            "
"
        ),
    )
    .unwrap();
    let r = rpc(&addr, "clips.recording", json!({ "session_id": SESSION })).await;
    let v = &r["result"];
    assert_eq!(
        v["media_path"],
        format!("/media/runs/sandbox/{SESSION}/recording.mp4"),
        "{r}"
    );
    assert_eq!(v["stream_offset_ms"], -542);
    assert_eq!(v["av_offset_ms"], 200);
    assert_eq!(
        v["heard"],
        json!([{ "line": 4, "start_ms": 10720, "end_ms": 10720 + 4626 }])
    );
    assert!(!has_null(&r));
}
