# Stage: design notes

The stage is Overlord's broadcast picture: a fixed 1920x1080 canvas that OBS will later capture
as a browser source. Nothing on it is a camera or a real desktop. For now it replays
`StreamEvent` transcripts (contract § transcript) so a simulated stream can be watched.

## Running it

```
pnpm -C apps/stage install
pnpm -C apps/stage dev          # http://localhost:5178/ plays the built-in demo
pnpm -C apps/stage build        # dist/ is relocatable (base "./")
pnpm -C apps/stage test
node apps/stage/scripts/make-demo.mjs   # regenerates fixtures/demo.jsonl
```

| URL parameter | Effect |
|---|---|
| (none) | plays `fixtures/demo.jsonl` (bundled) |
| `?file=<url>` | fetches a transcript; also tries the sidecar `<name>.case.json` |
| `?case=<url>` | the SEG-02A `Case` to draw the hexmap with |
| `?run=<run_id>` or `?run=live` | daemon WebSocket instead of a file; sends `{"subscribe": run, "speed": speed}` |
| `?ws=<url>` | WebSocket URL (default `ws://127.0.0.1:7717/events`) |
| `?broadcast=1` | hides every control (the OBS view) |
| `?speed=2` | playback speed (also 0.25x to 8x in the controls, `[` `]` keys) |
| `?loop=1` | restarts a finished replay after the outro has held for 8 s |
| `?t=<seconds>&paused=1` | deep link to a still frame |
| `?debug=1` | outlines regions and anything that overflows its box |

A `.jsonl` dropped anywhere on the page replaces the transcript; a dropped `.json` that parses as
a `Case` replaces the case. Bad lines are skipped and listed (line number and field path) in the
controls, never half-rendered.

## Code layout: the stage on `@shadowrunr/stage-kit`

The scene model and its components are shared with the console's On Air page, so they live in
`packages/stage-kit`, not here:

- `@shadowrunr/stage-kit`: the reducer (`scene.ts`, `sceneModel.ts`), poll and grade tallies,
  `Timeline` playback, hex geometry, caption timing, the display lore (rank titles and weights,
  grades, foils) and the wire vocabulary lists. Wire types come from `@shadowrunr/contract`,
  generated from `crates/shadowrunr-types`; there is no hand-copied contract here any more.
- `@shadowrunr/stage-kit/ui`: the stage components (caption, chat lane, decrees, vote panel,
  foil card, hexmap and its panels, job ticker, explainer card, overlays) and the art (rank
  sigils, the silhouette).
- `@shadowrunr/stage-kit/parse`: the transcript parser. Every line goes through the contract's
  one door (`parseEvent`), so the stage validates exactly what the console validates.

This app keeps only what is the OBS stage's own: the 1920x1080 composition (`Stage.tsx`), the
region layouts (`layouts.ts`), the sources (`source.ts`: URL parameters, fetches, drops, the
daemon WebSocket), the controls, and `styles.css`, the broadcast skin the kit's class names are
styled by. Colours on that skin that carry meaning (rank, grade) are the token variables from
`@shadowrunr/tokens` (`--sr-rank-*`, `--sr-grade-*`), never local literals.

## Animation: plain CSS, no library

Every animation is a CSS transition or keyframe; there is no animation library.

- **The layout system is a transition problem, not a choreography problem.** A layout is a rect per
  named region (`src/layouts.ts`). Switching layouts changes the rects, and `transition` on
  left/top/width/height moves the same React elements to their new place. Framer Motion's
  `layout` animations would do the same with more weight.
- **Exit animations are not needed.** Every overlay knows its own lifetime from event
  timestamps (a decree lives 9 s, a card 16 s, a redaction 3.8 s), so "leaving" is a class set
  from the playback clock, not an unmount that a library has to defer.
- **OBS's browser source is Chromium (CEF).** CSS animations run on the compositor and stay
  smooth while React renders at about 30 fps; a JS animation loop would compete with the
  playback clock.
- **Bundle:** React plus the contract's compiled validators (the parser's door), about 118 kB gzip in total. Fonts (Cinzel, IBM Plex Sans Condensed,
  IBM Plex Mono) are bundled through Fontsource, so the broadcast never depends on a font CDN.

