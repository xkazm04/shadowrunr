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
| `[engine]` | `EngineConfig`: accounts by `label`, optional `config_dir` (CLAUDE_CONFIG_DIR) and `fallback` (reached only while every other account cools), `api_key_env` (the env var *name*), `cli_fallbacks`, `scripted`, route overrides. No credentials. |
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

## `shadowrunr preflight [--json] [--scripted] [--fix [--download]]`

Runs PF-01..PF-08 and prints one line per check. The command exits non-zero if any check is
red. `--json` prints the `HealthReport[]` instead. Pre-flight is read-only: without `--fix` it
creates, writes and repairs nothing.

Each amber or red line ends with its remedy, authored beside the check:
- `fix: shadowrunr preflight --fix`: an applied remedy that `--fix` runs for you (PF-03 stale
  index: `memory reindex`).
- `fix: shadowrunr preflight --fix --download`: an applied remedy that downloads (PF-05
  placeholder-only lists with a known source: `policy install-list <id>`).
- `fix: <command or what to do>`: guided; you act (PF-02 package, PF-05 absent or not the file
  the brain reads, PF-06 calibration absent).
- `external: <what>`: outside shadowrunr (PF-01 engine, PF-07 disk, PF-08 media stack).

| Check | What it verifies | Red when | Amber when |
|---|---|---|---|
| PF-01 | Engine health: `claude --version`, each account's login, fallbacks | no engine can serve | CLI version differs from the verified one, fallback-only |
| PF-02 | The `[sim] character` package loads; constitution present | the package or constitution is missing or broken | |
| PF-03 | `<data>/memory` is usable and its SQLite index agrees with the markdown truth. Read-only: it creates, writes and rebuilds nothing (the repair is `shadowrunr memory reindex`) | the directory is read-only or cannot be created, or the markdown is unreadable | the index is stale (missing, or its counts disagree with the markdown): the detail names the repair |
| PF-04 | Segment tables are valid; every active segment starts and every descoped segment is refused, both sets read from the segment tables (the one source of the descoped set); the segment data dir exists | any of these fails | |
| PF-05 | `<policy>/platform-rules.json` parses (term-rule regexes included) | it does not parse | absent; or not the file the brain reads (`<characters>/../policy`) |
| PF-06 | `<sim>/calibration.json` is present and valid (population archetypes and cited source ids resolve); the `[sim] character` bank reaches its `bait_tactics` | it does not parse, or a tactic binds a situation the bank lacks | only the built-in defaults exist; or a bait tactic is bound to no situation (the detail names the count and the first ids) |
| PF-07 | `<runs>/sim` is writable; free disk space | not writable, or under 1 GiB | under 5 GiB |
| PF-08 | Media stack (TTS, avatar, OBS, chat connectors) | | always: "not in media-less phase" |

```
$ shadowrunr preflight --scripted
PF-01  green  scripted engine
PF-02  green  Arch-Lich Malakor (malakor): constitution 3375 chars, tone theatrical, 5 foils, 31 situations
PF-03  green  .../data/memory: index in step with the markdown (0 episodes, 0 facts, 0 tombstones)
PF-04  green  4 tables valid; SEG-01, SEG-02A start; SEG-02B-I, SEG-02B-II refused
PF-05  green  23 term rules; platforms: tiktok, twitch, youtube
PF-06  amber  .../sim/calibration.json: 43 sources, 8 archetypes, 4 populations, 9 figures are assumptions; 10 of 14 bait tactics unreached by the malakor bank: say-x-dare, obfuscated-slur, payload-split, loaded-history-question, villain-licence, ...
PF-07  green  .../runs/sim writable; 496.1 GiB free
PF-08  amber  not in media-less phase (TTS, avatar, OBS, chat connectors)  external: the media stack is outside the media-less phase
```

`--fix` applies the applied remedies, re-runs every check and prints a before -> after board.
A row's state shows `before -> after` when it changed. The fix's success claim is the re-probe:
a fix whose row did not improve is reported as `not fixed`.
- **Reindex** refuses while a daemon answers `/rpc` on the configured `[daemon] port`, since it
  holds the index open: `not fixed: the daemon on 127.0.0.1:<port> holds the index; stop it
  first`.
- **List installs** need `--download` as well. Without it nothing is fetched and the row says
  `needs --download: fetches <list-id> from <source>`. An install writes only a list that has
  no installed copy; it never overwrites one.
- A second run on a repaired store prints `nothing to fix`.

