# Territory dossier - Character Brain (crates/shadowrunr-brain, characters/malakor)

Commit: 24039f7 - written 2026-11-07

Paths below starting `src/` are relative to `crates/shadowrunr-brain/`.

## Why this area

Shadowrunr OS is "an open-source runtime for AI characters that stream", and its flagship is
Malakor, a theatrical skull-faced sorcerer-villain who treats viewers as minions (README.md:1-11).
Everything else (segments, stage, voice, clips, console) is furniture around one asset: a
character viewers come back for. The Character Brain is that character's mind (the core thread,
picker, orchestration, pace, memory and moderation) plus the character package it composes from.
Hunt evidence (`tiger.mjs hunt`, 180 days at 24039f7):
- context-map group "Character Brain": 388 commits, 47 files, 1.2 MB, the third-hottest group;
- `crates/shadowrunr-brain/src` is the hottest source directory after generated contract code and
  console pages (310 commits);
- the bp product tree marks `malakor-core-brain` and `malakor-character-package` as major features.

The operator chose this area over Show Segments and Simulation on 2026-11-07.

## Who uses it, for what

- **Viewers** (adult Twitch-style audience). They come for Malakor: a recognisable, funny,
  continuous villain who answers a few chat lines well, remembers regulars, runs the night's
  segment and obeys votes with theatrical reluctance (characters/malakor/constitution.md:16-38).
  Done well means the show stays on the segment instead of a topic-switching roast show, he does
  not overreact, the humour lands, and he stays himself all night and across nights.
- **The owner/operator.** Tunes Malakor through the tuning loop (docs/concepts/malakor-tuning.md),
  directs him via the Lord's brief, `directions.md` and segment directions, toggles optional
  rules, and reviews sessions in the console.
- **The simulator** (crates/shadowrunr-sim). Runs the brain against simulated audiences of
  3, 30 and 300 chatters with an LLM judge. This is the only place the brain is exercised today,
  because v0.1 is "the media-less phase" (README.md:13-14).

## What it does today

Construction happens at `Brain::new_with` (src/lib.rs:346). It refuses a moderator package (:352),
applies the tone's reply shape to pace (:354), opens the MemoryStore (:355), loads the policy and
persona gate (:357-358) and builds the stable prefix (:359). `with_brief` re-renders the prefix with
the Lord's brief (:428-431).

One beat is `Brain::tick` (src/lib.rs:1084). The module doc summarises it at :4-30.
1. Fleet health check and possible sign-off (:1095-1108). Working-set expiry, and finished
   research jobs are collected without awaiting them (:1144-1145, collect_jobs :676).
2. Chat pre-filter. Care ledger (:1149), `InputFilter::filter` (src/moderation.rs:1096) for
   injection, flood and rate caps, safety picks that start care (:1153-1157), repeat screen (:1158).
3. The orchestrator plans a beat mode: cold open, mixed, react, bit, bridge or hold
   (src/orchestrate/mod.rs:1-40; :1188-1202). The picker's `pick_upto` (:1236) is deterministic
   at 40 candidates or fewer and uses an LLM above that (src/picker.rs:66-68, :770). Its score
   components are at src/picker.rs:300-401, and its order is safety, greet, one owed request,
   carried bait, then a novelty pick every 3rd (:8-26). Hold and Scene beats return with no
   core call (src/lib.rs:1267-1285).
4. Recall (src/lib.rs:1292-1364):
   - 5 facts per picked viewer;
   - the top 3 facts each for the Character, World and Case scopes;
   - an FTS fact search (8 hits);
   - the top 5 procedurals ("When X, do Y" habits, :1338);
   - an episode search, the 6 most recent episodes and this stream's unflushed turns.
5. Compose (src/compose.rs:942).
   - System prompt = the byte-stable prefix (src/compose.rs:61-98): constitution, enabled rules
     sorted by id, tone, directions.md, bible, then the fenced Lord's brief (:103-110). A golden
     test pins it byte-identical across beats (:4-8).
   - The user message is assembled in a fixed order: segment rules, state and directions; viewer
     briefing; working set; memory fitted to 24,000 chars and nonce-fenced (:1006-1038); tonight's
     material; through-line; case; "your voice" (worn-out phrases, repeated openers); chat (care,
     room notes, safety directive, fenced picks); "how to answer" (sentence and char budget,
     delivery cues, OP:/FOIL/NOTE formats, :1115-1156).
