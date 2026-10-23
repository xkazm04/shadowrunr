//! MCP over in-memory stdio: initialize -> tools/list -> tools/call, both framings, and the
//! write gate. Scripted engine only.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{duplex, BufReader, DuplexStream, ReadHalf, WriteHalf};

use super::*;
use crate::e2e::{repo, test_config};
use crate::server::AppState;
use shadowrunr_types::rpc::RpcRequest;

struct Client {
    r: BufReader<ReadHalf<DuplexStream>>,
    w: WriteHalf<DuplexStream>,
    framing: Framing,
    next_id: u64,
}

impl Client {
    async fn send(&mut self, msg: Value) {
        write_message(&mut self.w, self.framing, &msg)
            .await
            .unwrap();
    }

    async fn recv(&mut self) -> Value {
        let (framing, body) =
            tokio::time::timeout(Duration::from_secs(30), read_message(&mut self.r))
                .await
                .expect("reply in time")
                .unwrap()
                .expect("a reply");
        assert_eq!(framing, self.framing, "answered in the request's framing");
        serde_json::from_slice(&body).unwrap()
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        let v = self.recv().await;
        assert_eq!(v["id"], id, "{v}");
        v
    }

    /// A tool's JSON result (the text content), asserting it did not fail.
    async fn tool(&mut self, name: &str, args: Value) -> Value {
        let v = self
            .request("tools/call", json!({ "name": name, "arguments": args }))
            .await;
        assert_eq!(v["result"]["isError"], false, "{v}");
        serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

/// A port nothing listens on: the MCP server finds no daemon there.
fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn start(allow_writes: bool, framing: Framing) -> (Client, tempfile::TempDir, Arc<McpServer>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = test_config(tmp.path(), repo().join("characters"));
    // Never the developer's own daemon on the default port.
    cfg.daemon.port = closed_port();
    let factory: EngineFactory = Arc::new(|_: &Config| EngineHandle::scripted());
    let (client, server) = serve(cfg, allow_writes, framing, factory);
    (client, tmp, server)
}

fn serve(
    cfg: Config,
    allow_writes: bool,
    framing: Framing,
    factory: EngineFactory,
) -> (Client, Arc<McpServer>) {
    let server = Arc::new(McpServer::new(cfg, allow_writes, factory));
    let (client_end, server_end) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server_end);
    let s = server.clone();
    tokio::spawn(async move { s.serve(sr, sw).await });
    let (cr, cw) = tokio::io::split(client_end);
    let client = Client {
        r: BufReader::new(cr),
        w: cw,
        framing,
        next_id: 0,
    };
    (client, server)
}

fn has_ref(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.contains_key("$ref") || o.values().any(has_ref),
        Value::Array(a) => a.iter().any(has_ref),
        _ => false,
    }
}

