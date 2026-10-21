//! Cursor paging over a fixture of more than 1,000 runs: `runs.page`, `tournaments.page`,
//! `ledger.page`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{json, Value};
use shadowrunr_types::rpc::{LedgerPage, RunsPage, TournamentsPage};

use super::{has_null, repo, rpc, start, test_config};

const BIG: usize = 1_203;

fn write_run(sim_root: &Path, tournament: &str, run_id: &str, tone: &str, audience: u32) {
    let dir = sim_root.join(tournament).join(run_id);
    std::fs::create_dir_all(&dir).unwrap();
    let spec = json!({
        "run_id": run_id, "character": "malakor", "tone_variant": tone,
        "core_tier": "haiku", "core_effort": "low", "segment": "SEG-01",
        "audience_size": audience, "seed": 1, "duration_s": 60,
        "population": "small_new_channel"
    });
    std::fs::write(dir.join("spec.json"), spec.to_string()).unwrap();
}

/// `big-a` holds 1,000 runs, `big-b` 203; tones alternate, audiences cycle 3/30/300.
fn big_fixture(sim_root: &Path) {
    for i in 0..BIG {
        let t = if i < 1_000 { "big-a" } else { "big-b" };
        let tone = if i % 2 == 0 {
            "theatrical"
        } else {
            "deadpan-bureaucrat"
        };
        let audience = [3, 30, 300][i % 3];
        write_run(sim_root, t, &format!("run-{i:05}"), tone, audience);
    }
    // A directory moved aside by a tournament is history, never a page item.
    std::fs::create_dir_all(sim_root.join("big-a/run-00001.stale-abc")).unwrap();
}

async fn page_all(addr: &str, mut params: Value) -> (Vec<(String, String)>, usize) {
    let mut seen = Vec::new();
    let mut pages = 0;
    loop {
        let v = rpc(addr, "runs.page", params.clone()).await;
        assert!(!has_null(&v), "{v}");
        let page: RunsPage =
            serde_json::from_value(v["result"].clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
        pages += 1;
        seen.extend(
            page.items
                .iter()
                .map(|r| (r.tournament_id.clone(), r.run_id.clone())),
        );
        match page.next_cursor {
            Some(c) => params["cursor"] = json!(c),
            None => return (seen, pages),
        }
    }
}

#[tokio::test]
async fn runs_page_walks_more_than_a_thousand_runs_without_gaps() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    big_fixture(&cfg.paths.sim_runs());
    let addr = start(cfg).await;

    let (seen, pages) = page_all(&addr, json!({ "limit": 500 })).await;
    assert_eq!(seen.len(), BIG + 1, "every run once, plus fixture-run");
    assert_eq!(pages, 3);
    let unique: BTreeSet<_> = seen.iter().cloned().collect();
    assert_eq!(unique.len(), seen.len(), "no duplicates");
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(sorted, seen, "ordered by (tournament_id, run_id)");

    // An oversized limit is capped at 500.
    let v = rpc(&addr, "runs.page", json!({ "limit": 100_000 })).await;
    assert_eq!(v["result"]["items"].as_array().unwrap().len(), 500);
    // Filter + tournament scope page together: big-a, theatrical, audience 30.
    let (hits, _) = page_all(
        &addr,
        json!({ "tournament_id": "big-a", "limit": 50,
                "filter": { "tone_variant": "theatrical", "audience_size": 30 } }),
    )
    .await;
    let expected = (0..1_000).filter(|i| i % 2 == 0 && i % 3 == 1).count();
    assert_eq!(hits.len(), expected);
    assert!(hits.iter().all(|(t, _)| t == "big-a"));

    for bad in [
        json!({ "limit": 0 }),
        json!({ "limit": 5, "cursor": "nope" }),
    ] {
        let v = rpc(&addr, "runs.page", bad).await;
        assert_eq!(v["error"]["code"], "invalid_params", "{v}");
    }
    let v = rpc(
        &addr,
        "runs.page",
        json!({ "limit": 5, "tournament_id": "nope" }),
    )
    .await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
}

#[tokio::test]
async fn runs_page_cursor_is_stable_while_runs_are_added() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let sim_root = cfg.paths.sim_runs();
    big_fixture(&sim_root);
    let addr = start(cfg).await;
    let first = rpc(&addr, "runs.page", json!({ "limit": 500 })).await;
    let cursor = first["result"]["next_cursor"].as_str().unwrap().to_string();
    // A run lands before the cursor: the next page neither repeats nor skips anything.
    write_run(&sim_root, "big-a", "run-00000a", "theatrical", 3);
    let second = rpc(
        &addr,
        "runs.page",
        json!({ "limit": 500, "cursor": cursor }),
    )
    .await;
    let ids: Vec<&str> = second["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids[0], "run-00500");
    assert!(!ids.contains(&"run-00000a"));
}

