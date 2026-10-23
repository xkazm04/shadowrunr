# Kiroween 2026: Shadowrunr OS v2, a new project built for the hackathon

Status: plan, written 2026-10-23 against Overlord `main` at `8e6586d`. Nothing in this note is
built. The owner's rule from the hackathon: **no building before Friday 2026-10-30**, and only
projects created during the submission period are eligible. Submissions close **Friday
2026-12-04, 14:00 Pacific**. The full rules, judging criteria and bonus challenges arrive on
Oct 30, so every claim about them below is marked `[verify Oct 30]`.

This note has to be reconciled once more before implementation, because Overlord keeps moving
for another week. Section 9 says how.

## 1. What we are doing and why

Enter the **Costume Contest** track ("a haunting user interface that is polished and
unforgettable; spooky design elements that enhance the app's function") with Shadowrunr OS, the
runtime for AI characters that stream, whose flagship is a lich who runs a livestream like a
bureaucracy of the damned. The hackathon is also the forcing function to restart the product
clean, now that its full scope is known: a new repo, better architecture, a brand, and a theme
system.

### Decisions already taken by the owner (2026-10-23)

| Question | Decision | Rejected |
|---|---|---|
| v1 scope | **Full product parity.** The new repo becomes the direct successor and Overlord is thrown away for its sake. If the deadline is missed we keep building the product we intended anyway. | Haunted console + stage + one character; console-only |
| Port policy | **Fresh repo, new concept, reusing only assets and ideas.** No source file is carried over. | Clean-room rewrite from specs; port files and refactor |
| Engine for judges | **Scripted/replay default, pluggable providers.** | Bedrock only; Claude CLI fleet as primary |
| Theming | **Two themes from day one: a baseline brand theme and a Halloween theme.** Halloween switches off after the season and the core brand theme takes over. | one-off skin |
| Brand | **We do not have one yet; forging it is part of the contest entry.** | keep "Shadowrunr OS" unexamined |

### Tensions the plan has to carry honestly

1. **Full parity against five weeks.** Overlord is about 131k lines of Rust plus about 41k of
   console TS plus 18k of stage TS plus about 11k of Python. "Reusing only assets and ideas"
   means all of it is rewritten. A team of one owner plus agents cannot assume parity by
   Dec 4. Section 7 therefore defines a **submission slice** (what must demo flawlessly) and
   treats parity as the trailing queue. This is a sequencing decision, not a scope cut.
2. **Kiro is the hackathon's tool.** `[verify Oct 30]` entries are probably judged on how Kiro
   was used (a third-party guide says a write-up of Kiro usage is mandatory). Our current AI
   workflow is Claude Code skills (`spark`, `contest`, `perfect`). The new repo is built **in
   Kiro**: specs, steering, hooks. Section 6 maps it.
3. **A brand that does not exist yet and a name that may not be free.** "Shadowrunr" is one
   letter from *Shadowrun*, a long-running commercial game franchise. The repo says the names
   are "project names, not licensed marks", which is not a clearance. Run a name search
   before the brand is forged, and treat the name as undecided until then. This is a risk flag,
   not legal advice.

## 2. What Overlord actually is (measured 2026-10-23)

| Area | Size | What it is |
|---|---|---|
| `crates/*` | about 131k LOC incl. tests, 8 crates | types (contract), engine (LLM wrapper), brain (mind), segments, sim, reconcile, clips, daemon |
| `apps/console` | about 41.5k lines, 84 test files | 7 pages, each a Layer 1 (graphic) plus Layer 2 (nested page) |
| `apps/stage` + `packages/stage-kit` | 18.1k + 7.1k | 1920x1080 OBS browser-source stage, hand-written WebGL2 avatar |
| `tools/{voice,rig,avatar}` | about 10.7k Python | Kokoro voice sidecar, portrait-to-layers rig, depth maps |
| `packages/{contract,tokens}` | generated | TS types and validators from Rust; design tokens |

Evidence behind each row is in section 3. The scouts did not run the builds or gates; line
counts are `wc -l`.

## 3. Codebase analysis: what to carry as an idea, what to do differently

The headline: **the architecture's ideas are good and its execution is bloated.** The seams
that made it testable are worth rebuilding exactly; the places where it grew by accretion are
where a clean start pays most.

### 3.1 Architecture

Keep (as designs, rewritten):

- **A wire-contract crate with no dependencies** that every other crate builds on
  (`types/src/rpc.rs`, `schema.rs`).
- **The `Engine` trait seam** (`complete / open_session / health`, `types/src/engine.rs:167`)
  with a `ScriptedEngine` that tests and demos both use, and a `fake_claude` binary that
  replays recorded stream-json so the process-spawn path is tested without spending quota.
  Engine has 0 panic sites.
- **Role routing**: `cfg.engine.route(Role)` returns tier, effort and timeout, used in 9 places.
- **Segments as TOML tables**, depending only on types.
- **The stage's `LiveStore`**: seq numbers, gap detection and resubscribe, a 2000-event ring
  with a keyframe every 100 for DVR seeking, a watchdog that shows SIGNAL LOST after 3 s
  (`live.ts:13-21`), and a fixture replay mode so the stage runs with no daemon.
- **A pure scene reducer** (`stage-kit/scene.ts`) and **audio-as-the-clock lip-sync**
  (`voice/player.ts`).
- **The console's two-layer page model** and its one keymap with a conflict test.

Do differently:

| Problem in Overlord | Evidence | v2 decision |
|---|---|---|
| God files and a god daemon | daemon is 46k LOC, 101 files, 1073 pub items; 37 files over 400 lines; sim `judge.rs` 3785, `runner.rs` 3199; brain `moderation.rs` 3574 | Split by responsibility before the first line: about 11 crates (section 5.1). A 400-line cap from day one, on Rust too. |
| Stringly-typed RPC | `match req.method.as_str()` over 77 arms (`daemon/src/rpc.rs:206`); `serde_json::Value` 139 times; 64 `Result<_, String>` | One typed method registry that generates **both** the dispatcher and the JSON schema, so method names exist in one place. Typed error enum at the edge. |
| Fragile contract pipeline | Node build step shells out to `cargo run`; hand-curated `wire_types()` list silently drops a forgotten type; generated output committed; CRLF normalisation in the checker | Registry-derived schema (above); generated TS is a build artifact, not committed; the drift gate compares hashes. |
| Panic surface | daemon about 389 panic-ish sites (inflated by `e2e/`), brain 30, sim 14 | `clippy::unwrap_used` and `expect_used` denied outside tests, workspace-wide. |
| Duplicated subsystems | `daemon/src/sandbox.rs` (1481) and `sim/src/sandbox.rs` (2564); two `character.rs`; two clips | One sandbox crate; one owner per noun. |
| Persistence by accretion | SQLite memory, JSONL ledgers, run dirs, file stores for moderator, prep, review, tone | SQLite as the store of record; files only for artefacts (audio, clips). |
| Windows-only secrets | `keyring =3.6.3` with `windows-native` only | A `SecretStore` trait with a file-backed dev impl and an OS impl; the demo needs no secrets. |
| Loopback security is origin-only | `server.rs:149-165`, no token | Bind loopback, require a per-run bearer token. |

### 3.2 Code quality and structure

Carry the **gates**, not the code. They are what kept 41k lines of console TS under the cap:

- stylelint tokens-only for CSS modules (no hex, named colours, `rgb()`, `px`; about 1,700
  `var(--sr-` uses, 7 hex in CSS), a 400-line file cap that exits 2 if it walked nothing,
  `eslint-plugin-boundaries` layering, no `any`, no `as unknown as`, `JSON.parse` only inside
  the contract door, a counted suppression ratchet that can only shrink, a DOM-node budget per
  view, a per-route bundle budget, and exact-count perf gates (draws, RPC calls) instead of
  timing gates. Every one was proven red once on a seeded violation.
- CI lanes derived from the workspace rather than remembered (`scripts/ci-lanes.mjs --check`,
  21 lanes), with actions pinned by SHA and read-only tokens.

Fix what the scout found:

- **Hard-coded counts.** Page ids live in four tables (`router/route.ts:7`, `app/pages.ts:11`,
  `shell/posters.ts`, `LIGHT_FACES`) plus prose such as "seven posters" in three components
  and a DOM budget fixed at 7. **v2 has one page manifest** (id, title, route grammar, art,
  palette) and everything else is derived from it.
- **Duplicated helpers**: `clamp`/`wrap` re-implemented in four places; `@keyframes breathe`,
  `eyeflick`, `steam` copied across six files. One `kit/motion` module.
- **JS mirroring CSS** (`layout.ts` `rootPx()`). Read layout from CSS custom properties.
- **Two keymap declarations** tied by a compile-time `Same<>` assertion. One declaration.
- **The 400-line cap pushed splitting into 46 and 47 files per page** (`characters`, `review`).
  Keep the cap; add a per-folder file-count review so it does not become file confetti.
- **The fidelity gate** (2.7k lines, needs git-ignored contest data) pins the old poster look.
  Drop it. The replacement is a screenshot test of the two themes on the demo data plane.

### 3.3 Tech choices

| Layer | Overlord | v2 recommendation | Why |
|---|---|---|---|
| Core language | Rust, tokio, axum 0.8, rusqlite | **Keep Rust** | The seams and the test discipline are the product's strength; the rewrite is of structure, not of language. |
| Console | React 19, Vite, CSS modules, custom hash router, hand-rolled external stores | React 19 + Vite, **keep CSS modules and the token pipeline**, adopt a real router only if the page manifest cannot express the route grammar | The hand-rolled router and cache work but are 1.4k lines the product does not need to own. Decide at M1 with the page manifest in hand. `[reconcile]` |
| Stage avatar | three hand-written WebGL2 variants (`glow`, `cut`, `depth`) | **One renderer** (the `cut` look: rim light and shadow on a transparent canvas) with a procedural fallback | Three variants triple the maintenance and none of the extras is demo-critical. |
| Voice | Kokoro via sherpa-onnx, a 3-worker warm pool, "Kokoro Plus" delivery | Provider port with **pre-rendered WAV + visemes as the default** and live Kokoro as a local provider | A clone must speak with no Python, espeak or GPU. Measured RTF about 0.32 per sentence (repo docs, not re-run); TTS competes with WebGL for CPU. |
| Rig | portrait-to-layers + LaMa inpainting (offline Python) | Ship **committed Halloween rigs as assets**; the tool stays an optional offline step | `data/` is gitignored today, so a clone shows a placeholder. |
| Contract | schemars to JSON Schema to json-schema-to-typescript + ajv | Same chain, **registry-derived** | The serde internally-tagged-enum trap (log, 2026-10-23) is already known; wire enums use struct variants. |
| Themes | tokens.source.json (64 colours by hue name) | **Semantic names** (`surface-spirit`, `signal-ghoul`), two theme files rebinding one role layer | See 4.2. |

**Stack decision (2026-10-23): keep Rust + Tauri + React/Vite; switch nothing by default.** For a
local-first open-source app the stack's strength is distribution: one native binary, which
Tauri can bundle with the console, so a user installs one app with no Node, Python or Docker.
The daemon's hard parts (CLI process supervision, WebSocket feeds, SQLite, audio, many
concurrent simulated sessions) are where Rust's compiler pays. The stage stays web either way,
because it is an OBS browser source. Overlord's structural problems came from accretion, not
the language; the v2 fixes above apply in any language. This is a judgement from the scouts'
findings; no alternative was benchmarked.

