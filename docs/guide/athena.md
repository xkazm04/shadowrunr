# Athena drives the console

Athena is the operator's chat assistant. She can host a web app as a tab and work it through
the tools the page publishes on `document.modelContext` (WebMCP). The Shadowrunr console
publishes a curated catalog of these tools. With it she can find her way around the console,
read sessions, runs and characters, and move the console to what she is talking about. She
can also propose writes, and each write reaches you as an approval card before it runs. The
conversation itself stays in Athena's app, because the console has no chat of its own.

The console publishes the catalog only when both of these are true:

- the page has a WebMCP registry, which happens when Athena's bridge (`inject.js`) runs in the
  tab or the browser supplies a native one;
- you switched **Athena** on in the console's top bar. The setting is stored in
  `localStorage["shadowrunr.athena.drive"]` as `"1"` and is off by default.

Outside Athena there is no registry. The toggle is not rendered and the catalog code is never
loaded, because it is a separate chunk (`agent` in `budgets/bundle.json`).

## Pairing

1. Run the daemon (`shadowrunr daemon`, or `shadowrunr daemon --scripted` for a keyless
   rehearsal) and the console (`pnpm -C apps/console dev`, or the desktop app).
2. Open the console in an Athena tab, at `http://localhost:<port>/` and not at `127.0.0.1`,
   because the daemon only admits browser origins on `http://localhost:*`. Add
   `?daemon=http://127.0.0.1:<port>` when the daemon is not on 7717.
3. Register the console's origin in Athena. Until you do, Athena treats the app as a first
   sighting and gates every tool, reads included. The page names itself with
   `<meta name="athena:app" content="shadowrunr">`, `athena:app-name` "Shadowrunr OS" and
   `athena:app-version` (the console's package version).
4. Click **Athena** beside the connection chip. It turns accented and reads **Athena driving**,
   and the tools are registered. Click it again to stop: one abort unregisters every tool at
   once, and Athena's next `list` shows none.

## How she works it

- **Orientation lives in the results, not in the descriptions.** Athena cuts each description
  to 240 characters, so `where_am_i` and `app_map` carry the map. `where_am_i` returns the
  current hash, page, layer, title, breadcrumbs, the ids in view and `tools_here`, the 3 to 6
  tools that fit the page. `app_map` returns every page's purpose and route grammar with an
  example hash, a glossary and a few recipes.
- **She shows what she reads.** Every READ tool that has a view takes `show` (default true).
  On success it moves the console through the ordinary hash router, so breadcrumbs, motion and
  Esc behave as they do for you, and it returns `view`, the hash it showed. `show: false` reads
  without moving the console.
- **No ambient selection.** No tool reads "the current session" or "the current character".
  Ids are always arguments, and `where_am_i` returns the ids in view so she can pass them.
- **Bounded answers.** Lists carry `showing`, `total` and `footer` ("showing N of M"), with
  small default limits. Sessions, transcripts and tournament results come back summarised or as
  a window, never whole.
- **Results.** A tool returns `{ content: [{ type: "text", text: "<JSON>" }] }`, which the
  bridge flattens to the JSON text. A failure throws, and the bridge relays it as
  `{ ok: false, error: "<code>: <message>" }`. The code is the daemon's own (`not_found`,
  `conflict`, `invalid_params` and the rest of the taxonomy), or `refused` when the console
  itself declines.

## The catalog

Classes, as Athena's gate reads the flags:

- **READ**: `readOnlyHint: true`, `consequentialHint: false`, and `athena: { reversible: true,
  side_effects: "none" }`. Runs without asking once the origin is registered.
- **AUTO**: both hints false, and `athena: { reversible: true, side_effects: "internal" }`. Moves
  or starts something inside the app that can be undone.
- **GATED**: `consequentialHint: true`, and `athena: { reversible: false, side_effects:
  "internal" }`. Every call files an approval card.

