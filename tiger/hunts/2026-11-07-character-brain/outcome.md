# Outcome - Character Brain hunt (2026-11-07)

Run branch: `tiger/character-brain`, from main `f7fd3d2` (dossier commit 24039f7 plus one unrelated
registry commit; no brain or character changes in between). Built in two lanes split by write set:
- lane A: C2, C1, S1, S3
- lane B: C3, L1, L2

Each lane was merged and gated on the run branch.

## Items

All verify checks are deterministic, with scripted or fake engines and no quota. Tallies come from
the caller's own full run of `cargo test --workspace` on the merged branch at d7f0fd2.

| Item | Commit | Verify (deterministic) | Result |
|---|---|---|---|
| defect (found while judging) | d882e7b | viewer facts render inert inside the memory fence; injection-shaped fact test | landed; compose::tests 13/13 |
| C2 case move keeps its line | 95227c8 | 4 ops x normal/trimmed/lag/outgate reject/core fail; every emitted op has its aired line; a deferred op airs once; retry bound | landed; tests::delivery 7/7 |
| C1 duty lines as jokes | a486b8b | every beat_plan has a source; a package with no source effects composes byte-identical prompts; PURPOSE_NOTE at most 3 in 24 beats; 90 s CTA cooldown from aired lines only; no effect on care/crisis; miner matches a hand tally and refuses mixed hashes and over 10% unclassified | landed; tests::sources 7/7, mine:: 3/3 |
| S1 case names not tics | 4ec7b32 | no case name in the worn list; `tic_flags` untouched | landed; working::tests 9/9 |
| S3 marks become effect cards | 9277b00 | exhaustive mark roles; 3 marks give 1 card, 2 give none; no marked text in a card | landed; tone::inbox 2/2 |
| C3 memory truth and freshness | c1e0361 | exposure cannot hold off decay (60 simulated days); topic selects habits, at most 3; deleted source or tombstone suppresses recall, including after rebuild; receipts equal rendered ids; golden prefix unchanged; private_never_in_prompt green | landed; tests::recall 8/8 |
| L1 night two | c3f74b9 | two scripted nights: V is a regular with one cited moment inside the fence, the second message gets none, deletion or tombstone stops it, strongest act wins; `sim run --nights N` twin | landed; tests::nights 3/3, moments 3/3, sim nights 4/4 |
| L2 audience arcs | 04f866a | three scripted nights covering vote, consequence and resolution; the opposite vote branches; restart is idempotent; a tie, an unknown poll or chat changes nothing; crisis keeps the payoff; newcomer recap | landed; tests::arcs 4/4 plus an orchestrator test. **No Malakor arc active**: `characters/malakor/arcs/DRAFT-council-object.json` awaits owner approval |
| merge and golden re-scope | 8436d79, d7f0fd2 | C1's golden re-baselined on C3's intended NOTE-instruction change (one line x 5 beats) | full gate green |

**Full gate on the merged branch (d7f0fd2):**
- `cargo fmt --check`: ok.
- `cargo clippy --workspace --all-targets -D warnings`: ok.
- `cargo test --workspace`: 1481 passed, 0 failed, 3 ignored.
- Contract: `check` in sync (128 files); `test` 46/46.
- `apps/console` typecheck: ok.
- `packages/stage-kit` test: 227/227.

**Judged checks not run** (they spend model quota; at most 3 targeted runs each under the tuning charter):
- C1: duty humour at least 40%, humor_landed at least 55%.
- L1: at least 60% of returning viewers recognised.
- S3: a card's preregistered target.

C1 changes `effects.toml`, so these runs carry a new instrument hash and are not comparable with
older runs.

## Found while building

- **Defect, fixed (d882e7b):** remembered viewer facts were rendered raw outside the memory nonce
  fence (compose.rs:996). Consolidated facts derive from chat, so this was an injection seam.
- **Flaky test, untouched:** `shadowrunr-engine` `fake_cli`
  (`a_chat_pick_gets_in_while_a_warm_core_holds_a_slot_and_judges_fill_the_batch_lane`, fake_cli.rs:691)
  races under load. Both lanes saw it fail once, and it passed in the merged full run. Sent to backlog.
- **Environment:** lane B's `pnpm install` in its worktree went through the junctioned `node_modules`
  and re-pointed 15 of the main checkout's top-level links into the worktree. The caller repointed all
  15 to main's own store before cleanup and verified with `realpathSync`.

## Deferred inside approved items

- L2: room.rs state-text parsing is not replaced, because typed votes would touch 42 SegmentContext
  literals; only the arc path is typed. Propose-outcome arc triggers are not built (votes only).
- L1: a CLI two-night scripted run writes no moments, because scripted lines never answer a pick.
  Brain-level tests carry the proof.
