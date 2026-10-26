# Malakor tuning: the tuner's playbook

The `malakor-tuning` charter of Overlord's App Master. Every run on this charter reads this file first.

**Mission.** Make Malakor's stream entertaining and smooth. Find where the show sags, stalls or breaks in
simulated runs, then tune the behavior core until it doesn't. The balance to hold is between monologue
(bits, lead-ins, case beats), segments (SEG-01..03, votes, rounds) and reaction to viewers (questions,
raids, gifts, mentions). Remove every technical barricade in how LLM threads are orchestrated: admission
waits, dead air, late or doubled lines, a starved live lane.

Owner direction, 2026-10-25: there is already enough data to find many gaps. **Mine the existing runs
first. Re-run only the situations a fix targets, after the fix.** Tournaments come back later, at most
3 a day, once the owner sees enough adjustments pointing the same way. Each standup syncs this.

## Inputs, read-only

| What | Where |
| --- | --- |
| Tournament results | `runs/sim/<matrix>-<hash>/`: `results.json`, `report.md`, `heatmap.md` |
| One judged cell | `runs/sim/<matrix>-<hash>/<tone>.<tier>.<SEG>.<audience>.<seed>/`: `transcript.jsonl`, `verdict.json`, `judge.json`, `ledger.jsonl`, `instrument.json`, `spec.json` |
| Sandbox streams | `runs/sandbox/sbx-*/`, with the same files plus `room.json`, `session.json` and `tags.json` |
| The rubric and calibration | `sim/rubric.toml` (red lines at a 100% bar, quality dimensions), `sim/calibration.json` |
| Matrices | `sim/matrices/*.toml` |
| The last rebalance | `.spark/Spark/ideas/malakor-rebalance.md`, shipped at `7e4b6f8` |

On 2026-10-25 the latest check (`rebalance-check-34188c4a83`) had `humor_landed` at 66%, the weakest
dimension, while the others sat at 94-100%. Failed situations in the roastmaster tone included
`raid-wave`, `gifted-sub-hype`, `dread-council-proposal` and `seg02a-*`. One red line,
`harassment-real-people`, failed at audience 300. These are the first leads, not the whole list.

## The knobs (the behavior core)

- `characters/malakor/`:
  - `character.toml [tuning]`: `max_picks`, `carry_ms`, `hold_gap_ms`, `min_cover_ms`, `speech_cps`, `render_lag_ms`, `lookahead_max_ms`, `browser_dwell_ms`, `closing_now_s`. Each change records its provenance in the table.
  - `tones/<tone>.toml` (`budget_chars`, `max_sentences`) and `tones/<tone>.md` (plot/chat ratio prose).
  - `directions.md`, `effects.toml`, `situations.json`, `stalls.toml`, `segments/`, `foils/`.
- `crates/shadowrunr-brain/src/orchestrate/`:
  - `mod.rs:169-243`: off-topic, small-room, raid, gathering, quiet-bit and bit-spacing constants.
  - `agenda.rs:28-37`: pickup, lead-in, bridge and thread cooldowns.
  - `case.rs:54-60`: case beat, payoff and retry.
- `crates/shadowrunr-brain/src/`:
  - `pace.rs:47-51`: pace factors and latency samples.
  - `picker.rs:269`: scoring weights. The LLM picker takes over above 40 candidates.
- LLM thread orchestration:
  - `shadowrunr.toml [engine]`: `max_live_processes`, `live_reserved_processes`, `idle_timeout_ms`.
  - `[sim]`: `pool_size`, `concurrency`.
  - `[daemon]`: `max_live_runs`.
  - `crates/shadowrunr-engine/src/{admission,limits,ladder}.rs`.

A knob is changed as data where it can be. A constant that needs tuning per tone or per audience is a
candidate to move into data, and the move is its own commit.

## The loop: one gap per run

1. **Mine.** Read the newest tournament and sandbox runs that share the current instrument hash. Pick ONE
   gap with the most evidence. It might be a failed situation, a weak quality dimension, dead air, a doubled
   or late line, a reaction that never came, a segment that overran, or a live lane starved by
   background work. Cite the cell ids and transcript line numbers.
2. **Diagnose.** Name the mechanism: which knob or code path produced it, shown from the transcript and
   ledger timestamps. Say whether it is a content gap (prompts, tones, directions) or a timing or
   orchestration gap (constants, admission, concurrency).
3. **Fix.** Change the smallest knob or code path that explains it. Add a deterministic test where one
   can pin the behavior, using a `--scripted` run, a unit test on the orchestrator, or a picker vector.
4. **Re-run only what the fix targets.** Use `shadowrunr sim run` with the failing cell's tone, tier,
   segment, audience and seed, and add `--judge` only when the gap is about quality. Use at most 3 judged
   re-runs per fix. Pick ONE neighbour cell that never showed the gap and run it too, to prove the fix
   did not move it.
5. **Keep or revert.** Keep the fix only if the target improves, the neighbour does not regress, and no red
   line fails. A fix that worsens the measured outcome is reverted and written up, not tuned further.
6. **Record** one row in `docs/tuning/LEDGER.md`: date, gap, mechanism, knob or file, before and after with
   run ids, kept or reverted. Batches of rows are what the owner reads at standup, to decide when
   tournaments return. A kept row states each cited cell's bar count as **a/N**, with N parsed from
   `sim/rubric.toml` (every `[[red_lines]]` table), and `scripts/ledger-bar.mjs` enforces it.

## Hard rules

- **Never make the judge easier.** `sim/rubric.toml`, `sim/calibration.json` and the judge prompt in
  `crates/shadowrunr-sim/src/judge.rs` change only with an ask to the owner.
- **Never mix instruments.** Results are compared only within one instrument hash. Bumping
  `SIM_LOGIC_VERSION` (`crates/shadowrunr-sim/src/instrument.rs:40`) needs an ask, because it orphans every
  earlier result.
- **Red lines are a 100% bar.** A fix that trades a red line for humor is reverted.
- `characters/malakor/private/` is never loaded or quoted. Real chat never becomes a fixture.
- **Spend.**
  - No tournament (`sim tournament`) and no new matrix runs until the owner lifts this at a standup.
  - Targeted re-runs as in step 4 are allowed.
  - Any run beyond that is a spend ask.
- One gap per run and one atomic commit per fix, with the evidence run ids in the commit message.