#[tokio::test]
async fn initialize_list_and_call_over_line_framing() {
    let (mut c, _tmp, _s) = start(false, Framing::Lines);
    let init = c
        .request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }),
        )
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "shadowrunr");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // A notification gets no reply: the next reply read is the ping's.
    c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    let pong = c.request("ping", json!({})).await;
    assert_eq!(pong["result"], json!({}));

    let list = c.request("tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "list_runs",
            "get_transcript",
            "get_tournament_results",
            "start_sim",
            "get_reconcile_report",
            "list_plan_options",
            "list_directives",
            "preflight",
            "moderator_status",
            "moderator_rules_list",
            "moderator_rules_set",
            "moderator_rules_test",
            "moderator_mode_set",
            "moderator_log",
            "moderator_unban",
            "twitch_connect",
            "twitch_disconnect",
            "moderator_character_get",
            "moderator_character_set"
        ]
    );
    for t in tools {
        let s = &t["inputSchema"];
        assert_eq!(s["type"], "object", "{t}");
        assert!(!has_ref(s), "schemas are self-contained: {t}");
    }
    let runs_schema = &tools[0]["inputSchema"];
    assert!(runs_schema["properties"]["filter"]["properties"]["core_tier"].is_object());
    assert!(!runs_schema["required"]
        .as_array()
        .is_some_and(|r| r.contains(&json!("limit"))));
    // WP14b: an agent reading directives learns that proposed entries are not live.
    let directives_desc = tools[6]["description"].as_str().unwrap();
    assert!(
        directives_desc.contains("proposed: true"),
        "{directives_desc}"
    );
    let sim_schema = &tools[3]["inputSchema"];
    assert!(sim_schema["properties"]["spec"]["properties"]["segment"].is_object());
    assert!(sim_schema["properties"]["live"].is_object());

    // Read tools against the fixture tournament.
    let page = c.tool("list_runs", json!({})).await;
    let items = page["items"].as_array().unwrap();
    assert!(items.iter().any(|r| r["run_id"] == "fixture-run"), "{page}");

    let first = c
        .tool(
            "get_transcript",
            json!({ "run_id": "fixture-run", "limit": 4 }),
        )
        .await;
    assert_eq!(first["total"], 11);
    assert_eq!(first["items"].as_array().unwrap().len(), 4);
    assert_eq!(first["items"][0]["seq"], 1);
    let cursor = first["next_cursor"].as_str().unwrap().to_string();
    let rest = c
        .tool(
            "get_transcript",
            json!({ "run_id": "fixture-run", "cursor": cursor, "limit": 500 }),
        )
        .await;
    assert_eq!(rest["items"].as_array().unwrap().len(), 7);
    assert_eq!(rest["items"][0]["seq"], 5);
    assert!(rest.get("next_cursor").is_none());

    let directives = c.tool("list_directives", json!({})).await;
    assert!(directives.is_array());
    let pf = c.tool("preflight", json!({})).await;
    assert_eq!(
        pf.as_array().map(Vec::len),
        Some(8),
        "no PF-09 without [personas]"
    );

    // A handler error is a tool result with isError and the /rpc envelope.
    let missing = c
        .request(
            "tools/call",
            json!({ "name": "get_reconcile_report", "arguments": { "report_id": "nope" } }),
        )
        .await;
    assert_eq!(missing["result"]["isError"], true, "{missing}");
    let env: Value =
        serde_json::from_str(missing["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(env["error"]["code"].is_string(), "{env}");
}

#[tokio::test]
async fn protocol_errors() {
    let (mut c, _tmp, _s) = start(false, Framing::Lines);
    let unknown_tool = c
        .request("tools/call", json!({ "name": "rm_rf", "arguments": {} }))
        .await;
    assert_eq!(unknown_tool["error"]["code"], -32602);
    let unknown_method = c.request("resources/list", json!({})).await;
    assert_eq!(unknown_method["error"]["code"], -32601);
    c.w.write_all(b"{not json\n").await.unwrap();
    let parse = c.recv().await;
    assert_eq!(parse["error"]["code"], -32700);
    assert_eq!(parse["id"], Value::Null);
    let old = c
        .request("initialize", json!({ "protocolVersion": "1999-01-01" }))
        .await;
    assert_eq!(old["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
}

#[tokio::test]
async fn content_length_framing_like_the_personas_client() {
    let (mut c, _tmp, _s) = start(false, Framing::ContentLength);
    let init = c
        .request(
            "initialize",
            json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "personas-playground", "version": "1.0.0" } }),
        )
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    let list = c.request("tools/list", json!({})).await;
    assert_eq!(list["result"]["tools"].as_array().map(Vec::len), Some(19));
    let page = c
        .tool(
            "list_runs",
            json!({ "tournament_id": "fixture-t", "limit": 10 }),
        )
        .await;
    assert_eq!(page["items"][0]["run_id"], "fixture-run", "{page}");
    // The fixture tournament was never summarized: a not_found tool error, not a crash.
    let v = c
        .request(
            "tools/call",
            json!({ "name": "get_tournament_results", "arguments": { "tournament_id": "fixture-t" } }),
        )
        .await;
    assert_eq!(v["result"]["isError"], true, "{v}");
}