The rejected alternative, recorded for the reconcile pass: **TypeScript end to end** would
delete the contract pipeline and its traps and let agents iterate without Rust compile times,
at the cost of the single-binary install. Revisit it only if the Oct 30 rules favour a hosted
web demo over a downloadable app `[verify Oct 30]`.

Two decision items, each time-boxed:

| # | Experiment | When | Success | Fallback |
|---|---|---|---|---|
| D1 | **Voice without Python**: run the Kokoro ONNX model in-process from Rust (`ort`, or sherpa-onnx's Rust bindings), visemes included | W1 or W2, 2-3 days | A sentence renders at an RTF comparable to the sidecar's measured 0.32, with no Python on the machine | Pre-rendered WAV + visemes (already the default); the Python sidecar stays an optional local provider |
| D2 | **TS types straight from Rust**: specta or ts-rs (tauri-specta for the shell) instead of schemars, JSON Schema and then json-schema-to-typescript; JSON Schema kept only for runtime validation at the single contract door | M0, before any package depends on the contract | Every wire type, including tagged enums, round-trips through a generated type with no hand-kept list | The current chain, registry-derived, with struct-variant wire enums |

**The Rust cost is DX; budget for it from M0.** Overlord's `target/` is 31 GB, plus 15 GB
across six worktrees, each with its own `target/`. Use one shared cargo target directory (or
sccache) across worktrees and agents, keep crates small so incremental builds stay incremental,
and use `cargo check` in the inner loop with the full gates at task completion. This decides
how fast agents can work in the repo more than the language does.

### 3.4 AI-driven DX

What demonstrably raised quality, to carry as practice:

- **Repo law as a short list** that every builder reads (`.claude/spark/config.md`): the wire
  contract is changed by one owner; stay in your file scope; tests never spend quota; optional
  JSON fields are omitted, never `null`; fabricated data carries `fictional: true`; spawned
  CLIs get a tool allowlist.
- **A "when X changed, run Y" gate matrix**, including live-run smoke tests read from artifacts
  rather than from an exit code, which were the only thing that caught four real defects.
- **Lessons promoted into gates** after a second occurrence.
- **Parallel builders in separate worktrees** with the lead owning the contract.

What does not travel (Claude Code and Personas machinery with no Kiro twin): the symlinked
registry skills, `.ai/conform-runs` (59 generated files, committed), `.claude/scan-sweep`
(66 returns, 36 cards, committed), `context-map.json` (2180 lines, absolute root path),
`.personas/*`. **Do not generate any of this in the new repo.** Hygiene fixes for day one:
runtime output under one `var/` directory (not `runs/`, `ledger.jsonl`, `shadowrunr.toml` at
the root), worktrees outside the repo or ignored, no committed `dist/`.

Registry standards read before writing this plan (logged to `.ai/consults.jsonl`):
`design-tokens` (two layers, themes rebind roles, components never consume primitives),
`demo-data-plane` (one interface, many planes; the fake plane must be unmistakably labelled),
`invariant-placement` (decide the altitude of a rule before choosing its checker),
`agent-instruction-files` (every line in the always-loaded file must earn its place; machine
overviews measured no better than none). Each shapes a section below.

## 4. The product and the Halloween theme

### 4.1 Brand forge (a workstream, not a footnote)

Deliverables by the end of week 1: a **name decision** (after the search in section 1), a mark,
a one-line promise, a voice guide in the lich's register (theatrical, bureaucratic, petty), and
two theme definitions. The brand lives in `packages/brand` (name strings, mark SVGs, voice
rules) so renaming is a one-file change. Until the name is decided the working title is
**Shadowrunr OS**.

### 4.2 Theme architecture

Per the design-tokens standard: raw scales, then semantic roles, with themes rebinding roles
and components consuming only roles.

- `tokens/scales/*` raw ramps, no meaning. `tokens/roles.json` the closed vocabulary.
  `tokens/themes/baseline.json` and `tokens/themes/halloween.json` each bind **every** role
  (a completeness test fails on a missing binding).
- Theme switch is a `data-theme` attribute plus a persisted preference; `prefers-color-scheme`
  and `prefers-reduced-motion` are inputs. A third theme must need no component change.
- **Haunting is function.** Every spooky element is bound to a status role: the candle that
  gutters when the daemon is offline, fog density tied to chat velocity, the on-air state as
  the brightest thing on screen. A `haunt` token axis (fog, flicker, glow intensity) lets the
  baseline theme set them to zero.
- Accessibility floor: body text at least 14px equivalent, contrast checked per theme in a
  test, every animation has a reduced-motion twin (Overlord already has 81 such blocks).
- The **landing contest** (below) decides the art direction. Its winner is promoted under the
  contest skill's two contracts (fidelity and code quality).

### 4.3 The landing contest: shortlisted, decision at project start

**State (2026-10-23): two finalists. The owner decides between them, or combines them, once the
project starts** (Oct 30 or later), as the first design decision of M0.

| Finalist | Concept | Strength | Known defect |
|---|---|---|---|
| **The Spire, Cut Open** (`kiroween-landing` A/1) | The season wall is a lit cross-section of the lich's tower, one illustrated room per page. A lift call-plate navigates, a beacon marks on air, and a cellar soul-furnace is the daemon. | Richest per-page art; each floor is a drawn scene | Type down to 8.1 px, 16% of visible text under 12 px; cartoon-vector look |
| **The Cellar Exchange** (`kiroween-landing-b2-horror` A/1, the winner of that round) | B/2's haunted telephone exchange made real: a 1950s basement switchboard under one bare bulb, with stained plaster, cracked bakelite and brass. It keeps the ON AIR lightbox, CRT, DAEMON dial and patch-cord navigation. When the daemon dies, the bulb goes out and the shadow of an operator who isn't there stands up the wall. | Material realism and dread, with B/2's clear status read; type 12.7 px and up | The intro's skip hint overlaps the jack labels; the dread lives in the daemon-lost state, so the on-air first frame is calm |

How we got here: round one shortlisted A/1 and B/2 (The Switchboard of the Ninth Exchange).
The owner then ran an art-direction round on B/2 alone, asking for more haunting, less
cartoonish, more realism and horror. Its winner, The Cellar Exchange, **replaced B/2** on the
shortlist. The other two directions in that round were not picked: a gaslit painterly 1909
board, and a 1979 cinder-block relay room with a shader-rendered monitor. Its ledger pattern:
*make a liked concept haunting through real materials and light, not new metaphors*.

At decision time: open
`.contest/arena/kiroween-landing/router.html` and
`.contest/arena/kiroween-landing-b2-horror/router.html` (or the vault-wide
`.contest/Landing/router.html`). Then record the final choice on `kiroween-landing`, which is
still open as a shortlist:
- `verdict --winner A/1` if the Spire wins;
- `wrap --close "superseded by kiroween-landing-b2-horror A/1"` if the Cellar Exchange wins;
- a `--combine` with a design doc if they are fused. One plausible fusion is the Cellar
  Exchange as the landing and the Spire's rooms, re-rendered in the cellar's materials, as the
  page scenes.

Then run `wrap` on both contests. Note that `.contest/` is git-ignored and lives only on this
machine.

How the contest ran: two seats, **Sonnet 5.5 at high** and **Opus 5.5 at high**, three
variants each, no judge panel, the owner decides. Opus took 25 min and reported $14.36;
Sonnet took 33 min and reported $4.62. Both seats drew on the bible and converged on two
metaphors (a spire cutaway and a phylactery orrery), so the field held four distinct concepts.

- Arena: `.contest/arena/kiroween-landing/` (git-ignored). The staged material is the current
  console's screenshots (daemon offline, labelled as such), its shell source, the lich's bible
  and the page list.
- Variant 1 of each seat may keep the two-layer concept and haunt it fully; variants 2 and 3
  must succeed the concept, not the layout. The brief names no metaphors.
- The final verdict must be recorded in the contest instrument in the same turn it is acted on,
  or the contest keeps reading as open.
- The winner's art direction feeds `halloween.json`. The two-layer model is the design bar;
  note the first landing contest's ledger (graphic first, nested pages not drawers, survive
  real data).