The command exits non-zero only if a check is still red after the fix pass. `--fix --json`
prints `{rows: [{check, before, after, detail, remedy?, remaining?, result?}], nothing_to_fix}`.
In it, `remedy` is `{kind: "applied", fix, command, ...}`, `{kind: "guided", command?, file?,
text}` or `{kind: "external", text}`, and `result` is `{status: "fixed" | "not_fixed" |
"needs_download", message}`. Plain `--json` keeps the `HealthReport[]` array.

```
$ shadowrunr preflight --fix --scripted
PF-01  green  scripted engine
...
PF-03  amber -> green  fixed: memory reindex
...
PF-08  amber  not in media-less phase (TTS, avatar, OBS, chat connectors)  external: the media stack is outside the media-less phase
```

Other PF-03 details: `.../data/memory: empty store, no index yet` (green), `.../data/memory
not created yet (the store creates it on first open under ...); nothing to index` (green), and
``index stale: run `shadowrunr memory reindex` (.../data/memory: <what disagrees>)`` (amber).
The PF-04 segment names come from the segment tables, so a segment that is descoped or
restored there shows up in this line without a code change.

## `shadowrunr memory reindex [--json]`

Deletes and rebuilds `<data>/memory/index.sqlite` from the markdown truth. This is the repair
PF-03 names when the index is stale; pre-flight only verifies and never rebuilds
(`shadowrunr preflight --fix` runs this repair and refuses while a daemon answers). Stop the
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
  no quota and writes nothing. It prints `bank_reach` (and a `bank reach: N of M tactics
  (unreached: ...)` line on stderr): which of the calibration's `bait_tactics` the
  character's situations perform, bound in `sim/calibration.json` under
  `bait_tactics[].situations.<character>`. A binding to a situation the bank lacks stops the
  plan before any spend. An unreached tactic is never provoked, so zero failures on it are
  silence, not evidence.
- `--brief current|<path>` (also on `sim run`, or `brief = "current"` in a matrix) hands
  every run the Lord's brief: the latest chosen plan's dispatch, segments and topics, plus the
  active directives (never proposed ones). `current` reads them from `<data>`; a path reads a
  `StreamBrief` JSON file. The brief's digest joins the instrument hash, so a tournament with a
  brief never mixes with one without, and `--plan-only` prints it as `brief_digest`. The
  judge then checks each prevent directive (`directive:prevent:<id>`), each try directive
  (`directive:try:<id>`) and, when a dispatch is present, `follows_dispatch`.
- `rate_cap_exemptions = true` in a matrix (Reconcile opt-5, off by default) runs every spec
  with the rate-cap experiment on: a grief message, a viewer's first real question of the
  stream and a `!propose` from a rank allowed to propose skip the rate cap (`rate_cap` /
  `exempt:grief|first_question|proposal`), and a room below 10 chatters gets a looser cap. It
  joins the instrument hash as `experiment/rate_cap_exemptions`, and `--plan-only` lists it
  under `experiments`. `sim/matrices/rate-cap-exemptions.toml` (treatment) and
  `rate-cap-exemptions-control.toml` (control) are the paired design.
- `case = "<id>"` in a matrix (or `sim run --case <id>`) audits a staged real case from
  `<data>/cases/<id>/` in every SEG-02A run instead of the fictional demo case; the
  bone-clerks answer from its public records (the 2-searches-per-hex cap and the Plebiscite
  still apply). Its content hash joins the instrument hash as `case/<id>`, and `--plan-only`
  prints `case` with `unapproved_real_case`. A case the owner has not approved still runs (a
  sim is private), with a warning, and each run's `run.json` records it.
- A SEG-02A run planned for an hour or more plays the case as a three-act arc over its
  duration (the `[arc]` table of `seg-02a.toml`): a later act's hexes stay locked, each act
  opens with a card, and `sign_off` waits for the 10-minute seal window at the end. Inside
  an act reveals are paced: one slot per hex staged for the act (6 hexes in a 60-minute act:
  one every 10 minutes), and a hex needs a bone-clerk search or a closed crowd vote that chose
  it first. Early or unworked reveals are refused (`reveal_not_due`, `reveal_needs_work`) and
  show in the transcript; the turn context says when the next reveal unlocks.
- A transcript longer than the rubric's `window_threshold_s` (1800 s) is judged in windows of
  `window_s` (1200 s, plus `window_overlap_s` = 120 s of context), one judge call per window
  at the judge route's timeout, aggregated into one verdict (a rule failed in any window
  fails; `judge.json` has one row per window). Parsed windows are cached in
  `<run_dir>/judge-windows/`, so a re-judge resumes.

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