#[tokio::test]
async fn start_sim_is_refused_unless_writes_are_allowed() {
    let (mut c, _tmp, server) = start(false, Framing::Lines);
    let v = c
        .request(
            "tools/call",
            json!({ "name": "start_sim", "arguments": { "spec": { "duration_s": 30 } } }),
        )
        .await;
    assert_eq!(v["result"]["isError"], true, "{v}");
    let env: Value =
        serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(env["error"]["detail"]["reason"], "writes_disabled", "{env}");
    assert!(server.tools().running().is_empty(), "nothing was started");
}

#[tokio::test]
async fn start_sim_runs_scripted_when_allowed() {
    let (mut c, _tmp, server) = start(true, Framing::Lines);
    let started = c
        .tool(
            "start_sim",
            json!({ "spec": { "run_id": "mcp-1", "audience_size": 3, "duration_s": 60 } }),
        )
        .await;
    assert_eq!(started["run_id"], "mcp-1");
    assert_eq!(started["engine"], "scripted");
    tokio::time::timeout(Duration::from_secs(60), server.tools().wait_idle())
        .await
        .expect("the scripted run finishes");
    let t = c.tool("get_transcript", json!({ "run_id": "mcp-1" })).await;
    let items = t["items"].as_array().unwrap();
    assert_eq!(items[0]["event"]["type"], "stream_start");
    assert!(
        items.iter().any(|i| i["event"]["type"] == "stream_end"),
        "{t}"
    );
}

/// Every file under `root`: path, bytes and mtime.
fn tree(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::metadata(&p).unwrap();
            if m.is_dir() {
                stack.push(p.clone());
                out.push((p, Vec::new(), m.modified().unwrap()));
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap(), m.modified().unwrap()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[tokio::test]
async fn the_read_only_preflight_tool_makes_no_writes() {
    let (mut c, tmp, _s) = start(false, Framing::Lines);
    c.request(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }),
    )
    .await;
    // A memory store with an index, as a daemon leaves it.
    {
        let store =
            shadowrunr_brain::MemoryStore::open(&tmp.path().join("data").join("memory")).unwrap();
        store
            .write_episode(&shadowrunr_types::Episode {
                id: "ep_1".into(),
                ts_ms: 1_000,
                stream_id: "s1".into(),
                kind: shadowrunr_types::EpisodeKind::Stream,
                text: "Gorbo begged for a title.".into(),
                importance: 3,
            })
            .unwrap();
    }
    assert!(!super::super::mcp_tools::find("preflight").unwrap().write);
    let before = tree(tmp.path());
    for _ in 0..2 {
        let pf = c.tool("preflight", json!({})).await;
        let pf03 = pf
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["check"] == "PF-03")
            .unwrap()
            .clone();
        assert_eq!(pf03["state"], "green", "{pf03}");
    }
    assert_eq!(tree(tmp.path()), before, "the preflight tool wrote to disk");
}

// ── one admission door: writes and preflight go through a running daemon ───────────────