| Tool | Class | What it does |
|---|---|---|
| `where_am_i{}` | READ | The current view, the ids in view and the tools that fit it |
| `app_map{}` | READ | Pages, route grammar, glossary, recipes |
| `go_to{view}` | AUTO | Moves the console to a hash, checked by the router's decoder and the page grammar |
| `list_sessions{limit?, show?}` | READ | Sandbox sessions, newest first (`review.sessions`) |
| `get_session{session, show?}` | READ | One session summarised: checks, verdict, mix, notable lines (`review.session`) |
| `open_moment{session, line, show?}` | READ | One line with 3 either side, its tag, failures and marks; shows `#/review/<s>/timeline/<line>` |
| `list_runs{limit?, tournament?, cursor?, show?}` | READ | Sim runs (`runs.page`) |
| `get_transcript{run, from?, limit?, show?}` | READ | A window of a run's transcript (`run.transcript`) |
| `get_verdict{run, show?}` | READ | The judge's verdict summarised (`run.verdict`) |
| `list_tournaments{limit?, show?}` | READ | Tournaments, most recently updated first (`tournament.list`) |
| `get_tournament{tournament, show?}` | READ | Results summarised: totals, worst rules, cells (`tournament.results`) |
| `ranks_table{character?, show?}` | READ | The merged rank ladder (`ranks.table`) |
| `ledger_summary{tournament?, run?, show?}` | READ | Engine calls, cost and routes (`ledger.summary`) |
| `query{method, params?}` | READ | One read from the allowlist below |
| `list_characters{show?}` | READ | Every package (`character.list`) |
| `get_character{id, show?}` | READ | A package summarised, not its text (`character.get`) |
| `read_character_doc{id, doc, show?}` | READ | A whole package document and its digest (`character.doc.get`) |
| `validate_character{id, show?}` | READ | The package's issues (`character.validate`) |
| `set_character_doc{id, doc, text, base_digest}` | GATED | Replaces a package document (`character.doc.set`) |
| `set_character_rules{character, id, enabled}` | GATED | Switches an optional rule (`character.rules.set`) |
| `set_tone{id, tone}` | GATED | Sets the active tone (`character.tone.set`) |
| `add_tone_line{character, tone, text, tags, status?}` | GATED | Adds a bank line (`tone.lines.add`) |
| `set_tone_line_status{character, tone, id, status}` | GATED | Approves, seeds or retires a bank line (`tone.lines.set_status`) |
| `leave_feedback{session_id, line, tags, note?}` | GATED | Marks a Review line (`review.feedback.set`) |
| `start_sandbox{minutes, seed?, tone?}` | GATED | Starts a 5, 10 or 30 minute sandbox session (`sandbox.start`) |
| `stop_sandbox{}` | GATED | Stops the running session, the latest one `sandbox.status` reports (`sandbox.stop`) |
| `start_sim{spec, judge?}` | AUTO | Starts one text sim run on a scripted daemon and shows `#/simulator` (`sim.start`) |

`query` sends only these read methods: `plan.current`, `plan.proposals`, `directive.list`,
`reconcile.list`, `reconcile.report`, `reconcile.incidents`, `segments.tables`,
`segments.directions.get`, `tone.directions.get`, `tone.lines.list`, `tone.inbox`,
`tone.variants`, `prep.list`, `prep.get`, `clips.list`, `clips.get`, `sandbox.status`,
`engine.status`, `ledger.page`, `review.feedback.list`, `runs.list`, `tournament.list`,
`character.rules`, `moderator.status`, `moderator.rules.list`, `moderator.log` and
`voice.settings.get`. Any other method is refused by name and never sent.

### Package documents

`read_character_doc` and `set_character_doc` cover `constitution.md`, `bible.md`,
`directions.md`, `character.toml`, `stalls.toml`, `situations.json` and `foils/<slug>.md`. Pass
the `digest` from the read as `base_digest`, or `""` for a new foil. If the file changed in
between, the write is refused with `conflict` and the file is left as it is. The daemon keeps
the previous bytes under `<data>/character-backups/`, validates the package after the write,
and undoes a write that breaks it. See the "athena copilot" appendix of
[docs/spec/contract.md](../spec/contract.md).

**The constitution.** The brain never edits `constitution.md`. Athena is not the brain: she is
your own agent, and every constitution edit reaches you as a GATED card that you approve or
decline.

## What stays out of reach

- **Live sims.** `start_sim` calls `engine.status` first and refuses unless the fleet mode is
  `scripted`. Sandbox sessions run on the daemon's engine, but each start is a GATED card.
- **Clips and publishing.** `clips.list` and `clips.get` are readable through `query`. No tool
  scans, renders, approves, rejects or publishes a clip.
- **The moderator and Twitch.** Its status, rules and log are readable. No tool switches its
  mode, edits its rules, unbans or connects Twitch.
- **Writes through `query`.** The allowlist holds reads only, and writes go only through the
  named tools above.

## Testing it end to end

The headless harness lives in Athena's repository at
`athena-portable/apps/desktop/e2e/shadowrunr/`. It boots the daemon `--scripted` on a scratch
copy of the characters, the console, Playwright Chromium with `inject.js`, and a real
`athena serve`. Scripted transcripts call `where_am_i`, `go_to`, `list_sessions` and
`set_character_doc`, and assert that the hash moved, that a card was filed and that the
scratch bible changed. Its README covers the live pilot.