## `shadowrunr clips ...`

The clip factory (contract appendix "Clips"). It reads transcripts, never writes under
`<runs>`, and stores everything in `<data>/clips/<id>/`.

- `clips scan --run <dir|run_id|tournament/run> | --tournament <id> [--no-hook] [--scripted] [--json]`
  scores every beat (moment markers within caps, the crowd's distinct-chatter z-score against
  the stream's own baseline, the programme level of a recorded run), asks the `clipper` route
  (Haiku, low effort) for one hook read per run, cuts around the line that carries the moment
  (12 s pre-roll, snapped earlier to a whole line; SEG-01 25-35 s, SEG-02A 45-60 s) and keeps
  the top 5 per run as drafts waiting at the owner gate. `--no-hook` skips the model and
  redistributes its weight; with `--scripted` the hook is unmeasured (the scripted engine's
  echo is not a hook read). A rescan replaces a run's untouched drafts and keeps every one the
  owner acted on (watched feedback carries over to the same cut). `engine_failed` runs are
  left out.
  - A run with a `voice.jsonl` (a sandbox session the voice followed) is cut on the voice's
    real timing: each line from its first `played` to its last part's end, the window opening
    up to 0.4 s before its first line (never into the line heard before) and its reaction beat
    stopping where the next heard line starts. A line never heard keeps its transcript time
    and shows as `[never heard]`.
  - A run with `recording.json` and its recording: each window's RMS level (ffmpeg `astats`)
    against the whole recording's is the audio signal (an assumption, labelled in the
    provenance). Without a recording, audio stays unmeasured and its weight is redistributed.
- `clips list [--status <s>] [--tournament <id>] [--run <id>] [--limit n] [--json]`: best first,
  with counts by status. Every ranking carries "weights unmeasured".
- `clips show <id> [--json]`: window (and which clock it is on), signals with the measured
  level, markers, evidence, the gate checklist, the recording, the render, watched feedback
  and captions.
- `clips render <id> [--json]`: a recorded run is cut from its recording with ffmpeg (about
  5 s per clip): `cut-16x9.mp4`, the window trimmed out of the broadcast picture with nothing
  burnt in, and `cut-9x16.mp4`, 1080x1920, the stage's face region with its AI badge above it
  and the lines burnt in below (see recording.md, "Clips from the recording"). Both are
  verified with ffprobe. A run without a recording is re-drawn: it records `draft.mp4`
  (1080x1920) from the stage in a headless Chromium and encodes it with ffmpeg, verified with
  ffprobe. That needs `apps/stage/dist` (build it with `pnpm -C apps/stage build`), node,
  ffmpeg/ffprobe and Chrome, Edge or a Playwright chromium build (`[clips]` in
  `shadowrunr.toml` names them), about 3 s of wall time per second of clip, and the manifest
  says "re-drawn (no recording)". With a tool missing it writes the poster frame and caption
  tracks and says which tool is missing; it never fakes a video.
- `clips recut <id> [--open <+-n>] [--close <+-n>] [--json]`: fix a cut instead of rejecting
  it. The window's edges move by whole transcript lines (`--open -1` opens one line earlier,
  `--close +1` closes one later; a viewer's question travels with the line that answers it)
  and the result is a **new** candidate: the run's transcript (and voice log) is read again,
  the captions are redacted afresh, and the gate runs from the first check against the whole
  stream (care state, moderation, injections), never copied from the original. The original
  is left exactly as it was; rejecting it is a separate act. Refused, nothing written, when
  the window would run over its segment's band (the error gives the figure), drop the payoff
  or the setup line it answers, or match lines another candidate already holds. The hook read
  and the loudness are the original's, carried with a note in the scoring provenance. Its
  first history entry names the original, and a rescan keeps it like an approved clip.
- `clips approve <id> --signature <name> [--override <reason>] [--note <text>]`: the owner's
  act. Refused when a check failed (exclusions hold whatever the score); an unmeasured check
  needs `--override`, recorded beside the hold reasons it overrides.
- `clips reject <id> --reason <text>`.
- `clips publish <id> [--platform youtube|tiktok|twitch]...`: **dry run**. Refused unless the
  clip is `approved_for_upload`. Writes the exact request each adapter would send to
  `<data>/clips/<id>/requests/<platform>.json`; nothing is sent and no credential is read.

```
$ shadowrunr clips scan --tournament <tournament_id>
$ shadowrunr clips show <id>
$ shadowrunr clips recut <id> --open -1
$ shadowrunr clips render <id>
$ shadowrunr clips approve <id> --signature owner --override "corrections log not built yet; fictional demo case"
$ shadowrunr clips publish <id>
DRY RUN: nothing was sent; each request was written instead
```

