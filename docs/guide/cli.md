# The `shadowrunr` command line

`shadowrunr` is one binary (crate `shadowrunr-daemon`). It runs the pre-flight checks and
text simulations, calls Reconcile & Plan, and serves the daemon that the console and the
stage connect to.

```
cargo build --release -p shadowrunr-daemon     # target/release/shadowrunr(.exe)
cargo run -p shadowrunr-daemon -- <command>     # or run it straight from the workspace
```

## Configuration

The binary reads `shadowrunr.toml`. It looks in the working directory first, then in each
parent directory; `--config <path>` names a file directly and works with every command.
Without a file, the built-in defaults apply, rooted at the working directory. Relative paths
in the file resolve against the directory that holds it.

```
cp shadowrunr.example.toml shadowrunr.toml
```

| Section | Holds |
|---|---|
| `[engine]` | `EngineConfig`: accounts by `label` and optional `config_dir` (CLAUDE_CONFIG_DIR), `api_key_env` (the env var *name*), `cli_fallbacks`, `scripted`, route overrides. No credentials. |
| `[paths]` | `characters`, `runs`, `data`, `policy`, plus `sim` (calibration, rubric, matrices) and `segments` (segment tables). |
| `[sim]` | Defaults for `sim run` and `sim.start`: character, tone, tier, segment, audience, seed, duration, populations by audience size, judge tiers, concurrency. `max_quota_pause_s` (default 28800, 8 h) caps the total time a tournament waits on quota before it stops (`sim tournament --max-pause` overrides it). |
| `[daemon]` | `port` (7717), `max_live_runs`, `extra_origins`. |

Where things land:
- Simulations go to `<runs>/sim/<tournament_id>/<run_id>/`. Single runs go to
  `<runs>/sim/adhoc/<run_id>/`.
- The engine's process-wide call ledger is `<data>/engine-ledger.jsonl`. Each sim run also
  writes its own `ledger.jsonl`.
- The memory store is `<data>/memory/`. Reconcile reports are `<data>/reconcile/`.
  Directives are `<data>/directives.json`.

`--scripted` (or `[engine] scripted = true`) swaps the live Claude Code fleet for the
deterministic `ScriptedEngine`. It spends no quota, and its content is placeholder text by
design.

## `shadowrunr preflight [--json] [--scripted]`

Runs PF-01..PF-08 and prints one line per check. The command exits non-zero if any check is
red. `--json` prints the `HealthReport[]` instead.

| Check | What it verifies | Red when | Amber when |
|---|---|---|---|
| PF-01 | Engine health: `claude --version`, each account's login, fallbacks | no engine can serve | CLI version differs from the verified one, fallback-only |
| PF-02 | The `[sim] character` package loads; constitution present | the package or constitution is missing or broken | |
| PF-03 | `<data>/memory` is usable and its SQLite index agrees with the markdown truth. Read-only: it creates, writes and rebuilds nothing (the repair is `shadowrunr memory reindex`) | the directory is read-only or cannot be created, or the markdown is unreadable | the index is stale (missing, or its counts disagree with the markdown): the detail names the repair |
| PF-04 | Segment tables are valid; every active segment starts and every descoped segment is refused, both sets read from the segment tables (the one source of the descoped set); the segment data dir exists | any of these fails | |
| PF-05 | `<policy>/platform-rules.json` parses (term-rule regexes included) | it does not parse | absent; or not the file the brain reads (`<characters>/../policy`) |
| PF-06 | `<sim>/calibration.json` is present and valid | it does not parse | only the built-in defaults exist |
| PF-07 | `<runs>/sim` is writable; free disk space | not writable, or under 1 GiB | under 5 GiB |
| PF-08 | Media stack (TTS, avatar, OBS, chat connectors) | | always: "not in media-less phase" |

```
$ shadowrunr preflight --scripted
PF-01  green  scripted engine
PF-02  green  Arch-Lich Malakor (malakor): constitution 3375 chars, tone theatrical, 5 foils, 31 situations
PF-03  green  .../data/memory: index in step with the markdown (0 episodes, 0 facts, 0 tombstones)
PF-04  green  4 tables valid; SEG-01, SEG-02A start; SEG-02B-I, SEG-02B-II refused
PF-05  green  23 term rules; platforms: tiktok, twitch, youtube
PF-06  green  .../sim/calibration.json: 43 sources, 8 archetypes, 4 populations, 5 figures are assumptions
PF-07  green  .../runs/sim writable; 496.1 GiB free
PF-08  amber  not in media-less phase (TTS, avatar, OBS, chat connectors)
```

Other PF-03 details: `.../data/memory: empty store, no index yet` (green), `.../data/memory
not created yet (the store creates it on first open under ...); nothing to index` (green), and
``index stale: run `shadowrunr memory reindex` (.../data/memory: <what disagrees>)`` (amber).
The PF-04 segment names come from the segment tables, so a segment that is descoped or
restored there shows up in this line without a code change.

## `shadowrunr memory reindex [--json]`