## 5. Target architecture

### 5.1 Repository layout (new repo, created Oct 30)

```
crates/
  wire/            types, the method registry, schema export        (no deps)
  engine/          Engine trait, providers, ScriptedEngine, fake CLI
  segments/        segment tables, ranks, votes, scoring
  memory/          SQLite store, retrieval
  persona/         character package loading, prompt composition
  moderation/      policy lists, classification, casework
  orchestrate/     the brain's beat, picker, worker pool
  sim/             simulator, judge, sandbox (the only sandbox)
  reconcile/       plan vs shipped
  adapters/        twitch, obs, mcp behind traits; SecretStore trait
  daemon/          axum, typed dispatch, config, thin
  cli/             the `shadowrunr` binary's commands
web/
  console/         React app, page manifest, shell
  stage/           broadcast stage, one avatar renderer
  kit/             motion, keymap, primitives, scene reducer
  brand/           names, marks, voice, tokens + themes
tools/
  voice/           optional local TTS sidecar
  rig/             optional offline rig builder
assets/            characters, Halloween rigs, pre-rendered audio (committed, small)
.kiro/             steering, specs, hooks, settings
docs/              spec, guides, decisions
var/               all runtime output (git-ignored)
```

Layering is enforced by the build (cargo workspace `deny` for crate edges, `eslint-plugin-
boundaries` for web), per invariant-placement: make the wrong import a build failure rather
than a review comment.