## `shadowrunr case ...`

Real SEG-02A cases (contract appendix "Real cases"). A case is researched off-stream, stored
in `<data>/cases/<id>/` (git-ignored: real case data is never committed) and runs on a live
stream only after the owner approves it.

- `case research --subject "<company, scheme or case>" [--max-hexes 19] [--source <url>]...
  [--id <ID>] [--tier haiku|sonnet|opus] [--no-discovery] [--json]`: the CALM pipeline. The
  Researcher role finds the case's records with web fetch restricted to the research allowlist
  (`crates/shadowrunr-segments/data/research-allowlist.toml`, or `[research] domains`); the
  pipeline fetches each page itself (allowlisted `https` hosts only, redirects included),
  reading a page that refuses automated clients through its Wayback Machine capture; one
  tool-less structured-output call extracts hexes, records (DOC/REP/CLM, never SPC) and the
  three acts from the fenced page text; the PII scrubber turns named people into roles and
  removes emails, phone numbers and street addresses; the case is staged `CALM_STAGED`, not
  approved for stream. The subject must be a company or a scheme named in an enforcement
  record: a person is never a subject. Exit 3 when nothing was staged (the report says why).
  It fetches real pages and spends engine calls.
- `case list [--json]`, `case show <id> [--json]`: status, approval, map counts by grade,
  every record's source with its retrieval day (and Wayback capture), every page fetched with
  its sha256, scrub hits, refusals, and the corrections log.
- `case approve <id>`: the owner's approval of the case as it is now. A live stream refuses
  an unapproved case, or one whose content changed after the approval. A simulation is private
  and may run it; its runs record `"unapproved_real_case": true` in `run.json`.
- `case retract <id>`: the case runs nowhere, not even in a simulation.
- `case correct <id> --type factual|reply|downgrade --note "<public note>" [--hex HEX-03]`:
  appends to `corrections.jsonl` (each entry blocks clip publishing for the case until the
  owner clears it).

`[research]` in `shadowrunr.toml` (all optional): `domains` (replaces the allowlist; adding a
domain is owner-gated), `user_agent` (sec.gov serves automated clients only when the
User-Agent names a contact, e.g. `"Org name contact@example.org"`; without one its pages are
read through the Wayback Machine), `max_sources` (14), `max_page_chars` (20000),
`fetch_timeout_s` (30), `discovery_timeout_s` and `extraction_timeout_s` (900).

```
$ shadowrunr case research --subject "Woodbridge Group of Companies (SEC and DOJ enforcement)" --tier opus
case: WOODBRIDGE-...
stored: .../data/cases/WOODBRIDGE-.../
$ shadowrunr case approve WOODBRIDGE-...
$ shadowrunr sim run --segment SEG-02A --case WOODBRIDGE-... --duration 900 --judge
```

## Stream prep: `shadowrunr prep ...`

The material a stream opens and runs on (contract appendix "Conversation orchestration"): a
cold-open story told while the room fills, a deck of quiet-room premises, stories and callbacks
per segment, and connector lines between bits and chat. The planner writes it before each
stream, the checks below run over it, and it is stored in `<data>/prep/<prep_id>/prep.json`
(`prep-<yyyymmdd>-<hhmmss>`, UTC) as `staged`. Nothing is used until the owner approves it.
Every command works on the store directly; no daemon is needed.

- `prep generate [--report <report_id>] [--json]`: one `planner` call (route
  `[engine.routes] planner`). Its input is the constitution, the active tone, the bible's
  "Running jokes" and per-segment notes, the active segments, and the plan: the newest plan
  chosen from `--report`, else the current plan (`<data>/next_plan.json`), else none. The
  plan's dispatch, segment notes, topics and active directives were composed from chat-derived
  reports, so they reach the planner fenced as untrusted data; a reply that echoes the fence
  token is refused and nothing is staged. Prints the title, the cold-open lines, the bits per
  segment and any failed check. It spends one engine call.