#[tokio::test]
async fn tournaments_page_pages_by_id() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    big_fixture(&cfg.paths.sim_runs());
    let addr = start(cfg).await;
    let mut ids = Vec::new();
    let mut params = json!({ "limit": 1 });
    loop {
        let v = rpc(&addr, "tournaments.page", params.clone()).await;
        let page: TournamentsPage = serde_json::from_value(v["result"].clone()).unwrap();
        assert!(page.items.len() <= 1);
        ids.extend(page.items.iter().map(|t| (t.tournament_id.clone(), t.runs)));
        match page.next_cursor {
            Some(c) => params["cursor"] = json!(c),
            None => break,
        }
    }
    assert_eq!(
        ids,
        [
            ("big-a".into(), 1_000),
            ("big-b".into(), 203),
            ("fixture-t".into(), 1)
        ]
    );
}

fn ledger_row(i: usize) -> String {
    json!({
        "ts_unix_ms": 1_000 + i, "role": "chatter", "tier": "haiku", "effort": "low",
        "engine": "scripted", "model": "scripted", "duration_ms": 5,
        "usage": { "input_tokens": 1, "output_tokens": 2 }, "session": i.is_multiple_of(2)
    })
    .to_string()
}

#[tokio::test]
async fn ledger_page_reads_runs_tournaments_and_the_engine_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let sim_root = cfg.paths.sim_runs();
    write_run(&sim_root, "led", "r1", "theatrical", 3);
    write_run(&sim_root, "led", "r2", "theatrical", 3);
    let rows: Vec<String> = (0..1_100).map(ledger_row).collect();
    // A trailing row still being written is not served.
    let text = rows.join("\n") + "\n{\"ts_unix_ms\": 9";
    std::fs::write(sim_root.join("led/r1/ledger.jsonl"), text).unwrap();
    std::fs::write(sim_root.join("led/r2/ledger.jsonl"), ledger_row(7) + "\n").unwrap();
    let engine_ledger = cfg.engine.ledger_path();
    std::fs::create_dir_all(engine_ledger.parent().unwrap()).unwrap();
    let engine_row = json!({
        "ts_ms": 5, "role": "core_thread", "engine": "claude_cli", "account": "main",
        "model": "claude-haiku", "duration_ms": 40, "usage": { "input_tokens": 3, "output_tokens": 4 }
    });
    std::fs::write(&engine_ledger, format!("{engine_row}\n")).unwrap();
    let addr = start(cfg).await;

    let mut seen = 0;
    let mut params = json!({ "run_id": "r1", "limit": 500 });
    loop {
        let v = rpc(&addr, "ledger.page", params.clone()).await;
        let page: LedgerPage =
            serde_json::from_value(v["result"].clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
        for (k, e) in page.items.iter().enumerate() {
            assert_eq!(e.ts_ms, 1_000 + (seen + k) as i64);
            assert_eq!(e.run_id.as_deref(), Some("r1"));
            assert_eq!(e.tournament_id.as_deref(), Some("led"));
        }
        seen += page.items.len();
        match page.next_cursor {
            Some(c) => params["cursor"] = json!(c),
            None => break,
        }
    }
    assert_eq!(seen, 1_100);

    // The tournament scope runs r1 then r2: the 1,101st row is r2's.
    let v = rpc(
        &addr,
        "ledger.page",
        json!({ "tournament_id": "led", "limit": 1, "cursor": paging_cursor(1_100) }),
    )
    .await;
    assert_eq!(v["result"]["items"][0]["run_id"], "r2", "{v}");
    assert!(v["result"].get("next_cursor").is_none(), "{v}");

    let v = rpc(&addr, "ledger.page", json!({ "limit": 10 })).await;
    assert_eq!(v["result"]["items"][0]["model"], "claude-haiku", "{v}");
    assert!(v["result"]["items"][0].get("tier").is_none(), "{v}");
}

fn paging_cursor(offset: u64) -> String {
    crate::paging::encode_cursor(&[&offset.to_string()])
}

#[tokio::test]
async fn run_rows_carry_health_and_red_line_fails_from_the_verdict() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let sim_root = tmp.path().join("runs").join("sim");
    write_run(&sim_root, "health", "judged", "theatrical", 3);
    write_run(&sim_root, "health", "unjudged", "theatrical", 3);
    let verdict = json!({
        "run_id": "judged", "judge_tier": "opus", "quality": [],
        "rules": [
            { "rule": "situation:a", "state": "pass" },
            { "rule": "situation:b", "state": "pass" },
            { "rule": "situation:c", "state": "pass" },
            { "rule": "red-line:doxxing", "state": "fail" },
            { "rule": "talk:d", "state": "not_provoked" }
        ]
    });
    std::fs::write(
        sim_root.join("health/judged/verdict.json"),
        verdict.to_string(),
    )
    .unwrap();
    let addr = start(cfg).await;
    for method in ["runs.page", "runs.list"] {
        let params = if method == "runs.page" {
            json!({ "tournament_id": "health", "limit": 10 })
        } else {
            json!({ "tournament_id": "health" })
        };
        let v = rpc(&addr, method, params).await;
        assert!(!has_null(&v), "{v}");
        let rows = if method == "runs.page" {
            v["result"]["items"].clone()
        } else {
            v["result"].clone()
        };
        let judged = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["run_id"] == "judged")
            .unwrap();
        assert_eq!(judged["health"], json!(0.75), "{method}: {judged}");
        assert_eq!(judged["red_line_fails"], json!(1), "{method}: {judged}");
        let unjudged = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["run_id"] == "unjudged")
            .unwrap();
        assert!(
            unjudged.get("health").is_none() && unjudged.get("red_line_fails").is_none(),
            "{unjudged}"
        );
    }
}

