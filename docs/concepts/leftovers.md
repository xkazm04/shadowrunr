# Leftovers from the implementation plan

What the plan asked for and main does not do yet, checked against the code at `5cec36f`
(2026-10-23). Sources are the four-topic plan of 2026-10-21 (custom voice, segments vs the
background screen, humour tone, character parametrization), the voice-providers work of
2026-10-22/23, the program digest ([obsidian-program-digest.md](../design/obsidian-program-digest.md)),
and the follow-ups from the last recorded and voiced shows.

Sizes: **S** is a focused change in one or two files, **M** a package of a day or so, **L** a
feature of its own that needs design first.

## Start here

The five that buy the most next:

1. **The owner's first directions** ([owner-review-loop.md](owner-review-loop.md)): the tools
   exist; the content doesn't.
2. **A repeated question can starve fresh chat** during a case beat (Show and segments).
3. **Case names count as worn phrases** in the tic share (Show and segments).
4. **The clip caption can cite a hex that was only mentioned** (Recording and clips).
5. **Write the headless stage setup down** (Platform and ops): without it, a voiced validation
   measures the test browser, not the voice.

## Voice

| Item | Evidence | Size |
|---|---|---|
| **Quality revisit.** Kokoro Plus is the development voice; the ElevenLabs designed voice is the bar. Decide later whether local reaches it or real streams use ElevenLabs. | owner decision, 2026-10-22 | M |
| **ElevenLabs lip-sync.** The provider's character timings are counted, not mapped; the mouth runs on loudness. | `crates/shadowrunr-daemon/src/voice/elevenlabs.rs` sends no visemes | M |
| On a fresh daemon `voice.settings.get` labels an ElevenLabs voice "voice 43UFEk…" until a set, verify or render reads its name. Look the name up at start or in `get`. | `voice/settings.rs` `settings`; only set and verify call `lookup_name` | S |
| Fast pace factor is 1.04; the confirming show measured 1.01. Re-measure over more fast lines before changing. | `crates/shadowrunr-brain/src/pace.rs` `PACE_FACTOR_FAST` | S |
| The package's `[voice] speed` is parsed but Kokoro Plus takes speed per line from the cue. Decide whether it scales the cue. | `voice/settings.rs`; `tools/voice/shadowrunr_voice/plus.py` | S |
| Two recorded talking-avatar deviations: the stage drops a whole utterance on bad visemes; the daemon forwards a partial timeline. | `apps/stage/src/voice/frame.ts`; `voice/sidecar.rs` | S |
| Fine-tuning an original voice (custom-voice phase 2). | absent | L |
| Foil voices: `Speaker::Foil` exists, the voice service doesn't speak it. | `crates/shadowrunr-types/src/event.rs` | L |

## Show and segments

| Item | Evidence | Size |
|---|---|---|
| **Case beats and repeated questions.** A case beat caps the turn at one pick; carried messages gain an age bonus and repeats aren't deduped, so an old repeated question can keep taking the only slot. | `crates/shadowrunr-brain/src/orchestrate/mod.rs`; `picker.rs` | M |
| **Judge fails in the last voiced show** (sbx-20261022-233457-10m): `bit-are-you-real-lich` unanswered, `celebrity-impression` picked but not aired. Partly code (the pick), partly directions. | the run's `judge.json` | M |
| **Tic share 41–45%.** The detector counts the case's proper nouns ("bone clerks", syndicate names) as tics, and the names are also really repeated. Exempt case names from the detector, then direct the repetition. | `crates/shadowrunr-brain/src/working.rs` | S–M |
| No structured case state in `SegmentContext`; `case_names.rs` parses the state summary text. | `crates/shadowrunr-types/src/segment.rs` | M |
| The disclosure is one string, with no theatrical variants. | `crates/shadowrunr-types/src/character.rs` | S |
| An act opening doesn't move the board's focus; the act card still says "Now open on the map". | `crates/shadowrunr-segments/src/runtime.rs` | S |
| No rule makes the latest poll winner the next reveal. | `runtime.rs` (ties and crowd votes are done) | S |
| Bait priority has no cap per window, only one injection per beat. | `crates/shadowrunr-brain/src/picker.rs` | S |
| **SEG-03 (Table Read) follow-ups** (2026-10-24, smoke runs `sbx-20261024-*-10m`): the judge scores scripted scene lines on chat facts (`responsive_to_chat`), which drops the quality mean; exclude `scene_start`..`scene_end` lines from chat facts. The guest spoke only in the intro in the last run, none in the short post-scene talk. Honesty measured 0.42-0.64/3; the A/B (`sim/matrices/seg03-honest.toml`) has not run. The SEG-01 handover can fall due seconds after a foil is summoned. | `crates/shadowrunr-sim/src/judge.rs`; `crates/shadowrunr-brain/src/scene.rs` | M |
| SEG-03 clips still reframe on the host's face and can crop the guest out of the two-shot. | `crates/shadowrunr-clips/src/window.rs` | S |
| The reconcile planner's plan and directive schemas offer only SEG-01/SEG-02A (deliberate: SEG-03 needs a prep's `table_read`). | `crates/shadowrunr-reconcile/src/synth.rs` | S |
| Owned requests drop when their 25 s carry window ends, not after the intended 90 s. | `crates/shadowrunr-brain/src/orchestrate/mod.rs` (commit filter) | S |
| The ledger's `cost_usd` is the CLI's running `total_cost_usd` per warm session, so summed costs overcount (about 3x in the SEG-03 smoke runs). | `crates/shadowrunr-engine/src/stream.rs:215` | S |