Deletes and rebuilds `<data>/memory/index.sqlite` from the markdown truth. This is the repair
PF-03 names when the index is stale; pre-flight only verifies and never rebuilds. Stop the
daemon first, because it holds the index open. `--json` prints the counts as JSON. The command
fails when there is no memory store to reindex.

```
$ shadowrunr memory reindex
.../data/memory: index rebuilt: 0 episodes, 0 facts, 0 tombstones, 0 procedurals, 0 cycles
```

## `shadowrunr sim run`

Runs one simulated stream. Every flag defaults from `[sim]`.

```
shadowrunr sim run --character malakor --tone theatrical --tier haiku --segment SEG-01 \
  --audience 30 --seed 1 --duration 600 [--population growing_channel] [--judge] \
  [--scripted] [--out runs/sim/adhoc]
```

- The run id is `<tone>.<tier>.<segment>.a<audience>.s<seed>`.
- The run lands in `<out>/<run_id>/`: `spec.json`, `instrument.json`, `transcript.jsonl`,
  `ledger.jsonl` and `run.json`. With `--judge` it also writes `verdict.json` and
  `judge.json`.
- If an earlier run with the same id exists, it is moved aside to `<run_id>.attempt-<ts>`.
- The judge never runs on the core tier. The first of `[sim] judge_tiers` that differs from
  the core tier judges the run.
- Descoped segments are refused.

```
$ shadowrunr sim run --scripted --audience 30 --duration 120
run_dir: .../runs/sim/adhoc/theatrical.haiku.SEG-01.a30.s1
transcript: 52 events
verdict: not judged
```

## `shadowrunr sim tournament`

Runs, or resumes, the tournament a matrix file defines, then summarises it.

```
shadowrunr sim tournament --matrix sim/matrices/pilot.toml [--out runs/sim] \
  [--concurrency 2] [--max-runs 10] [--no-judge] [--scripted] [--max-pause 28800]
shadowrunr sim tournament sim/matrices/pilot.toml          # positional form
shadowrunr sim tournament sim/matrices/pilot.toml --plan-only
```

- `--out` is the root directory. The tournament lands in `<out>/<matrix_id>-<base_hash10>/`,
  so a changed instrument (rubric, segment tables, calibration, character package) starts a
  new directory instead of mixing results.
- A resumed tournament skips runs that are complete for the same instrument (judged, or void
  for a content reason). It re-judges `judge_pending` runs without re-simulating them, and
  re-runs `engine_failed`, partial and stale runs (a changed tone file makes a run stale),
  moving the old directory aside first.
- When every account is cooling, the tournament pauses until one is back instead of burning
  runs. The total pause is capped by `--max-pause <seconds>`, which defaults to
  `[sim] max_quota_pause_s` (28800 = 8 h). Past the cap it stops with its summary
  (`"stopped": "pause_cap"`) and exit code 4; a later run resumes it.
- The command prints a JSON summary: planned, skipped, executed and failed runs, the result
  counts, the runs left `engine_failed` or `judge_pending` (not results, never counted as
  void), `paused_ms`, `pauses`, `stopped` when it stopped early, and `exit_code`.
- `--plan-only` prints the action the planner would take for every run (`skip`, `rejudge` or
  `run`) with its reason (`new`, `complete`, `void_kept`, `stale`, `partial`,
  `engine_failed`, `judge_pending`) and the counts. It builds no engine, runs nothing, spends
  no quota and writes nothing.

| Exit code | Meaning |
|---|---|
| 0 | every planned run is judged, or void for a content reason |
| 1 | error (bad matrix, I/O) |
| 4 | stopped: the total quota pause cap (`--max-pause` / `[sim] max_quota_pause_s`) was exceeded |
| 5 | finished with `engine_failed` or `judge_pending` runs left (a resume re-runs or re-judges them) |

```
$ shadowrunr sim tournament sim/matrices/pilot.toml --plan-only
{
  "tournament_id": "pilot-0dd890a1ad",
  "dir": ".../runs/sim/pilot-0dd890a1ad",
  "planned": 4,
  "skip": 0,
  "rejudge": 0,
  "run": 4,
  "by_reason": { "new": 4 },
  "runs": [
    { "run_id": "theatrical.haiku.SEG-01.a3.s1", "action": "run", "reason": "new" },
    ...
  ]
}
```

## `shadowrunr sim summarize <dir> [--json]`

Aggregates a tournament directory. It writes `results.json`, `heatmap.md` (worst first) and
`report.md`. `--json` prints `results.json` to stdout.

```
shadowrunr sim summarize runs/sim/pilot-d1ef98d3a6
```

## `shadowrunr reconcile --tournament <id> [--scripted] [--json] [--dry-run]`

Runs Reconcile & Plan (`shadowrunr-reconcile`) over `<runs>/sim/<id>/`:
1. deterministic metrics;
2. the Reconciler's synthesis on the `reconciler` route;
3. mechanical verification of every evidence ref and diff.

The report is stored in `<data>/reconcile/<report_id>.json`. If the LLM synthesis fails (it
always does on `--scripted`), the deterministic report is still stored, with the failure in
its summary and no plan options. The transcript excerpts in the synthesis prompt carry viewer
chat, so they sit inside an untrusted-data fence; a reply that echoes the fence token counts
as a failed synthesis.