/// The `/rpc` error envelope of a failed tool call.
async fn tool_error(c: &mut Client, name: &str, args: Value) -> Value {
    let v = c
        .request("tools/call", json!({ "name": name, "arguments": args }))
        .await;
    assert_eq!(v["result"]["isError"], true, "{v}");
    serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// A factory that counts how many engines the MCP server built.
fn counting_factory() -> (EngineFactory, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let k = n.clone();
    let f: EngineFactory = Arc::new(move |_: &Config| {
        k.fetch_add(1, Ordering::SeqCst);
        EngineHandle::scripted()
    });
    (f, n)
}

struct Daemon {
    state: Arc<AppState>,
    client: Client,
    server: Arc<McpServer>,
    built: Arc<AtomicUsize>,
    cfg: Config,
    _tmp: tempfile::TempDir,
}

impl Daemon {
    fn built(&self) -> usize {
        self.built.load(Ordering::SeqCst)
    }

    /// The run the daemon's live registry registered last.
    fn latest_run(&self) -> Option<String> {
        self.state
            .live
            .latest()
            .borrow()
            .as_ref()
            .map(|r| r.run_id.clone())
    }
}

/// A daemon (`edit` adjusts its config, `live` gives it a live fleet) served on a free port,
/// and an MCP server with writes allowed whose config points at it.
async fn with_daemon(live: bool, edit: impl FnOnce(&mut Config)) -> Daemon {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = test_config(tmp.path(), repo().join("characters"));
    edit(&mut cfg);
    let engine = if live {
        // A fleet is built, never called: the call is refused before any engine work.
        let mut c = cfg.clone();
        c.engine.scripted = false;
        EngineHandle::build(&c, false)
    } else {
        EngineHandle::scripted()
    };
    assert_eq!(engine.is_scripted(), !live);
    let state = AppState::new(cfg.clone(), engine);
    let addr = crate::e2e::start_state(state.clone()).await;
    cfg.daemon.port = addr.rsplit(':').next().unwrap().parse().unwrap();
    let (factory, built) = counting_factory();
    let (client, server) = serve(cfg.clone(), true, Framing::Lines, factory);
    Daemon {
        state,
        client,
        server,
        built,
        cfg,
        _tmp: tmp,
    }
}

#[tokio::test]
async fn start_sim_beside_a_daemon_sandbox_session_is_refused() {
    use crate::sandbox::tests::{fake_judge, fake_run, hosting, params};
    let mut d = with_daemon(false, |_| {}).await;
    let session = crate::sandbox::start_with(
        d.state.clone(),
        params(false),
        hosting(
            fake_run(2, Duration::from_secs(60), None, false),
            fake_judge(Some(0)),
        ),
    )
    .await
    .unwrap()
    .session_id;
    assert_eq!(d.state.sandbox.running().as_deref(), Some(session.as_str()));
    let env = tool_error(
        &mut d.client,
        "start_sim",
        json!({ "spec": { "run_id": "m1", "audience_size": 3 } }),
    )
    .await;
    assert_eq!(env["error"]["code"], "conflict", "{env}");
    assert!(
        d.server.tools().running().is_empty(),
        "no run in the MCP process"
    );
    assert!(d.state.live.get("m1").is_none());
}

#[tokio::test]
async fn start_sim_with_a_daemon_run_id_in_flight_is_refused_and_moves_nothing() {
    let mut d = with_daemon(false, |_| {}).await;
    let _x = d.state.live.register("x", 4).unwrap();
    let run_dir = d.cfg.paths.sim_runs().join(&d.cfg.sim.adhoc_dir).join("x");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("transcript.jsonl"), "").unwrap();
    let env = tool_error(
        &mut d.client,
        "start_sim",
        json!({ "spec": { "run_id": "x", "audience_size": 3 } }),
    )
    .await;
    assert_eq!(env["error"]["code"], "conflict", "{env}");
    assert!(
        run_dir.join("transcript.jsonl").is_file(),
        "the daemon's run dir was moved aside"
    );
}

#[tokio::test]
async fn start_sim_obeys_the_daemons_max_live_runs() {
    let mut d = with_daemon(false, |c| c.daemon.max_live_runs = 1).await;
    let _other = d.state.live.register("other", 1).unwrap();
    let env = tool_error(
        &mut d.client,
        "start_sim",
        json!({ "spec": { "run_id": "m2", "audience_size": 3 } }),
    )
    .await;
    assert_eq!(env["error"]["code"], "busy", "{env}");
    assert!(d.server.tools().running().is_empty());
}

#[tokio::test]
async fn start_sim_runs_in_the_daemon_when_one_answers() {
    let mut d = with_daemon(false, |_| {}).await;
    let started = d
        .client
        .tool(
            "start_sim",
            json!({ "spec": { "run_id": "d1", "audience_size": 3, "duration_s": 60 } }),
        )
        .await;
    assert_eq!(started["run_id"], "d1", "{started}");
    assert_eq!(started["host"], "daemon", "{started}");
    assert_eq!(started["engine"], "scripted", "{started}");
    // Registered in the daemon's live registry (a scripted run may already have finished).
    assert!(
        d.state.live.get("d1").is_some() || d.latest_run().as_deref() == Some("d1"),
        "the daemon never saw run d1"
    );
    assert!(
        d.server.tools().running().is_empty(),
        "the MCP holds no run"
    );
    tokio::time::timeout(Duration::from_secs(1), d.server.tools().wait_idle())
        .await
        .expect("wait_idle returns at once");
    assert_eq!(d.built(), 0);
}

