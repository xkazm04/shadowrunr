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
| `?run=<run_id>` or `?run=live` | daemon WebSocket instead of a file (live mode, below); sends `{"subscribe": run, "speed": speed, "since", "run_id"}` |
| `?ws=<url>` | WebSocket URL (default `ws://127.0.0.1:7717/events`) |
| `?broadcast=1` | hides every control (the OBS view) |
| `?speed=2` | playback speed (also 0.25x to 8x in the controls, `[` `]` keys) |
| `?loop=1` | restarts a finished replay after the outro has held for 8 s |
| `?t=<seconds>&paused=1` | deep link to a still frame |
| `?debug=1` | outlines regions and anything that overflows its box |
| `?voice=1` | voice on without the toggle (the OBS view has no controls); live only |
| `?face=demo`, `?face=rest`, `?face=aa` | dev builds only: drive the face from a scripted line, rest, or one held mouth shape |
| `?avatar=cut`, `?avatar=glow`, `?avatar=depth` | which renderer draws the face (`src/avatar/variants.ts`); default and anything unknown: `cut` (`DEFAULT_FACE`, the owner's pick) |
| `?rig=<url>` | the face rig's base URL (http or https; default: the daemon's `/avatar/` beside `?ws=`), e.g. a static server over a local copy of the rig |

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

## Live mode

`?run=live` (or `?run=<run_id>`, or any `?ws=`) feeds the canvas from the daemon's `/events`
socket instead of a file (`src/live.ts` on `connectDaemon` in `src/source.ts`). A sandbox
session's stage URL is `?run=live&broadcast=1`; `shadowrunr stream sandbox --minutes 5` prints it
(see docs/guide/cli.md), and the daemon must be running at the default
`ws://127.0.0.1:7717/events` or the one `?ws=` names.

- **Resume.** Every subscribe is enveloped (contract § event resume): the first sends
  `"since": 0, "run_id": ""`, and a reconnect sends the last contiguous seq applied and its run
  id, so the daemon continues where the stage left off. A frame at or below that seq is never
  applied twice. A seq that skips ahead counts its missing seqs as gaps; the stage then holds
  later frames and resubscribes from the last applied seq, which backfills the hole. A `busy`
  frame (the daemon cut off a slow consumer) adds its `skipped` count to the gaps and
  resubscribes the same way.
- **New stream.** A different `run_id` in the envelope, or a `stream_start`, starts a fresh
  scene and event store (as the console's `LiveFeed` does), and seq tracking restarts with the
  run.
- **Bounded store.** The scene is reduced one event at a time as frames arrive and covers the
  whole stream. The events themselves live in a ring of the last 2,000, which only the
  controls' debug views and the preview's DVR window (below) read (copied once per change, never
  in the broadcast view). A 30-minute
  session therefore costs the same per event at minute 29 as at minute 1.
- **Acks.** Every 5 s, and as soon as `stream_end` is applied, the stage sends
  `{"ack": {"seq", "gaps", "stalled_ms"}}`: the highest contiguous seq applied, the gaps found in
  this run, and the ms since the last event while the socket is open (0 when it is closed or
  the stream has ended). The sandbox's `stage_ack` check reads these. File and demo replays
  have no socket and send nothing.
- **The clock.** The live clock is the last event's `ts_ms` plus the wall time since it
  arrived (at most 120 s). Sandbox timestamps are wall ms since the start, so it runs in real
  time, and the top bar shows the stream's elapsed time (mm:ss, from `stream_start`, frozen at
  `stream_end`).
- **DVR window (operator preview only).** The live preview can look back over the events the
  ring still holds (`src/dvr.ts` on `LiveStore.sceneAt`). The store keeps the scene after every
  100th applied event as a keyframe (a retained reference: `reduce` shares structure). Keyframes
  leave with the ring and are cleared on a new run or `stream_start`, so about 20 are held. A seek
  restores the newest keyframe at or before `t` and folds at most 100 ring events. At or past the
  head it returns the live scene itself and folds nothing. A time before the window clamps to the
  earliest reachable scene, which is marked hatched on the track. The transport is
  `follow | behind{t, playing}`. Following, the preview is the head. A seek, a clip marker, the
  arrow keys or a pause put it behind. Play advances at the controls' speed (so 2x catches up) and
  returns to follow at the head, and GO LIVE returns at once. The controls show "N s behind
  live". Behind, captions use the estimated word timing, not the voice's audio clock, because
  the voice is live audio. Limits: only the held window (the last 2,000 events) is reachable, and
  the voice is not replayed. The broadcast view (`?broadcast=1`, which OBS and the sandbox use)
  builds no DVR state and always draws the head. Scrubbing never touches the seqs or the acks,
  so the sandbox's `stage_ack` gate reads the same frames either way.