6. Core call: a warm session recycled every 6 turns (src/lib.rs:198-199, :767-800), Sonnet tier
   by default. `stall::cover_call` (:1501) covers slow calls. On a fleet error the beat is rolled
   back transactionally and a model-free stall line airs (:1531-1542, :718-734).
7. Output.
   - `parse_reply` (src/grammar.rs:141): plain line plus `[emotion]` cue, `OP: {json}`, `NOTE:`,
     `FOIL <id>:`.
   - The pace budget drops whole lines, last first; crisis and owed beats are never trimmed
     (:1555-1670). A lag ceiling drops more (:1697-1716).
   - `outgate::pass` runs on all 5 surfaces (:1796, 1861, 1892, 1917; exhaustive match at
     src/outgate.rs:33-44).
   - Then: brigade refusal (:1989-2025), a guaranteed crisis help line (:2034-2062), the crisis
     ban record (:2068-2089), and re-carry of picks whose answer was trimmed (:2144-2172).
8. Working memory update (src/working.rs, in memory, 1 h TTL): jobs, open poll, active foil,
   recent lines, tics, names. `record_turn` (:739) stores turns, with disclosure text replaced
   (:1466-1467). `flush_turns` writes a Stream episode every 5 ticks (:2178-2183).
9. Long-term memory (src/memory/store.rs:1-16): markdown files are the truth, SQLite FTS5 is a
   rebuildable index, with `_history` and `_tombstones` kept alongside. `end_stream` (src/lib.rs:2207)
   decays and runs consolidation when due:
   - trigger: 40k chars of new episodes, at least 6 h apart, 72 h valve
     (src/memory/consolidate.rs:21-30, :128);
   - input up to 120 episodes or 30k chars; output at most 12 facts and 6 procedurals;
   - every item must cite a shown episode id; uncited or tombstoned items are rejected (:290-482).

LLM roles (crates/shadowrunr-engine/src/config.rs:14-29):

| Role | Tier / effort / timeout | Output |
|---|---|---|
| CoreThread | Sonnet / low / 30 s | free text in the grammar |
| ChatPicker | Haiku / low / 20 s | `{picks}` schema |
| Researcher | Sonnet / medium / 240 s, WebSearch and WebFetch | free text |
| Consolidator | Sonnet / medium / 300 s | facts and procedurals schema |

The researcher runs through a bounded pool (src/pool.rs:91-125).

Callers: the sim builds the brain (crates/shadowrunr-sim/src/runner.rs:1011-1034) and ticks it
(:772, end_stream :862), and so does the sandbox (crates/shadowrunr-sim/src/sandbox.rs:1320-1330).
The daemon never builds `Brain` itself; it runs the sim or sandbox (crates/shadowrunr-daemon/src/live.rs:357).

**What it does unusually well (evolve these):**
- the byte-stable, cache-friendly persona prefix with a golden test;
- nonce-fenced untrusted spans (chat, memory, material), with injection text withheld from the
  prompt (src/lib.rs:1316);
- transactional beats that un-consume orchestrator state on failure;
- deterministic budgets that drop whole lines, never truncate them, and carry unanswered picks forward;
- a deterministic picker with a legible score and an LLM only at scale;
- markdown-as-truth memory with citation-gated consolidation and tombstones;
- a guaranteed crisis pointer, with disclosure text that never reaches memory;
- `private/` never read, enforced by tests/private_never_in_prompt.rs;
- model-free stall lines that cover fleet failures;
- tuning values tagged measured vs assumption with their source run (characters/malakor/character.toml:92-101).

**Character package** (characters/malakor/):
- constitution.md, 81 lines, immutable: awareness, voice, red lines, one owner-approved satirical
  exception at :64-67;
- bible.md, 234 lines;
- 5 tones (md + toml), with unhinged-roastmaster the default;
- 6 foils, 3 rules (crisis-handoff is an always-on rail);
- effects.toml (effects and a behaviour profile replaced catchphrases);
- stalls.toml, directions.md, segments/SEG-03.md;
- situations.json, 29 situations, read only by the sim's chat and judge (src/character.rs:54-118),
  never by the prompt.

## Measured state

- Tests: `cargo test -p shadowrunr-brain` at 24039f7 (run 2026-11-07) gives 421 passed, 0 failed,
  1 ignored. The ignored test is `no_private_sentence_reaches_the_prefix_or_the_turn`, which fails on
  27 rule and behaviour sentences that private/ quotes. Property suites:
  - orchestrate_props: 5
  - pace_props: 20
  - picker_props: 18
  - all proven red on seeded faults (commit 37b03e1)