Framer Motion would earn its weight only for gesture or spring physics, and the stage has
neither.

## Time model

`scene.ts` is a pure reducer: `reduce(state, event)`. It never reads a clock. Everything that
changes over time in the view is derived from the event's `ts_ms` and the playback clock `now`,
for example caption words revealed, a decree's age or a poll's countdown. Seeking therefore just
folds the events up to `t` (`Timeline.advanceTo`), and a still frame at `?t=` is exact. Entry
animations play in real time when an element mounts; durations that matter to the show (who is
on screen and for how long) are all in stream time, so they scale with the playback speed.

## Layouts

| Layout | When | Dominant element |
|---|---|---|
| `boot` | before the first `segment_start` | the intro seal |
| `improv` | `SEG-01` | Malakor's silhouette slot and a large kinetic caption; right column: foil card, vote panel, chat lane |
| `hexmap` | `SEG-02A` | the hexmap (hexes on the left; a lane on the right for the observer badge, decrees and explainer cards, so nothing covers a hex); right column: hex inspector, minion jury, vote panel, chat lane; small Malakor and caption at the bottom |
| `descoped` | `SEG-02B-*` | the `improv` arrangement (these segments are descoped) |

The right column stacks its panels top-down: a panel without content (no poll, no foil) gives
its height to the chat lane, and the change animates. The minion-dispatch ticker spans the bottom
in every layout. The disclosure badge **"AI character · synthetic voice"** sits above every layer,
including the intro and the outro, and is never hidden.

## Event → visual

| Event | Visual |
|---|---|
| `stream_start` | intro seal "Now broadcasting from the Obsidian Spire" (4.6 s); the audience size goes to the top bar |
| `segment_start` | switches to the segment's layout (regions glide to new rects) |
| `handover` | the same layout switch plus a skewed obsidian wipe with the new segment's title and Malakor's reason |
| `segment_end` | toast "SEG-xx sealed · SES +0.42" |
| `chat` | a row in the chat lane: rank sigil, name in rank colour, command chip (`!summon`, `!hint`, `!grade`, `!propose`, `!vote`) |
| `chat` `!vote` / `!summon` | a rank-weighted ballot in the open poll (feeds the rank colour bands) |
| `chat` `!hint <hex>` | a dashed, moving outline and a count badge on that fogged hex |
| `chat` `!grade <hex> <g>` | the minion-jury tally for that hex |
| `picked` | the messages glow as "HEARD" in the lane and lift into "Heard by the Lich" cards with the viewer's sigil |
| `say` (malakor) | kinetic caption: words appear on estimated timing (about 150 wpm, longer words slower, pauses after punctuation); emphasis on caps, numbers and the show's vocabulary; reply chips name the chatters answered; the Phylactery Orb pulses and the eyes brighten while he speaks |
| `say` (foil) | the line, kinetic, in the foil card (SEG-01) or the Skeptical Observer badge (SEG-02A) |
| `vote` | animated bars; length from the authoritative weighted tally, colour bands from the ranks of the ballots seen in chat; the min-rank sigil, a countdown ring; on `closed` the winner is ringed "chosen" and the panel lingers 9 s |
| `moderation` (output) | a "REDACTED BY THE LORD" bar sweeps across the caption area for 3.8 s with the rule id |
| `moderation` (input, with `msg_id`) | the message is struck through as "purged by the Warden" and any decree for it is removed |
| `job` | the minion-dispatch ticker: queued ◇, running ◈ (spinning), done ✓ with its summary, failed ✕ |
| `stream_end` | "transmission ended" line on the outro card |

| Op | Visual |
|---|---|
| `highlight` | the message lifts out of the lane into a "Decree of the Lich" card with the viewer's rank sigil (9 s) |
| `start_poll` / `close_poll` | the vote panel opens in the right column / seals |
| `summon_foil` / `dismiss_foil` | foil card materialises with its emblem, name and tagline (SEG-01); in SEG-02A it shrinks to the Skeptical Observer badge / it leaves |
| `reveal_hex` | the hex burns through its fog (rotate, scale and flash), the grade badge drops in, the ALLEGED stamp lands on anything below DOC, links between revealed hexes light up, and the inspector shows the hex |
| `dispatch_research` | a queued job in the ticker with its brief; later `job` events update the same entry |
| `show_card` | explainer card with an animated diagram picked from the text: `A -> B -> C` becomes a flow, two or more `label: number` pairs become comparison bars, a single percentage becomes a ring gauge, anything else becomes a keyword constellation |
| `mark_moment` | a subtle verdigris flash, a "✦ Clip marked" chip, and a marker on the scrubber timeline (click to jump 3 s before it) |
| `switch_segment` | "⟶ next segment" chip in the top bar until the handover |
| `sign_off` | the outro card: "The ledger is sealed", the reason, minions, hexes, clips, redactions, and "Let the deficit be recorded." |