### 5.2 Data planes (demo-data-plane standard)

One daemon interface, two planes selected at runtime, not by a build flag:

- **live**: real providers.
- **replay**: recorded shows plus seeded, self-consistent fixtures, served over the same
  endpoints with network-faithful latency. This is the **default on first run**, so a judge's
  `git clone && just demo` works with no credentials, no GPU and no Python.
- The UI shows an unmistakable "recorded, not live" mark in replay; fabricated content carries
  `fictional: true`. Replay is also the screenshot and video source.

### 5.3 Engine providers

`ScriptedEngine` (default) · `Anthropic API` · `Bedrock` · `Claude CLI fleet` (local, optional,
with the allowlist rule). The router picks by role as today. Cost metering and the per-call
token ledger carry over as ideas: the warm-session token-growth defect (7.8k to 500k per call)
is the reason the ledger exists.

## 6. Building it in Kiro

Kiro-native equivalents of what the current workflow does `[verify the exact file formats
against Kiro's docs on Oct 30]`:

| Overlord practice | Kiro form |
|---|---|
| Repo law, gate matrix | `.kiro/steering/{product,tech,structure}.md` always included; per-area steering with file-match inclusion. Keep them short; every line must be something the agent cannot discover from the repo. |
| spark brief and work packages | `.kiro/specs/<feature>/{requirements,design,tasks}.md` with acceptance criteria written to be checkable. One spec per milestone package. |
| "Run the gates" | agent hooks: on save run lint and the size cap; on task completion run workspace tests; on schema change run the contract drift check. |
| Contract regeneration, replay recording | an MCP server only if a gate needs a tool call; otherwise a plain script. |
| Parallel builders in worktrees | one spec per branch, CI lane check as the arbiter. |