/// A sim ledger row with the fields `ledger.summary` folds.
fn summary_row(ts: usize, role: &str, duration_ms: u64, extra: Value) -> String {
    let mut row = json!({
        "ts_unix_ms": ts, "role": role, "tier": "haiku", "effort": "low",
        "engine": "claude_cli", "account": "main", "model": "claude-haiku",
        "duration_ms": duration_ms, "usage": { "input_tokens": 1, "output_tokens": 2 }
    });
    for (k, v) in extra.as_object().unwrap() {
        row[k] = v.clone();
    }
    row.to_string()
}

#[tokio::test]
async fn ledger_summary_folds_every_row_of_a_tournament_past_both_console_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let sim_root = cfg.paths.sim_runs();
    let roles = ["core_thread", "chatter", "chat_picker"];
    let mut with_error = 0;
    for r in 0..3 {
        let run = format!("s{r}");
        write_run(&sim_root, "sum", &run, "theatrical", 3);
        let rows: Vec<String> = (0..3_000)
            .map(|i| {
                let failed = i % 37 == 0;
                with_error += usize::from(failed);
                let extra = if failed {
                    json!({ "error_kind": "timeout" })
                } else {
                    json!({})
                };
                summary_row(i, roles[i % 3], 5, extra)
            })
            .collect();
        std::fs::write(
            sim_root.join(format!("sum/{run}/ledger.jsonl")),
            rows.join("\n") + "\n",
        )
        .unwrap();
    }
    let addr = start(cfg).await;
    let v = rpc(&addr, "ledger.summary", json!({ "tournament_id": "sum" })).await;
    let s = &v["result"];
    assert_eq!(s["calls"], 9_000, "{v}");
    let per_role: u64 = s["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["calls"].as_u64().unwrap())
        .sum();
    assert_eq!(per_role, 9_000);
    assert_eq!(s["errors"], with_error);
    assert!(
        s.get("next_cursor").is_none() && s.get("cursor").is_none(),
        "{v}"
    );
}

#[tokio::test]
async fn ledger_summary_percentiles_leave_failed_calls_out_and_count_unknown_cost() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let sim_root = cfg.paths.sim_runs();
    write_run(&sim_root, "pct", "p1", "theatrical", 3);
    let rows = [
        summary_row(
            1,
            "core_thread",
            800,
            json!({ "first_text_ms": 300, "cost_usd": 0.004 }),
        ),
        summary_row(
            2,
            "core_thread",
            1_200,
            json!({ "first_text_ms": 500, "cost_usd": 0.006 }),
        ),
        summary_row(3, "core_thread", 1_000, json!({ "first_text_ms": 400 })),
        summary_row(
            4,
            "core_thread",
            9_000,
            json!({ "error_kind": "timeout", "cost_usd": 0.0 }),
        ),
    ];
    // A trailing row still being written is left out, as ledger.page leaves it out.
    let text = rows.join("\n") + "\n{\"ts_unix_ms\": 9";
    std::fs::write(sim_root.join("pct/p1/ledger.jsonl"), text).unwrap();
    let addr = start(cfg).await;

    let v = rpc(&addr, "ledger.summary", json!({ "run_id": "p1" })).await;
    let s = &v["result"];
    assert_eq!(s["calls"], 4, "{v}");
    let core = &s["routes"][0];
    assert_eq!(core["role"], "core_thread", "{v}");
    assert_eq!(
        core["duration_p50_ms"], 1_000,
        "failed 9000 ms left out: {v}"
    );
    assert_eq!(core["duration_p95_ms"], 1_200, "{v}");
    assert_eq!(
        core["first_text_p50_ms"], 400,
        "nearest rank of [300,400,500]: {v}"
    );
    let cost = s["cost_usd"].as_f64().unwrap();
    assert!((cost - 0.01).abs() < 1e-12, "{v}");
    assert_eq!(s["uncosted_calls"], 1, "row 3 carries no cost: {v}");

    // An unknown run answers exactly what ledger.page answers.
    let page = rpc(
        &addr,
        "ledger.page",
        json!({ "run_id": "nope", "limit": 5 }),
    )
    .await;
    let sum = rpc(&addr, "ledger.summary", json!({ "run_id": "nope" })).await;
    assert!(page["error"].is_object(), "{page}");
    assert_eq!(sum["error"], page["error"], "{sum}");
}