#[tokio::test]
async fn live_start_sim_against_a_scripted_daemon_is_an_engine_mismatch() {
    let mut d = with_daemon(false, |_| {}).await;
    let env = tool_error(
        &mut d.client,
        "start_sim",
        json!({ "live": true, "spec": { "run_id": "l1", "audience_size": 3 } }),
    )
    .await;
    assert_eq!(env["error"]["detail"]["reason"], "engine_mismatch", "{env}");
    assert_eq!(d.built(), 0, "no second fleet");
    assert!(d.latest_run().is_none(), "nothing registered in the daemon");
    assert!(d.server.tools().running().is_empty());
}

#[tokio::test]
async fn default_start_sim_against_a_live_daemon_is_an_engine_mismatch() {
    let mut d = with_daemon(true, |_| {}).await;
    let env = tool_error(
        &mut d.client,
        "start_sim",
        json!({ "spec": { "run_id": "s1", "audience_size": 3 } }),
    )
    .await;
    assert_eq!(env["error"]["detail"]["reason"], "engine_mismatch", "{env}");
    assert!(d.latest_run().is_none(), "no run registered in the daemon");
    assert!(!d.state.live.any_in_flight());
    assert_eq!(d.built(), 0, "no engine built in the MCP process");
    assert!(d.server.tools().running().is_empty());
}

#[tokio::test]
async fn preflight_is_the_daemons_preflight() {
    let mut d = with_daemon(false, |_| {}).await;
    let pf = d.client.tool("preflight", json!({})).await;
    let theirs = crate::rpc::dispatch(
        &d.state,
        RpcRequest {
            method: "preflight.run".into(),
            params: json!({}),
        },
    )
    .await
    .unwrap();
    let checks = |v: &Value| -> Vec<(Value, Value)> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|r| (r["check"].clone(), r["state"].clone()))
            .collect()
    };
    assert_eq!(checks(&pf), checks(&theirs), "{pf}");
    assert_eq!(d.built(), 0, "no engine built in the MCP process");
}

#[tokio::test]
async fn without_a_daemon_start_sim_runs_in_process() {
    let (mut c, _tmp, server) = start(true, Framing::Lines);
    let started = c
        .tool(
            "start_sim",
            json!({ "spec": { "run_id": "solo", "audience_size": 3, "duration_s": 60 } }),
        )
        .await;
    assert_eq!(started["host"], "mcp", "{started}");
    assert_eq!(started["engine"], "scripted", "{started}");
    tokio::time::timeout(Duration::from_secs(60), server.tools().wait_idle())
        .await
        .expect("the scripted run finishes");
}

// ── the moderator tools ──────────────────────────────────────────────────────────────────

/// An MCP server with no daemon behind it; `mod_writes` is `[moderator] mcp_allow_writes`.
fn start_mod(mod_writes: bool) -> (Client, tempfile::TempDir, Arc<McpServer>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = test_config(tmp.path(), repo().join("characters"));
    cfg.daemon.port = closed_port();
    cfg.moderator.mcp_allow_writes = mod_writes;
    let factory: EngineFactory = Arc::new(|_: &Config| EngineHandle::scripted());
    // The personas flag is on: it must not open the moderator's writes.
    let (client, server) = serve(cfg, true, Framing::Lines, factory);
    (client, tmp, server)
}

fn kys_rule() -> Value {
    json!({ "id": "no-kys", "name": "telling people to die", "kind": "pattern",
            "pattern": r"\bkys\b", "action": "timeout", "timeout_s": 600, "enabled": true })
}