Two process rules from the start: **keep a Kiro usage log** (`docs/kiro-log.md`: which spec,
what the agent got right, what it got wrong, a screenshot per milestone) because the write-up
is likely mandatory and is impossible to reconstruct in week 5; and **record the build
dates**, because eligibility depends on what existed before Oct 30.

## 7. Milestones and cut lines (Oct 30 to Dec 4)

The submission slice is what the video and the live demo show. Everything else is the parity
queue and does not block the slice.

| Week | Milestone | Exit criterion |
|---|---|---|
| W1 Oct 30 - Nov 5 | **M0 spine**: repo hygiene, steering, gates at zero and proven red, wire crate with the method registry, engine + ScriptedEngine, daemon skeleton on the replay plane, page manifest, theme system with both themes bound, brand decided, landing picked between the Spire and the Cellar Exchange, D2 settled, shared cargo target dir | `just demo` shows the shell in both themes with replay data; CI green on a fresh clone |
| W2 Nov 6 - 12 | **M1 on-air slice**: landing (contest winner promoted), On Air page, stage with one avatar, pre-rendered voice with visemes, one character (Malakor) | A recorded show plays end to end, voiced, in the Halloween theme |
| W3 Nov 13 - 19 | **M2 mind**: memory, persona, orchestrate beat, segments, Characters and Brain pages | A scripted session runs a segment and the Brain page explains it |
| W4 Nov 20 - 26 | **M3 trials and review**: sim, judge, Session review, Reconcile, moderation | Simulator run scored in replay; review page reads a transcript at real volume |
| W5 Nov 27 - Dec 3 | **M4 polish and submission**: the demo video, README, Kiro write-up, accessibility and reduced-motion pass, a fresh-clone dry run | A submission-ready build by **Dec 1**, with Dec 2-3 as buffer |
| Dec 4 | Submit before 14:00 PT, with time to spare | |