## Behaviour and tone

| Item | Evidence | Size |
|---|---|---|
| **No owner directions yet.** `directions.md` is the seed draft; `characters/malakor/segments/` doesn't exist. | [owner-review-loop.md](owner-review-loop.md) | owner |
| <a id="owner-review-loop"></a>Four of the eight Review marks (broke_character, off_tone, repetitive, wrong_tag) and notes without a mapped mark feed nothing; no mark reaches the judge or the rubric. | only `crates/shadowrunr-daemon/src/tone/inbox.rs` reads `feedback.jsonl` | S–M |
| Review gaps: sim and tournament runs can't be opened; **Direct** is only in the player; the UI doesn't say a direction applies from the next show. | `crates/shadowrunr-daemon/src/review/mod.rs` (sandbox ids only); `apps/console/src/pages/review/Direct.tsx` | M |
| Directions have no in-app history or undo; git is the history, and nothing commits them. | — | M |
| Sim archetypes in `sim/calibration.json` are still written in Malakor's words, so a second character is tested against his audience. | `sim/calibration.json` | S |
| The vault's per-tone review files have empty "Your directions" sections and nothing reads them; the Tone Lab replaced them. Close them out. | vault `tones/` | S |

## Character parametrization

| Item | Evidence | Size |
|---|---|---|
| A console wizard to create a character. | absent (non-goal "later") | L |
| A safety review for user-submitted characters. | absent | L |
| A sandbox show can't pick its character; it uses `[sim] character`. | `crates/shadowrunr-sim/src/sandbox.rs` | S |
| Malakor's tagline isn't set (the field exists). Proposed: "High Auditor of the Obsidian Spire". | `characters/malakor/character.toml` | S, owner |

## Recording and clips

| Item | Evidence | Size |
|---|---|---|
| **Clip caption cites the wrong card.** A clip's card and source come from reveal ops plus any hex named in a spoken line, never from `RecordShown` or `LinkPinned`. Likely cause of the show-3 clip #1 caption. | `crates/shadowrunr-clips/src/scan.rs` | M |
| A pinned string below DOC grade doesn't count toward the clip's worst grade. | `scan.rs` reads only `RevealHex` | S |
| The board card's command rows are clipped when the slot is short ("THE MINIONS MAY" is the whole title; the rows below it are cut). | `apps/stage/src/OnAir.tsx`; `.standing` in `styles.css` | S |
| OBS shows a crash dialog on launch: no launcher, no crash-marker cleanup. Root cause unverified. | [recording.md](../guide/recording.md) documents `--disable-shutdown-check` | S |
| No 15 s broadcast delay and no BRB panic scene. | `crates/shadowrunr-daemon/src/record/mod.rs` | M |
| Upload is dry-run only. | `crates/shadowrunr-daemon/src/clips_cli.rs` | L |

## Platform and ops

| Item | Evidence | Size |
|---|---|---|
| **Headless stage setup for voiced validation.** A headless browser draws the stage's WebGL in software and takes about 60% of the CPU, which tripled first audio in one validation. Open it with Chromium's full channel (`channel="chromium"` in Playwright), `--use-angle=d3d11 --enable-gpu`, and an override of `navigator.webdriver`, which the stage otherwise treats as headless and refuses to voice. OBS's browser source is unaffected. Move this into a script or the CLI guide. | `apps/stage/src/App.tsx` (the webdriver check) | S |
| Starting the console isn't in any guide (it's in [owner-review-loop.md](owner-review-loop.md) now). | `docs/guide` | S |
| Real Twitch/YouTube chat ingest, and a platform ban adapter (`crisis-chat-ban` only records). | absent | L |
| The local fallback lane is a stub. | `crates/shadowrunr-engine/src/fleet.rs` | L |
| Directive proposals never expire. | `crates/shadowrunr-reconcile/src/directives.rs` | S |
| No memory reindex at daemon start; preflight only hints. | `crates/shadowrunr-daemon/src/preflight.rs` | S |
| The sim instrument hash doesn't cover brain code; `SIM_LOGIC_VERSION` is bumped by hand. | `crates/shadowrunr-sim/src/instrument.rs` | S |
| Review: `SessionTags` has no `calls`; the empty wall has a hint but no button; the ledger mapping is duplicated. | `crates/shadowrunr-types/src/review.rs`; `apps/console/src/pages/review/Layer1.tsx` | S |

### Not started from the program digest

SEG-05 (the Celestial Tribunal), the Morning Sheet, `minion_dossier`, the Grudge Graph, the
nightly batch, a memory MCP, the 12-week season arc, and Showrunner OS tenancy. Each is **L** and
needs its own design. SEG-02B stays descoped.

## Owner decisions waiting

- **Approve the real case** HARD-MONEY-20261019 (still `CALM_STAGED`): `shadowrunr case approve`.
- **The tagline**, as above.
- **Voice for real streams:** the quality revisit and a go or no-go on fine-tuning.
- **Chatting-share calibration** is assumption-grade until Season 0 data replaces it
  (`sim/calibration.json`).
- **The first behaviour and segment directions** ([owner-review-loop.md](owner-review-loop.md)).