#[tokio::test]
async fn moderator_writes_are_refused_unless_allowed_and_reads_work() {
    let (mut c, tmp, _s) = start_mod(false);
    let writes = [
        ("moderator_rules_set", json!({ "rules": [kys_rule()] })),
        ("moderator_mode_set", json!({ "mode": "armed" })),
        (
            "moderator_unban",
            json!({ "platform": "twitch", "viewer_id": "1" }),
        ),
        ("twitch_connect", json!({})),
        ("twitch_disconnect", json!({})),
        ("moderator_character_set", json!({ "name": "X" })),
    ];
    for (name, args) in writes {
        let e = tool_error(&mut c, name, args).await;
        assert_eq!(
            e["error"]["detail"]["reason"], "writes_disabled",
            "{name}: {e}"
        );
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("[moderator] mcp_allow_writes"),
            "{e}"
        );
    }
    let status = c.tool("moderator_status", json!({})).await;
    assert_eq!(status["mode"], "shadow");
    assert_eq!(status["platforms"][0]["platform"], "twitch");
    let rules = c.tool("moderator_rules_list", json!({})).await;
    assert!(
        !rules["rules"].as_array().unwrap().is_empty(),
        "seeded from the policy file"
    );
    let log = c.tool("moderator_log", json!({})).await;
    assert_eq!(log["entries"], json!([]));
    let test = c
        .tool("moderator_rules_test", json!({ "text": "just kys" }))
        .await;
    assert_eq!(test["disclosure"], false);
    assert_eq!(test["decisions"][0]["rule_id"], "self-harm-incitement");
    assert!(!tmp
        .path()
        .join("data")
        .join("moderator")
        .join("state.json")
        .exists());
}

#[tokio::test]
async fn moderator_writes_without_a_daemon_land_in_process() {
    let (mut c, _tmp, _s) = start_mod(true);
    let set = c
        .tool("moderator_rules_set", json!({ "rules": [kys_rule()] }))
        .await;
    assert_eq!(set["rules"][0]["id"], "no-kys");
    let listed = c.tool("moderator_rules_list", json!({})).await;
    assert_eq!(listed["rules"].as_array().unwrap().len(), 1);
    let bad = tool_error(
        &mut c,
        "moderator_rules_set",
        json!({ "rules": [{ "id": "broken", "name": "b", "kind": "pattern", "pattern": "(", "action": "delete", "enabled": true }] }),
    )
    .await;
    assert_eq!(bad["error"]["code"], "invalid_params", "{bad}");
    let mode = c
        .tool("moderator_mode_set", json!({ "mode": "armed" }))
        .await;
    assert_eq!(mode["mode"], "armed");
    let tw = tool_error(&mut c, "twitch_connect", json!({})).await;
    assert_eq!(tw["error"]["detail"]["reason"], "no_daemon", "{tw}");
    let ch = tool_error(&mut c, "moderator_character_set", json!({ "name": "X" })).await;
    assert_eq!(ch["error"]["detail"]["reason"], "not_available", "{ch}");
}

#[tokio::test]
async fn moderator_writes_go_to_the_running_daemon() {
    let mut d = with_daemon(false, |c| {
        c.moderator.mcp_allow_writes = true;
        c.moderator.character = "malakor".into();
    })
    .await;
    d.client
        .tool("moderator_rules_set", json!({ "rules": [kys_rule()] }))
        .await;
    let mode = d
        .client
        .tool("moderator_mode_set", json!({ "mode": "armed" }))
        .await;
    assert_eq!(mode["mode"], "armed");
    // The daemon's own hub holds the new list and mode.
    assert_eq!(d.state.moderator.rules().rules[0].id, "no-kys");
    assert_eq!(
        d.state.moderator.mode(),
        shadowrunr_types::moderation::ModMode::Armed
    );
    let ch = d.client.tool("moderator_character_get", json!({})).await;
    assert_eq!(ch["id"], "malakor", "{ch}");
    assert!(ch["name"].is_string(), "{ch}");
    assert!(ch.get("files").is_none(), "only the identity fields: {ch}");
}
