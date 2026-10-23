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
  tools that fit the page. `app_map{}` returns an index: one line per page (its purpose and an
  example hash) and the topics. `app_map{topic}` returns one topic: `glossary`, `recipes`, or a
  page id, which gives that page's route grammar and the tools that fit it.
- **She shows what she reads.** Every READ tool that has a view takes `show` (default true).
  On success it moves the console through the ordinary hash router, so breadcrumbs, motion and
  Esc behave as they do for you, and it returns `view`, the hash it showed. `show: false` reads
  without moving the console.
- **No ambient selection.** No tool reads "the current session" or "the current character".
  Ids are always arguments, and `where_am_i` returns the ids in view so she can pass them.
- **Bounded answers.** Lists carry `showing`, `total` and `footer` ("showing N of M"), with
  small default limits. Sessions, transcripts and tournament results come back summarised or as
  a window, never whole.
- **Every answer fits Athena's cut.** Athena cuts every host tool result at 1,600 characters
  (`RESULT_CAP` in athena-portable's `src/athena/lane/turn_frame.py`) and tells her "showing
  1600 of N". So each tool is built to answer in at most 1,500 characters of JSON. Long lines
  are clipped harder, a document comes back a page at a time, and a summary gives up detail
  before it drops its list. The shared tool wrapper also checks every answer. A list that would
  still run over is trimmed, and its `footer` says how many were kept. A list that pages by
  cursor also loses its `next_cursor`, because that cursor would skip the trimmed items. Any
  other answer that runs over is refused with `too_large`, and the refusal says what to ask for
  instead. `query` returns a long answer that is not a list as `{ truncated: true, text }`, the
  start of its JSON inside a string. Her cut never lands in the middle of the JSON.
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
| `app_map{topic?}` | READ | The index of pages and topics; a topic is `glossary`, `recipes` or a page id (its route grammar) |
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
| `read_character_doc{id, doc, offset?, limit?, show?}` | READ | One page of a package document with its digest, plus its outline on the first page (`character.doc.get`) |
| `validate_character{id, show?}` | READ | The package's issues (`character.validate`) |
| `set_character_doc{id, doc, text, base_digest}` | GATED | Replaces a package document whole: a new foil or a small file (`character.doc.set`) |
| `edit_character_doc{id, doc, base_digest, find, replace}` | GATED | Replaces one exact passage of a document, which is how a long one is changed (`character.doc.get`, then `.set`) |
| `set_character_rules{character, id, enabled}` | GATED | Switches an optional rule (`character.rules.set`) |
| `set_tone{id, tone}` | GATED | Sets the active tone (`character.tone.set`) |
| `add_tone_line{character, tone, text, tags, status?}` | GATED | Adds a bank line (`tone.lines.add`) |
| `set_tone_line_status{character, tone, id, status}` | GATED | Approves, seeds or retires a bank line (`tone.lines.set_status`) |
| `leave_feedback{session_id, line, tags, note?}` | GATED | Marks a Review line (`review.feedback.set`) |
| `start_sandbox{minutes, seed?, tone?}` | GATED | Starts a 5, 10 or 30 minute sandbox session (`sandbox.start`) |
| `stop_sandbox{}` | GATED | Stops the running session, the latest one `sandbox.status` reports (`sandbox.stop`) |
| `start_sim{character?, tone?, segment?, audience_size?, seed?, tier?, judge?}` | AUTO | Starts one text sim run on a scripted daemon and shows `#/simulator` (`sim.start`) |

`query` sends only these read methods: `plan.current`, `plan.proposals`, `directive.list`,
`reconcile.list`, `reconcile.report`, `reconcile.incidents`, `segments.tables`,
`segments.directions.get`, `tone.directions.get`, `tone.lines.list`, `tone.inbox`,
`tone.variants`, `prep.list`, `prep.get`, `clips.list`, `clips.get`, `sandbox.status`,
`engine.status`, `ledger.page`, `review.feedback.list`, `runs.list`, `tournament.list`,
`character.rules`, `moderator.status`, `moderator.rules.list`, `moderator.log` and
`voice.settings.get`. Any other method is refused by name and never sent.

There are 28 tools: 17 READ, 2 AUTO and 9 GATED.

### Package documents

The document tools cover `constitution.md`, `bible.md`, `directions.md`, `character.toml`,
`stalls.toml`, `situations.json` and `foils/<slug>.md`. Each also takes the bare stem (`bible`,
`constitution`, `directions`, `character`, `stalls`, `situations` or `foils/<slug>`). Anything
else is refused, and the refusal lists the legal names.

- **Reading.** `read_character_doc` returns `{doc, digest, total_chars, offset, text,
  next_offset?, outline?}`. `text` is at most 1,200 characters, and it is cut at the end of a
  line when one is near. While more follows, `next_offset` says where the next page starts.
  The first page (no `offset`, or 0) also carries `outline`: the markdown headings, each
  prefixed with the character offset it starts at, so she can jump to a section. The outline
  holds at most 20 headings. Headings are dropped from its end before the first page's text
  falls under 600 characters. The console fetches the whole document with `character.doc.get`
  and slices it itself.
- **Changing a long document.** `edit_character_doc` reads the document and refuses with
  `conflict` unless its digest still equals `base_digest`. `find` is the exact text to
  replace, and it must occur exactly once. Zero or several occurrences are refused, and the
  refusal gives the count. To insert, `find` is an anchor sentence and `replace` is the anchor
  plus the new text. The tool writes the whole new text through `character.doc.set` against
  the same digest, and returns `{doc, digest, previous_digest, backup?, changed_chars}`. The
  approval card shows `find` and `replace`, so you approve a readable change instead of a
  whole file.
- **New and small files.** `set_character_doc` replaces a document whole. Use it for a new
  foil (`base_digest: ""`) or a small file.

On every write, pass the `digest` from the read as `base_digest`. If the file changed in
between, the write is refused with `conflict` and the file is left as it is. The daemon keeps
the previous bytes under `<data>/character-backups/`, validates the package after the write,
and undoes a write that breaks it. See the "athena copilot" appendix of
[docs/spec/contract.md](../spec/contract.md).

**The constitution.** The brain never edits `constitution.md`. Athena is not the brain: she is
your own agent, and every constitution edit reaches you as a GATED card that you approve or
decline.

### Starting a sim

Every argument of `start_sim` is optional, so she never has to build a `SimRunSpec`. The
console fills the spec from the defaults `shadowrunr sim run` uses (`[sim]`, in
`crates/shadowrunr-daemon/src/config.rs`):

| Field | Default |
|---|---|
| `character` | `malakor` |
| `tone_variant` | the character's active tone (`character.get`). A tone it does not have is refused, and the refusal lists its tones |
| `segment` | `SEG-01`, or `SEG-02A`. The descoped segments are not offered |
| `audience_size` | 30, from 1 to 300. 3, 30 and 300 are calibrated |
| `seed` | 1 |
| `core_tier`, `core_effort` | the `core_thread` route in `engine.status`, else `sonnet` and `low`. `tier` overrides the tier |
| `duration_s` | 600 |
| `population` | the one for the nearest audience on a log scale: `small_new_channel` (3), `growing_channel` (30), `raid_moment` (300) |
| `run_id` | `<tone>.<tier>.<segment>.a<audience>.s<seed>`, minted the way the CLI mints it |

It returns `{run_id, spec_used, judge, view}`. The population table is the compiled-in
default. If a daemon's config overrides `[sim] populations`, the console still sends the
default population, because it cannot read that config.

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