**Cut lines, decided now so they are not decided in a panic.** If M2 slips by more than 3 days,
M3 pages ship as replay-only. If the avatar renderer is not stable by day 12, the stage ships
the procedural silhouette with real audio and the avatar becomes post-hackathon. Live voice,
Twitch, OBS and clips are post-slice by default; they are parity, and they are the work most
likely to fail on camera. Say plainly in the write-up what is and is not live.

## 8. Risks and open questions

1. **Eligibility** `[verify Oct 30]`: "only new projects created during the submission
   period" plus an assets-and-ideas-only port is the cautious reading, and that is the policy.
   Declare the ancestry in the README and ask in `#kiroween-hackathon` on Oct 30 whether reuse
   of one's own earlier art, character writing and designs is allowed.
2. **Assets are the grey zone.** Characters, bible text, rig art and the two-layer idea come
   from Overlord. The decision is the owner's after the rules are out; the plan assumes yes
   for characters and art and no for code.
3. **Parity vs polish.** The Costume Contest rewards the interface. A half-ported seventh page
   costs more than it earns; page seven can be a sealed door with a line of lore.
4. **Demo reliability.** Everything judged must work from replay with no GPU. Live paths are
   bonuses.
5. **Trademark** on the name (section 1).
6. **Unknown judging criteria.** Last year's post is cut off in what we could retrieve; one
   source names "potential value" first. Re-read on Oct 30 and re-weight section 7 if
   usefulness or implementation depth is scored above visual polish.