- Size: brain src is 36,346 lines. moderation.rs 4281, orchestrate/mod.rs 2757, lib.rs 2233,
  compose.rs 1915, character.rs 1869, memory/store.rs 1654, picker.rs 1372.
- humor_landed, the judge rubric's weakest dimension. These are reports in `runs/sim/*/report.md`,
  quoted from files and not re-run here, since the tournament ban forbids new tournaments.
  Instrument hashes differ, so the rows are not comparable to each other:

  | Run | Date | humor_landed | Notes |
  |---|---|---|---|
  | rebalance-check-34188c4a83 | 10-25 | 66% | n=113 |
  | credit-smoke-1998a8a03d | 10-29 | 64% | n=14 |
  | pilot-1998a8a03d | 10-29 | 59% | n=94 |
  | credit-raid-1998a8a03d | 10-30 | 52% | n=95 |
  | credit-seg02a-1998a8a03d | 10-30 | 46% | n=112, CI 37-55%, 5 of 10 runs void |

  The other rubric dimensions run 95-100%, with zero red-line failures in all of these.
- Tuning ledger docs/tuning/LEDGER.md: 26 rows (2026-10-25 to 10-30), none reverted. Bar cells are
  20/20 except one neighbour at 19/20 (tuning-808c3484 SEG-02A.a30.s1). BATCH-1 summary in
  docs/tuning/BATCH-1-SUMMARY.md.
- Open tuning leads (BATCH-1 §4):
  - dread-council proposal verdict content;
  - a raid landing in a 29 s speaking hold goes to a carried newcomer;
  - a real gift capped by carry_max;
  - case lines lost to the pace trim on dispatch, reveal and show beats;
  - the "with ceremony" echo;
  - no sincere-doxx-report stimulus in the sim.
- Leftovers (docs/concepts/leftovers.md):
  - repeated questions starving case beats (:41);
  - tic share at 41-45% (:43);
  - a refused celebrity impression offers no foil (:42);
  - owned requests dropping after 25 s instead of 90 s (:52);
  - SEG-03 honesty at 0.42-0.64/3 with its A/B not run (:49);
  - four of the eight Review marks feed nothing (:60).
- Registry deviations (.ai/registry-map.json, recorded; none conformant):
  - agent-memory: procedurals are never decayed, superseded or retired, yet the top 5 are injected
    every beat (src/memory/store.rs:920).
  - companion-identity: the citation gate checks only the derived index, not the markdown truth
    (store.rs:325).
  - on-stream-persona-continuity:
    - directives are admitted to the brief without a conflict check against the constitution
      (src/compose.rs:322);
    - the default tone has no measurable retirement threshold (tones/unhinged-roastmaster.md:48).
  - companion-runtime: turn events are returned after the turn instead of streamed (src/lib.rs:557).
  - conversation-orchestration: no inspectable disclosure of the memories recalled into a turn
    (src/lib.rs:996).
  - agent-runtime-assembly: research jobs are volatile tokio tasks, lost on restart (src/pool.rs:216).
  - live-safety-rails: the crisis care window is 30 min (src/care.rs:16), not the rest of the stream.
  - companion-identity (package): bible.md:219 restates and extends a constitution red line.
- Not measured: live-stream latency to first word, anti-repetition across a 4 h stream, memory
  recall usefulness across streams (no multi-stream sim exists), cost per stream hour.

## Recent history

All 114 brain and character commits since 2026-08-01 fall between 2026-10-18 and 2026-10-30.
None are reverts.
- Foundation:
  - e959d54, b10641d: the package with tones, foils and situations;
  - ef26226: memory and the anti-bait picker;
  - 4e2e94a: the care state;
  - d00e2f4: unhinged-roastmaster made the default tone.
- Memory hardening:
  - dfdeddd: nonce-fenced recall;
  - 4937e62: disclosures kept out of the record;
  - c7ec84d: bounded per-viewer state;
  - 5da34df: sleep reads skipped episodes.
- Orchestration:
  - 9413ac4: the orchestrator;
  - cd4b65d: beats paced to his speech;
  - e523f3b: continuity across pauses;
  - f2e09d2: one output gate;
  - f91ca5e: the investigation loop.
- Package v2: c7771d2 ("an engine with no Malakor in it"); b267ce3 replaced example lines with
  effects and a behaviour profile; 5eb1b7c segment-led beats.
