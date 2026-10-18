//! MCP over in-memory stdio: initialize -> tools/list -> tools/call, both framings, and the
//! write gate. Scripted engine only.

use std::time::Duration;

use tokio::io::{duplex, BufReader, DuplexStream, ReadHalf, WriteHalf};

use super::*;
use crate::e2e::{repo, test_config};

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

fn start(allow_writes: bool, framing: Framing) -> (Client, tempfile::TempDir, Arc<McpServer>) {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path(), repo().join("characters"));
    let factory: EngineFactory = Arc::new(|_: &Config| EngineHandle::scripted());
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
    (client, tmp, server)
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
            "preflight"
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
    assert_eq!(list["result"]["tools"].as_array().map(Vec::len), Some(8));
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
