# Personas integration

Personas is the operator's desktop agent platform. Shadowrunr connects to it in three ways,
each optional, and none of them can stop a stream, a sim or a command:

| What | Command / config | Direction |
|---|---|---|
| Registration check | `shadowrunr personas status` | Shadowrunr reads Personas |
| Activity | `[personas.activity]` | Shadowrunr sends signed webhook calls to Personas |
| MCP tools for Athena | `shadowrunr mcp` | Personas (or Claude Code) calls Shadowrunr |

Shadowrunr never calls a Personas write endpoint. Registration and the context scan are done
on the Personas side; `.personas/project.json` (committed) records the project id.

## `shadowrunr personas status [--json]`

```
personas  running (port 17400, pid 43640)
project   verified: registered as overlord (06d2c489-...), status active
root      C:\Users\kazda\kiro\overlord
activity  off
mcp       read-only ([personas] mcp_allow_writes = true enables start_sim)
```

The command:

1. reads the Personas handshake: `$PERSONAS_DATA_DIR/local-http.json` when that variable is
   set (an isolated instance), otherwise `~/.personas/local-http.json`, or `[personas]
   handshake`;
2. reads `.personas/project.json` next to `shadowrunr.toml`;
3. calls `GET /dev-tools/projects` on the loopback door, with the `X-Personas-Local-Token`
   header, and finds the project whose `root_path` is this repo.

| Exit | Meaning |
|---|---|
| 0 | Personas is running and this repo is the registered project the file names. |
| 1 | Personas is running, but the project is not verified (`not_registered`, `id_mismatch`, `no_project_file`, `door_error`). |
| 3 | Personas is not running: no handshake, a malformed one, or a stale one whose port does not answer. |

The door token is never printed, in text or in `--json`.

## Activity into Personas

Opt-in. With `[personas.activity]` present, the daemon forwards matching events to a
Personas webhook trigger as HMAC-signed calls.

### Setup

1. In Personas, create a **webhook trigger** on the persona that should hear about streams.
   Webhook triggers always have an HMAC secret: Personas refuses unsigned calls and
   triggers without a secret.
2. Put the trigger's secret in an environment variable, for example
   `SHADOWRUNR_PERSONAS_WEBHOOK_SECRET`. Never put it in `shadowrunr.toml`: the config
   refuses an inline `secret` key.
3. Add the section:

```toml
[personas.activity]
webhook_url = "http://127.0.0.1:9420"
trigger_id = "<the trigger id>"
secret_env = "SHADOWRUNR_PERSONAS_WEBHOOK_SECRET"
# events = ["stream_end", "segment_end", "handover", "tournament_complete", "report_ready"]
```

4. Run `shadowrunr preflight`: the **PF-09** line is green when the trigger answers, and
   amber when activity is configured but invalid, missing its secret, or unreachable. It is
   never red, so Personas never blocks a sim. PF-09 only appears when activity is
   configured.

### What is sent

`POST <webhook_url>/webhook/<trigger_id>`, `Content-Type: application/json`, signed with
`x-signature-256: sha256=<hex HMAC-SHA256(secret, raw body)>`, the scheme Personas verifies
(`engine/webhook.rs`).

```json
{
  "source": "shadowrunr",
  "kind": "stream_end",
  "ts_ms": 1760781212000,
  "project_id": "06d2c489-42ea-41e1-a605-5f313639bca0",
  "run_id": "theatrical.haiku.SEG-01.a3.s7",
  "seq": 412,
  "summary": "theatrical.haiku.SEG-01.a3.s7: stream ended (sim_complete)",
  "detail": { "type": "stream_end", "ts_ms": 600000, "reason": "sim_complete" },
  "dropped": 0
}
```

- `kind` is one of `stream_end`, `segment_end`, `handover`, `tournament_complete`,
  `report_ready` or `moderation`.
- `detail` is the transcript event, or a small summary for tournaments and reports.
- Optional fields (`project_id`, `run_id`, `seq`, `tournament_id`, `report_id`) are left out
  when they do not apply.
- `dropped` counts the events this forwarder has failed to deliver so far, so gaps show on
  the Personas side.

Chat text, prompts, tokens and secrets are never sent.

**Filter.** `events` picks the kinds; the default is the five above. Moderation events with
action `priority:safety`, or rule `character_break` or `persona_leak`, are **always**
forwarded, for visibility. Add `"moderation"` to forward every moderation event.

### Failure behaviour

Forwarding never blocks and is never fatal.

- **Queue.** Events go onto a bounded queue (`queue`, 256). When it is full, the event is
  dropped and counted.