## SEG-02A specifics

- **Grades:** DOC verdigris `#86d3bb`, REP stone `#cdbf9f`, CLM amber `#e8913f`, SPC flare `#ff5a2a`
  (`--sr-grade-*` from `@shadowrunr/tokens`, the Poster Wall's fidelity palette). Every hex graded below
  DOC carries an ALLEGED stamp, in the map and in the inspector.
- **Case title** in the top bar with a striped **FICTIONAL** tag, and a FICTIONAL CASE corner
  ribbon on the map, whenever `case.fictional` is true.
- **Minion jury:** the community `!grade` counts per hex against Malakor's grade. A hex is
  DISPUTED when at least 3 minions graded it and their majority differs from his grade (a
  simplified form of the digest's "confidence vs jury" divergence).
- **Case source.** The transcript carries the `Case` in a `case_open` event right after the
  SEG-02A `segment_start`, and that case always wins (the latest one at the playback position,
  else the first in the file). Older transcripts without `case_open` fall back, in this order, to
  `?case=`, then the sidecar `<transcript>.case.json`, then a dropped case file, then the built-in
  fictional demo case (a
  byte copy of `crates/shadowrunr-segments/data/demo-case.json`, which simulations run on). The
  built-in case is used only when it contains every hex the transcript reveals. Otherwise hexes
  are placed on a spiral and labelled with their ids, so any transcript stays drawable. The
  controls say which source is in use.

## Rank sigils

Six original SVG sigils (`packages/stage-kit/src/art/Sigil.tsx`) on one faceted seal, so a rank reads by shape first
and colour second. The colours are the Poster Wall's rank palette from `@shadowrunr/tokens` (its
`RANK_COLORS`, which the kit's `lore.ts` re-exports and its `lore.test.ts` pins):

| Rank | Glyph | Colour (`--sr-rank-*`) |
|---|---|---|
| Thrall | a broken shackle ring | ash grey `#8f877a` |
| Acolyte | a candle flame in a dashed circle | stone `#cdbf9f` |
| Cultist | a triangle with a watching eye | pale verdigris `#8fc4b0` |
| Warlock | a crescent cradling a four-point star | verdigris `#3f9c86` |
| Lesser Lich | a phylactery gem under three crown points | amber `#e8913f` |
| Dread Council | three obsidian spires over a ring of six seats | flare `#ff5a2a`, double seal |

Rank titles come from `characters/malakor/character.toml` and vote weights from
`crates/shadowrunr-segments/data/ranks.toml`. Both are copied into the kit's `lore.ts`, and
`lore.test.ts` fails if either copy drifts.

## The silhouette

`packages/stage-kit/src/art/Silhouette.tsx` is a placeholder for the future avatar, drawn from the bible: a
bone-white skull, verdigris-bronze high collar and ribcage, charcoal chancellor robes, a cracked
obsidian crown of floating astrolabe shards, and the green-glass Phylactery Orb. It has no hood,
no ram-skull staff and no purple.

## Fitting the canvas

The canvas is always laid out at 1920x1080 CSS pixels and scaled as a whole
(`fitCanvas`), so smaller windows letterbox it and the layout never reflows. Overflow is handled
by construction:

- every region has a fixed rect;
- variable text is line-clamped or ellipsised;
- the chat lane clips its oldest rows (column-reverse);
- caption size steps down with line length.

`stage.test.ts` checks that every visible region in every layout and flag combination lies
inside the canvas. The `?debug=1` audit flags any element whose content exceeds its box, except
intentional clamps. It was run over 30+ moments of the demo, at 1920x1080 and at a letterboxed
1280x1000, with no findings.