**Feed slates.** They sit on the 1920x1080 canvas, so OBS shows them with `?broadcast=1`; the
controls keep the debug detail (socket status, error frames, frames that failed validation).

| State | What the canvas shows |
|---|---|
| socket down for 3 s | a dim slate "SIGNAL LOST · reconnecting" over the scene (not after `stream_end`: the end card stays) |
| socket open, no event for 20 s | a small "signal stalled · N s" mark under the disclosure |
| `stream_end` without `sign_off` | the end card; with a `sign_off` the outro stays |
| a render throw in the canvas | a calm "The Spire is recomposing" slate with the disclosure; the error goes to the console and the canvas is retried after 5 s |

**Caption queue.** When `say` events burst, Malakor's lines queue rather than overwrite each
other. The reducer keeps a queue (`scene.captions`, read with `captionAt(scene, now)`): a new line waits
until the line on screen has revealed every word and held 2 s, at most 3 lines wait, and on
overflow the oldest waiting line is skipped, never the one on screen. A line said after the
one before has finished shows at once. The schedule comes from event timestamps and the word
timing alone, so replays and seeks show the same captions. `scene.caption` stays the latest
line said, as the console reads it.

## Screens and layouts

The canvas draws `scene.layout`: `boot` before the first segment, then the main screen
(contract `ScreenId`). A segment opens on its default screen (SEG-02A: `hexmap`; every other:
`host`) on `segment_start` and `handover`, so a transcript without `show_screen` still renders;
the runtime sends its own `show_screen` too. Inside a segment `show_screen` switches screens.

| Layout | When | Dominant element |
|---|---|---|
| `boot` | before the first `segment_start` | the intro seal |
| `host` | SEG-01 (and the descoped segments), or `show_screen host` | the big-avatar room: Malakor's slot and a large kinetic caption; right column: foil card, vote panel, chat lane |
| `hexmap` | SEG-02A's default, or `show_screen hexmap [focus]` | the hexmap fills the content area: the overview, or zoomed to the `focus` hex |
| `browser` | `show_screen browser [record_id]` | a staged record in a browser window filling the content area |
| `desk` | `show_screen desk [record_id] [focus]` | the hexmap window behind (top-left, smaller, dimmed) and the browser window in front; the record's hex is lit on the map, which zooms to the `focus` or, without one, to the record's hex |

**The streamer layout** (`hexmap`, `browser`, `desk`; `layout-corner` on the canvas): the content
area is everything left of the right column. Malakor stands cut out over its bottom-left corner
in a 448 px box whose bottom is the canvas's bottom edge (no badge, no backplate; the rig's torso
is cut off at its box's bottom, like a streamer's cut-out, and so is the silhouette fallback's
robe). The caption is a lower third from his right edge to the content's right edge. The foil
badge, decrees and explainer cards float in the content's top-right. The right column is the
chat lane, plus the vote while a poll is open; the hex inspector and the minion jury show on
`hexmap` and `desk` only, and the minion ticker closes the column, so it never sits under him.
The standing card (the segment's commands) shows between explainer cards on `host` and `hexmap`,
not over a browser page.

**No hex under him.** On `hexmap` and `desk` the map is fitted (`fitMap`, stage-kit) into its panel
so that no hex's box touches Malakor's box or, on the desk, the browser window in front; on
`hexmap` it also keeps the overlay lane clear while that costs less than a fifth of the scale.
`hexFrame` (`layouts.ts`) says what stands over the panel; `stage.test.ts` checks the demo case.

**The map's zoom** (`show_screen`'s `focus`; `scene.mapFocus`, read with `mapFocusOf`). The map
zooms to the focused hex and its ring of neighbours (`zoomMap`, stage-kit): the fit is the one for
the hex and all six cells around it, so every focus zooms alike, at most `MAP_ZOOM_MAX` (2.4).
Unlike the overview, the zoom keeps the overlay lane clear as a rule, so no card stands over the
hex he is talking about; with the demo case that is 1.38 against the overview's 0.885 on `hexmap`,
and 1.22 against 0.81 on the desk. The rest of the map dims, and a dimmed hex the zoom would put
under Malakor is not drawn at all, so the no-hex-under-him rule holds while zoomed
(`stage.test.ts` checks every demo hex on both screens). The focused hex gets the foil-coloured
outline. The move in and out is a CSS transition of the map's transform, 0.9 s on the stage's
move curve; under reduced motion it is instant.