- `prep revise <id> [--note '<line>: <text>']... [--json]`: one `planner` call that answers a
  staged prep's failed checks, and the owner's notes, with an edit plan instead of a whole new
  prep. The planner is given the base material fenced as untrusted data, every finding of the
  checks by target, untruncated (`conn-01`, `prem-03`, `cold_open.beats[2]`; a count shortfall
  is an insert slot such as `insert:connector:into_poll`), and each note; a note
  (`--note 'cb-02: too flat'`, repeatable) makes that line a target even when it passes. The
  plan's operations are `rewrite` and `cut` on a named target and `insert` into a slot with
  room; any other operation is refused and recorded. Every line no applied operation names
  stays byte-identical. The result is staged as a new prep, checked against every rule like any
  other and approved only by the owner; `<data>/prep/<new>/revision.json` keeps the base id,
  the findings, the notes, the applied and the refused operations. The base is never changed
  and stays staged. Refused (exit 1) unless the base is staged, or when the reply echoes the
  fence token (nothing staged); a usage error (exit 2) when a note names no line of the base or
  there is nothing to revise.
- `prep table-read <id> --guest <foil> --scenes a,b,c` (or `--clear`): plan a SEG-03 Table
  Read on a staged prep: the scheduled guest and 1-3 scenes from `<data>/scenes/<guest>/`, in
  ballot order. The checks run again; exit 6 when `table_read` (or another check) fails.
- `prep list [--json]`: newest first, with status, bit count and failed checks.
- `prep show <id> [--json]`: the whole material and every check; a revised prep also prints
  `revised from <base>: N ops` with each operation's target and why, and each refusal.
- `prep approve <id> --signature <name> [--json]`: staged becomes approved, with `approved_by`
  and `approved_ms`. Refused while any check fails, with no override: a failed prep is kept
  staged so the owner sees why, and the fix is `prep revise` or a new `prep generate`. Approval
  re-runs the checks over the stored material, so a hand edit that breaks one is caught.
- `prep retire <id> [--json]`: no stream uses it again.

The checks (each `pass` or `fail`, with a detail naming the line):

| Check | Fails when |
|---|---|
| `line_length` | a line is longer than 220 characters |
| `counts` | the cold open is not 6-10 beats; fewer than 12 premises; stories outside 3-5; callbacks outside 3-5; fewer than 3 connectors of any kind |
| `shape` | a premise is not one line, a story is not 2-5 lines, a callback is not 1-3 lines, a bit has no payoff, a line is empty, or the reply had an item of unknown kind |
| `real_names` | a line has an `@handle`, names a real platform or channel, "a real/famous streamer", "a streamer named ...", or a name on the operator's `<policy>/real-names.txt` (one per line, `#` comments; local, never committed) |
| `urls` | a line has a link or a web address |
| `prior_streams` | a line leans on an earlier stream ("last time", "as I said", "remember when", "yesterday" ...) |
| `segments` | a bit is for a segment that does not exist or is descoped |
| `duplicates` | a line (ignoring case, spacing and end punctuation) or an id repeats |
| `output_gate` | the brain's output gate (the platform term rules, character and persona guards, personal-data asks) would change a line |
| `table_read` | (only with a table read) the guest is not a foil of the prep's character; not 1-3 scenes; a scene is repeated, missing from `<data>/scenes/<guest>/`, a draft, or has a shape or gate problem |

Which prep a run uses:

- `sim run`, `sim tournament` and sandbox sessions use the newest approved prep for the
  character, else the character's fictional `characters/<id>/prep.sample.json`, else none.
  `run.json` (sims) and `spec.json` (sandbox) record it as `prep`: the prep id, `sample` or
  `none`. `sim run` prints it.
- A prep written for another character is never used.

Exit codes:

- 0: done (`generate`, `revise`: staged, every check passed).
- 1: refused or failed (a failed check holds the approval, a prep in the wrong state, the
  engine or the store failed, the prep does not exist).
- 2: usage error (a malformed id, an empty signature, a note naming no line, nothing to
  revise).
- 6: `generate` or `revise` staged a prep with a failed check (shown; it cannot be approved).

```
$ shadowrunr prep generate
prep-20261020-180000  staged  malakor
plan: report rep-...
cold open: The Rented Throne
  1. Before the minions arrive, a confession from the Spire: ...
  -> Which brings me to you, early arrivals: ...
SEG-01: 8 premises, 2 stories, 2 callbacks
SEG-02A: 4 premises, 2 stories, 2 callbacks
connectors: 15
checks: all 9 passed
$ shadowrunr prep show prep-20261020-180000
$ shadowrunr prep revise prep-20261020-180000 --note 'cb-02: too flat'
prep-20261020-181500  staged  malakor
...
checks: all 9 passed
revised from prep-20261020-180000: 1 ops
  note cb-02: too flat
  rewrite cb-02: sharper turn, same running gag
$ shadowrunr prep approve prep-20261020-181500 --signature owner
```