- **Retry.** One background task delivers in order. A network error, a 5xx, or a
  408/422/429 is retried with exponential backoff (`backoff_ms` doubling up to
  `max_backoff_ms`, honouring `Retry-After`), up to `max_attempts`.
- **Refusal.** Any other 4xx is a refusal: a wrong secret (401), an unknown trigger (404) or
  a disabled one (403). It is counted as rejected and not retried; check `trigger_id` and
  the secret.
- **Down.** When an event exhausts its attempts, Personas is marked down for
  `down_cooldown_ms` (30 s). Events arriving meanwhile are dropped and counted with no
  network call, and delivery resumes after the cooldown.

State changes print one line each to stderr. The secret is never printed.

### Where events come from

- **Daemon live runs (`sim.start`).** `stream_end`, `segment_end`, `handover` and the safety
  moderation events, as the run streams.
- **`sim tournament` (CLI)** sends `tournament_complete`.
- **`reconcile` (CLI) and `reconcile.run` (daemon)** send `report_ready`.

The daemon and CLI hook points are listed in the I1 report; until they are wired, only the
API exists for those sources (`integrations::activity::{start_for_daemon, notify_once,
emit}`).

## `shadowrunr mcp`: MCP tools for Athena

`shadowrunr mcp` is a Model Context Protocol server on stdin/stdout. It answers
`initialize`, `ping`, `tools/list` and `tools/call`. Each tool runs the same handler as the
daemon's `POST /rpc`, in-process.

| Tool | `/rpc` method | Notes |
|---|---|---|
| `list_runs` | `runs.page` | Paged; `limit` defaults to 50; pass `next_cursor` back as `cursor`. |
| `get_transcript` | `run.transcript` | Paged lines `{seq, event}`; `limit` defaults to 200. |
| `get_tournament_results` | `tournament.results` | |
| `start_sim` | `sim.start` | **Write.** Disabled unless `[personas] mcp_allow_writes = true`. Spec fields default to `[sim]`. Scripted unless `live: true`. |
| `get_reconcile_report` | `reconcile.report` | |
| `list_plan_options` | `plan.proposals` | |
| `list_directives` | `directive.list` | |
| `preflight` | `preflight.run` | Plus PF-09 when activity is configured. |

**Schemas and errors.** Input schemas are cut from the generated wire contract (`$ref`s
inlined). A failing handler returns an `isError` result carrying the `/rpc` error envelope
(`{"error": {code, message, detail?}}`).

**Framing.** Each message is answered in the framing it arrived in: newline-delimited JSON
(the MCP stdio transport, as Claude Code speaks it) or `Content-Length:` headers (as the
Personas MCP client speaks it). stdout carries only protocol messages; diagnostics go to
stderr.

**Runs started over MCP** live in the MCP process. When stdin closes, the server waits for
them to finish. If the client kills the process (Personas reaps idle stdio sessions after
about 45 s), the run stops where it is and its partial transcript stays on disk. For long
live runs, use the daemon.

### Add it to Personas (MCP gateway credential, stdio)

The Personas MCP client only spawns allowlisted programs (`npx`, `node`, `python`, `uvx`,
`uv`, `deno`, `bun`, `docker`, `podman`, `cargo`). It splits the command on whitespace and
rejects shell metacharacters (`| ; & $ % " ( ) < >` …). So launch through `cargo`, from a
repo path without spaces:

1. Build once, so the first spawn does not compile for minutes (the MCP handshake times out
   after 60 s):

   ```
   cargo build --release -p shadowrunr-daemon
   ```

2. In Personas, go to **Credentials → add MCP server (stdio)**:
   - **command:**
     ```
     cargo run --quiet --release --manifest-path C:\Users\kazda\kiro\overlord\Cargo.toml -p shadowrunr-daemon --bin shadowrunr -- mcp
     ```
   - **working_directory:** `C:\Users\kazda\kiro\overlord` (where `shadowrunr.toml` is
     found);
   - **env_vars:** none needed. Add `CLAUDE_CONFIG_DIR` only if `start_sim` with
     `live: true` should use a specific account.

3. Attach the credential to Athena (or to the gateway that fronts her), then test it with
   the credential's **Test connection**: it should list the eight tools.

`--release` keeps the MCP binary in `target/release`, away from `target/debug/shadowrunr.exe`,
which a running tournament may hold.

### Add it to Claude Code

```
claude mcp add shadowrunr -- cargo run --quiet --release --manifest-path C:\Users\kazda\kiro\overlord\Cargo.toml -p shadowrunr-daemon --bin shadowrunr -- mcp
```