- SEG-03 Table Read: e5fa724, f3d6e9d, plus the Hinkel parody guest in 29f8d8b.
- Tuning-loop fixes, 10-25 to 10-30: brigade refusal, gift handle, claim-as-fact, raid framing,
  `!propose` (48e823c, 30aaf13), pin lines (00f12eb), crisis handoff (9ddb561).

**Rejected or dropped directions. Do not re-propose these:**
- **Real-person impression voice segment.** Dropped by ear 2026-10-24 (.spark/Spark/ideas/impression-voice-spike.md);
  the models were deleted. No impressions of real people.
- **Theatrical as default tone.** The owner kept unhinged-roastmaster (.spark/Spark/morning-decisions.md:170-174).
- **Catchphrases or signature lines as a mechanic.** Removed in the rebalance (7e4b6f8): "overreactions
  are destroying the stream", "it should never turn to a roast show switching topic each reply".
- **Loading bible examples or anything in `private/` into prompts.**
- **Running new tournaments.** Banned until the owner lifts it. Mine existing runs, re-run only
  what a fix targets, at most 3 judged runs per fix (docs/concepts/malakor-tuning.md:11-13).
- **Making the judge easier** in any form (malakor-tuning.md:75-88).
- **Porting to the v2 rebuild repo.** That is a separate plan (docs/concepts/kiroween-shadowrunr-v2.md),
  out of scope here.

## Known limits and open questions

- Owner complaints:
  - the opening is too chat-driven; he should open with a prepared story or wait (morning-decisions.md:264);
  - overreaction to viewers;
  - refusing too often (asked-for roasts, dark jokes, grief), partly addressed in a397c61.
- Humour is the one rubric dimension that fails, and it fell from 66% to 46% across recent runs
  on differing instruments. Nobody knows yet whether that is content, timing or the judge.
- Memory across streams is built but unexercised. No sim spans more than one stream, so whether
  remembered regulars, facts and procedurals make him better on night 2 is unknown.
- Orchestrator, agenda and case thresholds are admitted assumptions, not measurements
  (src/orchestrate/mod.rs:140, :170-271; agenda.rs:21; case.rs:42-58).
- Case state is parsed from summary text, and a structured state is "the planned replacement"
  (orchestrate/case.rs:39-40). Room events break if state-text wording changes (room.rs:6-9).
- Delivery-cue repetition is "recorded, not enforced" (src/lib.rs:1816). Tic share is 41-45%.
- No platform adapter can ban yet; the ban event is a record a future adapter will act on (src/lib.rs:2064-2067).
- A scan-sweep direction is still open: "a package-driven brain hard-codes Malakor in six production
  sites" (.claude/scan-sweep/findings/brain-composition-memory.jsonl).

## Constraints

- Stack: Rust workspace. Gates are `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`
  and `cargo test --workspace`. Tests use the scripted engine and a fake CLI and never spend quota.
  Contract drift: a Rust type change that reaches `shadowrunr-types` must regenerate
  packages/contract (`pnpm -C packages/contract check`).
- The constitution is immutable to every in-band actor, including tuning passes and memory; only
  the owner edits it, through a reviewed commit (constitution.md:3-4). Red lines are a 100% bar.
- `private/` is never loaded into a prompt.
- The judge is never made easier. Instrument hashes are never mixed in a comparison.
- No new tournaments. A proposal that needs measurement must work from existing runs, or from at
  most 3 targeted judged runs, or a deterministic test or instrument.
- The owner-approved satirical guest (SEG-03, Hinkel) keeps its rails. Never write real
  copyrighted scripts.
- Keep tests quota-free (scripted engine). The brain stays package-driven: Malakor specifics
  belong in characters/malakor, not in the engine.
- Out of scope: the console UI, stage and avatar rendering, the voice sidecar, the v2 rebuild,
  and Twitch platform adapters (except as a seam the brain emits to).
- LLM-powered area: see C:/Users/kazda/.claude/skills/tiger/references/llm-lens.md for the engine
  checklist lens.
- Registry standards that govern this area: media-generation/on-stream-persona-continuity
  (anti-repetition-window, cited-memory-with-tombstones, owner-brief-below-the-constitution,
  per-turn-persona-anchor, tone-as-falsifiable-bet), software-engineering/agent-memory and
  companion-identity. A recorded deviation (above) is a known gap that may be proposed as a fix.