## Scenes: `shadowrunr scene ...`

SEG-03 (the Table Read) plays owner-authored scenes from `<data>/scenes/<guest>/<id>.toml`
(format: [characters.md](characters.md), "Guests and scenes"). They are owner-local and never
committed. A stream plays a Table Read when its approved prep plans one (`prep table-read`);
SEG-01 then hands over to SEG-03 instead of SEG-02A.

- `scene list [--guest <foil>] [--json]`: every scene with its status, line and live-slot
  counts and its problems.
- `scene check <guest>/<id> [--json]`: the shape check plus the brain's output gate over every
  fixed line, live-slot fallback, title and pitch. A term rule that blocks gives
  `gate_blocked`, any other rule that fires `gate_recast`. Exit 6 when there is a problem.
- `scene approve <guest>/<id> [--json]`: refused (exit 1) while the scene has a problem;
  otherwise changes only the file's `status` value to `approved`, keeping its formatting.

Exit codes: 0 done, 1 refused or failed, 2 usage error, 6 the check found a problem.

A run without a planned Table Read (`sim run --segment SEG-03`, a sandbox with no prep plan)
plays the compiled-in fictional sample: guest `the-painter`, three scenes. Each round is a
ballot of the scenes left (`!vote <n|title>`, 60 s), the scene, then 5 minutes of free talk;
the last scene plays without a vote; a tie or an empty ballot plays the first in plan order and
the host announces he broke it. Live slots are written when the scene starts; a late or failed
one speaks its `fallback`, and a line the gate blocks is skipped. The segment runs at least 20
minutes and starts no new round after 40. With no playable scene the run refuses and says why.

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
- `sandbox.start`, `sandbox.stop`, `sandbox.status` (see [Sandbox sessions](#sandbox-sessions))
- `review.sessions`, `review.session`, `review.feedback.set`, `review.feedback.list`,
  `review.tag` (see [Session review](#session-review))

Notes:
- **Run ids:** `run_id` may be bare or `<tournament_id>/<run_id>`. A bare id that exists in
  several tournaments is rejected as ambiguous.
- **Starting a sim:** `sim.start` runs pre-flight first and refuses with `preflight_red`
  while any check is red. The error's `detail.red` lists each red row as `{check, detail,
  remedy?}`, with the same remedy `shadowrunr preflight` prints. It also refuses descoped segments, returns `busy` beyond
  `max_live_runs`, and `conflict` while a sandbox session runs. An empty `spec.run_id` gets a generated one. The run lands in
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
time. A sandbox session's events arrive one at a time, as they happen.

A client may also send `{"ack": {"seq", "gaps", "stalled_ms"}}` at any time: the highest seq
it applied of the run it follows, the gaps it found, and how long it has been stalled. An ack
never changes the source and gets no reply. A malformed ack gets an `invalid_params` frame and
the source keeps running. The daemon keeps acks per connection for the sandbox `stage_ack`
check.

## Sandbox sessions

A sandbox session runs Malakor on the engine against the simulated room, on the wall clock, for
5, 10 or 30 minutes, and ends in a gated report. The daemon hosts the session; the command is
a thin client of it.

1. Run the daemon: `shadowrunr daemon`. Use `--scripted` for a keyless rehearsal.
2. Serve the stage with `pnpm -C apps/stage dev` and open
   `http://localhost:5178/?run=live&broadcast=1`. The command prints this URL; on a daemon
   port other than 7717 it adds `&ws=ws://127.0.0.1:<port>/events`. Use `localhost`, not
   `127.0.0.1`, because the daemon only accepts browser origins on `http://localhost:*`.
3. Start the session:

```
shadowrunr stream sandbox --minutes 5|10|30 [--seed S] [--chaos outage|lost] [--no-judge] [--tone T] [--port P] [--record]
```

| Flag | Meaning |
|---|---|
| `--minutes` | Required: 5, 10 or 30. |
| `--seed` | The simulated room's seed. |
| `--chaos outage` | 90 s of engine errors from half-way. The host must stall in character and recover. |
| `--chaos lost` | Engine errors from half-way to the end. The host must stall, then sign off in character. 10 and 30 minute sessions only. |
| `--no-judge` | Skip the judge. The `judge` check is then unmeasured, so the gate fails. |
| `--tone` | Tone variant. The default is the character's active tone. |
| `--port` | The daemon's port. The default is `[daemon] port` (7717). |
| `--record` | Record the session in OBS: the stage through a browser source, picture and voice in one file in the session directory. Refuses to start when OBS is not reachable. See [recording.md](recording.md). |

The command prints the session id and the stage URL, then a progress line every 10 s:

```
02:05/05:00 · say-gap max 9.2 s · errors 0 · ack seq 230/240
```

The fields are elapsed and planned time, the longest silence so far, the core engine errors,
and the highest seq the stage acknowledged against the events so far. Ctrl-C calls
`sandbox.stop`: the host signs off, and the command waits for the report. A second Ctrl-C
detaches, and the session keeps running in the daemon. At the end the command prints the
checks table and the session directory.

The checks, in report order:

| Check | Passes when |
|---|---|
| `say_gap` | No silence on stream is longer than the ceiling (15 s). |
| `core_errors` | The core thread had no engine errors outside the chaos window. |
| `sign_off` | The host signed off cleanly. |
| `chaos` | Under `--chaos`, the host stalled in character and recovered (or signed off). Without `--chaos` it passes ("no chaos requested"). |
| `stage_ack` | A stage subscriber acked the final event's seq with no gaps. It is unmeasured when no stage acked. The daemon waits up to 6 s after the session for the final ack. |
| `judge` | The judge found no rule fails. It is unmeasured with `--no-judge`, and fails when judging errors. |

An unmeasured check is not a pass, so a gated run needs the stage open and the judge on.

Exit codes:
- `0`: every check passed.
- `1`: the daemon refused the session (pre-flight red, or another session or a live sim is
  running), with `--record` OBS is not reachable or refused to record, or another error.
- `2`: a usage error, or no daemon is reachable (start one with `shadowrunr daemon`).
- `6`: the session ended and failed its gate, or failed outright.

Files land in `<runs>/sandbox/<session_id>/`, with `session_id` =
`sbx-<yyyymmdd-hhmmss>-<minutes>m` in UTC. `session.json` is the report. A session that fails
leaves `error.json` instead, and no `session.json` means the session failed.

The session directory holds:
- `spec.json`: the run spec plus the params, `planned_s`, `tick_s`, `programme`,
  `handover_s` and `chaos_window_s`.
- `instrument.json` (plus `brief.json` when the stream has a brief).
- `transcript.jsonl`: one event per line, flushed as it happens. Line n is `/events` seq n.
- `ledger.jsonl`: every engine call from the brain, the room, the judge and the tagger. Calls
  refused by chaos show as `unavailable`.
- `room.json`: featured chatters, stimuli, chat stats, removed viewers and brain errors.
- `session.json`: the gate report.
- `verdict.json`, `judge.json` and `judge-windows/`, after judging.
- `tags.json`, after tagging (see [Session review](#session-review)).
- `voice.jsonl`, when the voice follows the session: what the voice rendered and skipped, and
  what the stage played (see voice.md).
- `recording.<ext>` and `recording.json`, with `--record` (see [recording.md](recording.md)).

Over `/rpc`:
- `sandbox.start {minutes, seed?, chaos?, judge?, tone?}` returns `{session_id, dir,
  stage_url}`. It refuses with `invalid_params`, `preflight_red`, or `conflict`.
- `sandbox.stop {session_id}` returns the status. Stopping an ended session returns its status
  unchanged.
- `sandbox.status {session_id?}` returns the given session, or the latest one, including
  sessions from earlier daemon runs found on disk.

## Session review

The console's Review page reads finished sandbox sessions: every session directory under
`<runs>/sandbox/` that has a `session.json`. Two files feed it besides the session's own:

- `<runs>/sandbox/<session_id>/tags.json` types every Malakor line with a `kind` (roast, tease,
  joke, title, welcome, reply, callout, show, refusal, care, disclosure, signoff, other), a
  `target` (viewer, chat, foil, self, lord, case, none) and an `edge` (none, mild, sharp). It
  is derived and versioned (`tagger_version` 1): tagging rewrites it whole, through a temp file
  and a rename, and nothing edits it by hand. `untagged` counts the lines the model left
  without a valid tag.
- `<data>/review/feedback.jsonl` is the operator's marks and notes per line, append-only. Each
  row is `{session_id, line, tags, note?, at_ms}`. The latest row per `<session>#<line>` wins,
  and a row with no tags and no note clears the line. Each append takes a file lock and syncs,
  so concurrent saves never interleave. A malformed row is skipped (the daemon logs how many)
  and the rest of the log still folds.

`line` is the 1-based transcript line everywhere: the `/events` seq and the judge's
`<run>#<line>` refs.

**Tagging.** After a session is judged (`sandbox.start` with the judge on), the daemon tags it
in the background. The session's state does not wait for it, and a tagger failure is only
logged (`review: tagging <id> failed: ...`); the session then shows `tagged: false`. The tagger
sends about 60 lines per `tagger` call (Haiku, effort low), three calls at a time. Each line
goes with its segment, up to three chat lines it answers and Malakor's previous line. A call
that fails or a reply that does not parse is retried once; after that its lines are untagged.
When every call fails at the engine, nothing is written. The calls land in the session's
`ledger.jsonl` with correlation `tag:<session_id>`. The cost is about 15 Haiku calls per
30-minute session.

To tag, re-tag or backfill from the command line (a thin client of the running daemon, like
`stream sandbox`):

```
shadowrunr review tag <session_id> [--force] [--port P]
shadowrunr review tag --all [--force] [--port P]
```

A session whose `tags.json` is current is kept without a call unless `--force`. `--all` tags
every session, oldest first. Each session prints one line:

```
sbx-20261020-170127-10m: tagged 412, untagged 3, calls 7
```

`calls` counts this run's tagger rows in the session's ledger (`?` when the ledger is not
readable from where the command runs).

Exit codes:
- `0`: every session was tagged, or already was.
- `1`: tagging a session failed (unknown session, engine down, a file error).
- `2`: a usage error (including a malformed session id), or no daemon is reachable.

Over `/rpc`:
- `review.sessions {}` returns one row per session, newest first: minutes, chaos, start,
  length, the gate, checks, the longest silence, `rule_fails` (absent when not judged), Malakor
  lines, the tag mix, lines with feedback and `tagged`. Nothing is cached: each call reads
  every session's files again, with a light pass over each transcript (7 ms for seven
  sessions, 2.1 MB on disk, release build, 2026-10-20).
- `review.session {session_id}` returns the whole session in one reply (a 30-minute session
  of 2,497 events is about 750 KB of JSON): the row, segments, every event, `tags?`, `verdict?`, the catalog (what each failed rule and each judged quality
  dimension asks for, from the character's `situations.json` and `sim/rubric.toml`), the
  ledger rows and the session's feedback.
- `review.feedback.set {session_id, line, tags, note?}` appends one row and returns it. The
  line must be a Malakor line. Repeated tags are dropped, the note is trimmed (2,000
  characters at most) and a blank note is absent.
- `review.feedback.list {session_id?}` returns the folded feedback, by session and line.
- `review.tag {session_id, force?}` tags the session and returns `tags.json`.

A session id that is not `sbx-<yyyymmdd>-<hhmmss>-<minutes>m` is `invalid_params` (so is any
path), and a session with no `session.json` is `not_found`. An engine failure is `engine`,
and a file error `io`.

## `shadowrunr voice check|say`

Malakor's optional local voice (Kokoro through sherpa-onnx). `voice check` probes the engine, the
Kokoro files, the sidecar's Python and the durations model, and exits 0 only when the voice can
speak. `voice say "<text>" [--out <wav>]` renders one line through the sidecar and prints each
sentence's timing. Setup, `[voice]` keys and the stage side: [voice.md](voice.md).

## Character packages, rules and moderation lists

```
shadowrunr character new nova                                # characters/nova/ from characters/_template, then validated
shadowrunr character new nova --from path/to/template        # from another template directory
shadowrunr character validate nova                           # or a package directory; --json for the list
shadowrunr character rules malakor                           # list rules: on / off / lock (a rail)
shadowrunr character rules malakor --disable crisis-chat-ban  # operator override in <data>/characters/malakor/rules.json
shadowrunr character rules malakor --disable crisis-handoff   # refused: a rail is always on, nothing is written
shadowrunr policy install-list awdev1-better-profane-words   # downloads into policy/local/ (git-ignored, GPL-3.0, never committed)
```

`character validate <dir|id>` prints each issue as `file: key: reason`, then a count. A value
that still holds the template's `FILL:` placeholder is a warning (`(warning)` after the reason);
everything else is an error. It exits 0 when there are no errors, 1 otherwise. `character new
<id>` copies the template to `<characters>/<id>/`, sets its `id`, and prints the same list; it
refuses an id starting with `_` and never overwrites an existing directory. The RPC
`character.validate {id}` returns the same issues as `[{file, key?, reason, severity}]`. What a
package holds, and every table of `character.toml`: [characters.md](characters.md).

## Personas

```
shadowrunr personas status [--json]   # verifies the Personas registration (exit 3 = Personas not running)
shadowrunr mcp                        # stdio MCP server for Athena (see docs/guide/personas.md)
```
