//! Test double for the `claude` CLI. Never spends quota; never talks to the network.
//!
//! Behaviour is driven by `$CLAUDE_CONFIG_DIR/fake.json` (each fleet account in a test gets
//! its own config dir):
//! `{"fixture": "<path to recorded stream-json>", "logged_in": true,
//!   "die_on_turn": 0, "delay_ms": 0}`
//!
//! - `--version` prints a version line.
//! - `auth status --json` prints `{"loggedIn": <logged_in>, "authMethod": "claude.ai"}`.
//! - `-p` one-shot: reads the prompt from stdin, replays the fixture.
//! - `-p --input-format stream-json`: for each user line on stdin replays the fixture's turn;
//!   answers each control request with a success `control_response`. With `die_on_turn: n`,
//!   the first process exits without output on its n-th user message (and leaves a marker so
//!   the respawned process behaves).
//!
//! Every invocation appends `{"argv": [...], "api_key_present": bool}` to
//! `$CLAUDE_CONFIG_DIR/calls.jsonl` so tests can assert the argv and the child env.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("2.1.302 (Claude Code)");
        return;
    }
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    let cfg: Value = dir
        .as_ref()
        .and_then(|d| std::fs::read_to_string(d.join("fake.json")).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);

    if let Some(d) = &dir {
        let rec = json!({
            "argv": args,
            "api_key_present": std::env::var_os("ANTHROPIC_API_KEY").is_some(),
            "plugin_dirs_present": std::env::var_os("CLAUDE_CODE_PLUGIN_DIRS").is_some(),
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(d.join("calls.jsonl"))
        {
            // One write per record: `writeln!` may split it, and a test reading the file while
            // a sibling fake is still writing would see half a line.
            let _ = f.write_all(format!("{rec}\n").as_bytes());
        }
    }

    if args.first().map(String::as_str) == Some("auth") {
        let logged_in = cfg
            .get("logged_in")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        println!(
            "{}",
            json!({"loggedIn": logged_in, "authMethod": "claude.ai"})
        );
        return;
    }

    let fixture = cfg
        .get("fixture")
        .and_then(Value::as_str)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    // Replay everything except recorded control responses; those are generated live.
    let turn: Vec<&str> = fixture
        .lines()
        .filter(|l| !l.contains("\"type\":\"control_response\""))
        .collect();
    let delay = cfg.get("delay_ms").and_then(Value::as_u64).unwrap_or(0);
    let stream_input = args
        .windows(2)
        .any(|w| w[0] == "--input-format" && w[1] == "stream-json");

    let stdout = std::io::stdout();
    let replay = |out: &mut dyn Write| {
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        for l in &turn {
            let _ = writeln!(out, "{l}");
        }
        let _ = out.flush();
    };

    if !stream_input {
        let mut prompt = String::new();
        let _ = std::io::stdin().read_to_string(&mut prompt);
        replay(&mut stdout.lock());
        return;
    }

    let die_on_turn = cfg.get("die_on_turn").and_then(Value::as_u64).unwrap_or(0);
    let marker = dir.as_ref().map(|d| d.join("died.marker"));
    let mut turns = 0u64;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("user") => {
                turns += 1;
                if die_on_turn > 0 && turns == die_on_turn {
                    if let Some(m) = &marker {
                        if !m.exists() {
                            let _ = std::fs::write(m, b"died");
                            std::process::exit(3);
                        }
                    }
                }
                replay(&mut stdout.lock());
            }
            Some("control_request") => {
                let id = v.get("request_id").and_then(Value::as_str).unwrap_or("");
                let mut out = stdout.lock();
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"type": "control_response", "response": {
                        "subtype": "success", "request_id": id,
                        "response": {"still_queued": []}}})
                );
                let _ = out.flush();
            }
            _ => {}
        }
    }
}