Only results are loaded: `engine_failed` and `judge_pending` runs, moved-aside copies and
incomplete runs are left out and counted in the summary. `--dry-run` loads the tournament the
same way and prints what Reconcile would analyse and what it would leave out, as JSON. It
builds no engine, calls nothing and writes nothing.

```
$ shadowrunr reconcile --tournament pilot-d1ef98d3a6 --dry-run
{
  "tournament_id": "pilot-d1ef98d3a6",
  "loaded": 2,
  "judged": 2,
  "excluded": {}
}
```

`excluded` lists only the non-zero counts: `engine_failed`, `judge_pending`, `copies`,
`incomplete`.

```
$ shadowrunr reconcile --tournament pilot-d1ef98d3a6
report_id: <report_id>
stored: .../data/reconcile/<report_id>.json
plan options: <n>
<summary>
```

## `shadowrunr daemon [--port 7717] [--scripted]`

Serves the protocol in `docs/spec/contract.md` § Daemon protocol, bound to `127.0.0.1` only.

```
$ shadowrunr daemon
shadowrunr daemon on http://127.0.0.1:7717 (live engine): POST /rpc, GET /events
```

### Browser access

Allowed origins:
- `http://localhost:*`;
- `tauri://localhost`;
- `http://tauri.localhost` (the Tauri v2 window origin on Windows);
- any exact `[daemon] extra_origins`.

The daemon refuses a request or WebSocket upgrade that carries any other `Origin` with 403.
`/rpc` also requires `content-type: application/json`, so a cross-origin page cannot send a
"simple" request that skips the CORS preflight. Non-browser clients send no `Origin` and are
accepted.

### `POST /rpc`

The request is `{"method": "<name>", "params": {...}}`. The answer is `{"result": ...}` or
`{"error": {"code", "message"}}` (HTTP 200).

Methods:
- `preflight.run`
- `runs.list`, `run.transcript`, `run.verdict`
- `tournament.list`, `tournament.results`
- `sim.start`
- `reconcile.run`, `reconcile.report`
- `plan.proposals`, `plan.choose`
- `directive.add`, `directive.list`, `directive.deactivate` (`{ "id" }` returns the
  directive with `active: false`)
- `lord.chat`

Notes:
- **Run ids:** `run_id` may be bare or `<tournament_id>/<run_id>`. A bare id that exists in
  several tournaments is rejected as ambiguous.
- **Starting a sim:** `sim.start` runs pre-flight first and refuses with `preflight_red`
  while any check is red. It also refuses descoped segments, and returns `busy` beyond
  `max_live_runs`. An empty `spec.run_id` gets a generated one. The run lands in
  `<runs>/sim/adhoc/`.

Error codes:
- `invalid_params`
- `not_found`
- `method_not_found`
- `preflight_red`
- `busy`
- `io`
- `engine`
- `synthesis`
- `parse_error` (HTTP 400)
- `unsupported_media_type` (HTTP 415)
- `forbidden_origin` (HTTP 403)

```
curl -s http://127.0.0.1:7717/rpc -H 'content-type: application/json' \
  -d '{"method":"preflight.run","params":{}}'
curl -s http://127.0.0.1:7717/rpc -H 'content-type: application/json' \
  -d '{"method":"sim.start","params":{"judge":false,"spec":{"run_id":"","character":"malakor","tone_variant":"theatrical","core_tier":"haiku","core_effort":"low","segment":"SEG-01","audience_size":30,"seed":1,"duration_s":600,"population":"growing_channel"}}}'
```

### `GET /events` (WebSocket)

The client sends `{"subscribe": "<run_id>|live", "speed": 1.0}`. Each subscribe message
replaces the current source. The server pushes one `StreamEvent` JSON per text message.

| `subscribe` | Source |
|---|---|
| a `sim.start` run that is in flight | Its events so far, then each new one as it is written. `speed` does not apply. |
| any stored run id (or `<tournament_id>/<run_id>`) | A replay of its `transcript.jsonl`, with event gaps (`ts_ms`) divided by `speed` (> 0). |
| `live` | The `sim.start` run in flight, from its start, then each next run. |

If a source fails (unknown run, bad speed, a live run that errors), the server sends one
`{"error": {"code", "message"}}` frame. That frame is not a `StreamEvent`; clients that only
read events ignore it.

Live events arrive in the batches the simulator flushes to `transcript.jsonl`, not one at a
time.

## Character rules and moderation lists

```
shadowrunr character rules malakor                          # list optional rules and their state
shadowrunr character rules malakor --disable crisis-handoff  # operator override in <data>/characters/malakor/rules.json
shadowrunr policy install-list awdev1-better-profane-words   # downloads into policy/local/ (git-ignored, GPL-3.0, never committed)
```

## Personas

```
shadowrunr personas status [--json]   # verifies the Personas registration (exit 3 = Personas not running)
shadowrunr mcp                        # stdio MCP server for Athena (see docs/guide/personas.md)
```