## 9. Reconciliation: this plan against Overlord a week later

Overlord keeps being developed until Oct 30. This plan is pinned to `8e6586d`.

On **Oct 29-30**, before the repo is created:

1. `git log --oneline 8e6586d..HEAD` and read every commit that touched `crates/`, `apps/`,
   `packages/` or `characters/`.
2. For each, classify: **idea** (a new capability or a changed decision, fold into the specs),
   **fix** (a trap discovered; add to steering or a gate), **noise** (drop).
3. Re-measure section 2's table; re-check each number in section 3 that a commit could have
   moved (daemon LOC and panic count, page count, file-cap violations).
4. Reconcile the open items in `docs/concepts/leftovers.md` and `moderator-twitch-golive.md`
   against the parity queue; anything already built in Overlord and not in v2 is a spec to
   write, not code to copy.
5. Decide the landing finalists (the Spire or the Cellar Exchange, or a combination; section 4.3), record the verdict
   in the contest instrument, and promote the art direction into `halloween.json`.
6. Re-read the stack decision in 3.3 against the rules (downloadable app or hosted demo) and
   confirm D1 and D2 are still the right experiments.
7. Read the rules. Update sections 1, 6, 7 and 8 and delete every `[verify Oct 30]` mark that
   the rules settled.

Anything in this plan tagged `[reconcile]` is a decision that depends on that pass.

## 10. Before Oct 30 (allowed, and not building)

- Register, find the Kiro user ID, join the Discord, decide on a team. Owner action.
- Run the name search. The landing is down to two finalists (the Spire and the Cellar Exchange); the final
  pick waits for project start.
- Prepare assets that already exist (character art, bible, rig images) in a folder that will be
  copied in on day one, with their origin dated.
- Do not create the new repo, the specs or any code.