**The browser window** (`BrowserWindow`, stage-kit ui) shows the active record: tabs (at most 4,
most recent first: record id and a short title), a URL bar with the source's host and path and a
`retrieved YYYY-MM-DD` or `via Wayback` chip, a FICTIONAL ribbon for a fictional record, and the
page: the grade badge (the grade vocabulary and colours), ALLEGED below DOC, the title, the
summary (clamped) and the hex as a chip with its label. Nothing touches the network. Off air a
"staged copy" note sits in the URL bar.

**Records.** `record_shown` stores a record (at most 32, never an open tab). `show_screen browser`
or `desk` opens the record named, else the one already open, else the latest received. A record
that has not arrived yet is waited for: the screen shows the hexmap, and its `record_shown`
completes the switch (the contract allows either order). `host`, `hexmap` or a handover drop
the wait.

**Moves.** A screen switch moves the same regions to new rects on the 0.95 s curve, with no wipe
(a segment change keeps its wipe). The face is drawn at one size (560 px) and scaled to its box
with a CSS transform on the same curve, so a handover glides him from the host room's box into
the corner without the canvas re-allocating mid-move. Under reduced motion both moves are
instant.

**Where he looks** (`src/avatar/look.ts`, read by the face renderer every frame; `glow` ignores
it): straight at the viewer on `host` (0, 0), toward the content from the corner (0.6, -0.2), and
toward the chat column while the line on screen answers chatters (1, -0.1).

The right column stacks its panels top-down: a panel without content (no poll, no foil) gives
its height to the chat lane, and the change animates. In the host room the minion-dispatch ticker
spans the bottom. The disclosure badge **"AI character · text only"** sits above every layer,
including the intro, the outro and the feed slates, and is never hidden. It reads "AI character ·
synthetic voice" only when a voice is configured (`DisclosureBadge`'s `voice`; the stage's
`VOICE` constant is false, since Malakor has no voice yet). The avatar slot's "placeholder
silhouette" note is a production note, so `?broadcast=1` drops it.

## Event → visual

| Event | Visual |
|---|---|
| `stream_start` | intro seal "Now broadcasting from the Obsidian Spire" (4.6 s); the audience size goes to the top bar |
| `segment_start` | switches to the segment's default screen (regions glide to new rects) |
| `handover` | the same screen switch plus a skewed obsidian wipe with the new segment's title and Malakor's reason |
| `record_shown` | stores the record for the browser window; completes a `show_screen` that waits for it |
| `segment_end` | toast "SEG-xx sealed · SES +0.42" |
| `chat` | a row in the chat lane: rank sigil, name in rank colour, command chip (`!summon`, `!hint`, `!grade`, `!propose`, `!vote`); the row waits until the input filter's beat that judged it has closed (`shownLane`), so a line the filter drops never shows its text, not even for the seconds before the beat |
| `chat` `!vote` / `!summon` | a rank-weighted ballot in the open poll (feeds the rank colour bands) |
| `chat` `!hint <hex>` | a dashed, moving outline and a count badge on that fogged hex |
| `chat` `!grade <hex> <g>` | the minion-jury tally for that hex |
| `picked` | the messages glow as "HEARD" in the lane and lift into "Heard by the Lich" cards with the viewer's sigil |
| `say` (malakor) | kinetic caption, through the caption queue (below): words appear on estimated timing (about 150 wpm, longer words slower, pauses after punctuation); emphasis on caps, numbers and the show's vocabulary; reply chips name the chatters answered; the Phylactery Orb pulses and the eyes brighten while he speaks |
| `say` (foil) | the line, kinetic, in the foil card (SEG-01) or the Skeptical Observer badge (SEG-02A) |
| `vote` | animated bars; length from the authoritative weighted tally, colour bands from the ranks of the ballots seen in chat; the min-rank sigil, a countdown ring; on `closed` the winner is ringed "chosen" and the panel lingers 9 s |
| `moderation` (output) | a "REDACTED BY THE LORD" bar sweeps across the caption area for 3.8 s with the rule id |
| `moderation` (input, with `msg_id`) | when the filter withheld the text (`dropped`, `bleeped`, `tagged`, `priority:safety`, any new action), the message reads "purged by the Warden" and any decree for it is removed; a line it let through (`exempt:*`, `collapsed:*`, `warned`, `ignored`) keeps its text |
| `job` | the minion-dispatch ticker: queued ◇, running ◈ (spinning), done ✓ with its summary, failed ✕ |
| `stream_end` | "transmission ended" line on the outro card; without a `sign_off` before it, an end card "The Spire goes dark" with the same ledger stats |

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
| `show_screen` | switches the main screen (above); `browser`/`desk` open the record in the browser window; a `focus` zooms the map (a new focus on the same screen only moves the map) |

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

It is now the fallback under the face: it shows until the face is drawn, and stays when it cannot
be (no WebGL2, or the form did not load).

## The face and the voice

Malakor's face lives in the `avatar` region, above his caption in the host room and cut out in
the bottom-left corner of the content screens. `?avatar=` picks the renderer (`loadVariant`; the default is `cut`). `src/avatar/` is a glow-rig renderer (WebGL2, one draw per layer, premultiplied light
on a transparent canvas): `Avatar.tsx` is the box, and `runtime.ts` (the renderer, the driver and
the placeholder painter) is loaded with `import()` when the face mounts.

- **Form.** Live, the stage first tries the daemon's rig (`<daemon>/avatar/layers.json` and its
  PNGs). With no rig (404), no daemon, or a spec that does not check, it draws the procedural
  placeholder (`placeholder.ts`): a dark crowned skull, a hinged jaw under an ember seam, two
  verdigris eye lights, five floating crown shards, an ink collar and an ember orb, painted on
  canvases in the browser from the stage tokens. No image file is committed. Anything but a plain
  404 is logged once with the reason. `data-form` on the face says which one is drawn.
- **Motion.** `driver.ts` is seeded and procedural: breath, sway, blinks, saccades, nods on stressed
  vowels, and the mouth from the viseme (or from loudness alone). The face runs on one frame clock
  and stops asking for frames once it has settled. Under reduced motion it is one still frame.
- **Signal.** The face reads a `FaceSignal` from `FaceMixer`: the voice while it sounds, else the
  level-less speaking phase while a `say` caption is on screen, else rest.
- **Voice** (`src/voice/`, live only, off by default). The Voice toggle in the controls turns it on
  (remembered in localStorage) and unlocks the AudioContext inside the click. The stage connects
  to `<daemon>/voice` and sends `{"follow": {"run_id"}}` for the run it is showing. Each
  `utterance` is decoded from base64 PCM16 into an AudioBuffer and scheduled back to back on one
  AudioContext. A frame at or behind what was accepted is dropped. Past 6 s of queued audio, the
  oldest unstarted utterance is dropped and the rest close up. **The audio is the one clock**:
  the mouth shape is read at `currentTime` minus the utterance's start (35 ms ahead), and the
  loudness comes from an AnalyserNode. A headless browser is never voiced.
- **Disclosure and status.** While the voice is on, the badge reads "AI character · synthetic
  voice" and a "synthetic voice" label sits under the face. The controls show the voice state
  (off, starting, ready, degraded) with its reason.

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

## Clip mode (vertical drafts)

`?clip=<url>` loads one clip candidate's `clip.json` (written by `crates/shadowrunr-clips`) and
draws it on a 1080x1920 canvas instead of the broadcast (`src/ClipStage.tsx`, `src/clip.ts`,
`src/clip.css`). Top to bottom: the header (ALLEGED, the grade badge, the case title and a
FICTIONAL DEMO CASE chip for SEG-02A or case-linked clips; otherwise the segment title and
foil), the hexmap or the avatar with the foil card, the exact transcript captions word-timed
by stage-kit's caption model, the citation strip, and the AI disclosure with the clip's
provenance. Chat never appears: handles stay in the room.

`&record=1` stops the wall clock. `window.__clip.seek(ms)` commits a frame, paints twice and
settles every CSS animation at a time derived from the clip clock, so frames are
deterministic. `scripts/record-clip.mjs` (no dependencies) serves `dist/` and the clip,
drives a headless Chromium over the DevTools protocol frame by frame and pipes the frames
into ffmpeg (libx264, yuv420p), plus a poster PNG:

    node scripts/record-clip.mjs --dist dist --clip <clip.json> --out <draft.mp4>       --poster <poster.png> --chrome <chrome or msedge> --ffmpeg <ffmpeg> [--fps 30]

