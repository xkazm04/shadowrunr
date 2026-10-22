# Scout brief - scan-sweep --challenge, run challenge-2026-10-21

You are a READ-ONLY scout. You change no code and commit nothing. The only file you write
is your card file. Another session works in this repo in parallel (git worktrees under
`.claude/worktrees/`) - ignore it, never touch it.

Repo: C:\Users\kazda\kiro\overlord (Shadowrunr OS: a Rust daemon/brain/engine workspace under
`crates/`, a pnpm monorepo with `apps/console`, `apps/stage`, `packages/{contract,tokens,stage-kit}`,
Python tools under `tools/`). `context-map.json` lists every context's `file_paths`.

## Your job

Read your context's files IN FULL (all of them; riders too, if you have any), the governing
registry subject(s), and anything outside the context the hypothesis needs. Then return
**exactly two cards**:

- **Slot A - `architecture-challenger`**: the structural move this context needs to stop
  being the reason something else is hard - the seam that caps it, the duplicated rule that
  should be one module, the synchronous path that should be a job, the hand-list that should
  be derived, the state machine hiding in booleans, the contract two sides re-implement.
- **Slot B - `ux-elevation`**: the experience leap, not polish - a flow that takes five steps
  and should take one, a decision the user (operator, viewer) makes blind although the app has
  the data, a surface that reports state it should let you act on, a missing mode (compare,
  bulk, undo, preview). If the context has NO user surface (library, crate, sidecar), slot B is
  a second architecture card on a DIFFERENT seam, `"slot":"B-architecture (no UI surface)"`.
  An operator CLI's output or a daemon RPC the console renders counts as a surface only if
  the card's change lands inside this context.

Consider many ideas, return the best two, and name each slot's runner-up in one line.

## Floors - a card under any floor is not a challenge card

| field | floor |
| --- | --- |
| size | M or L (never S, never XL) |
| effort | >= 5 (1-10) |
| impact | >= 7 (1-10) |
| risk | 4-8 |
| write_set | declared, <= ~15 files, <= ~800 changed lines, ONE builder can land it |
| acceptance | 3-8 concrete cases `input -> expected`, each writable as a failing test BEFORE the build |

Gates that stay out of this mode (never pick these): `irreversible` (deletes user data,
rewrites stored history, migration without rollback), `policy-loosen` (weakens a safety /
moderation / privacy / audit check, widens access, sends or stores MORE), a contract consumed
by another repository, a direction outside the product's declared scope. `architecture` and
in-scope `direction` ARE allowed - the run's approval covers them; name them on the card.

Prefer the write set INSIDE your context's paths, plus tests and the coupled files the change
cannot land without (name them). A card that must edit a shared contract
(`packages/contract`, `crates/shadowrunr-types`) is allowed but say so in `shared_surfaces`.

Every claim cites `path:line` with the path relative to the repo root, exactly as
`git ls-files` prints it. A premise you did not verify by reading is not a premise.
Unused/dead claims need the zero-consumer grep in `evidence`.

## Never re-propose

These titles are already decided or filed - a card that restates one is void:
- "Warm-session deaths and failed opens leave no record in the transcript"
- "A package-driven brain hard-codes Malakor in six production sites" (filed as a direction;
  you may NOT adopt it)
- everything in `.claude/scan-sweep/findings/brain-composition-memory.jsonl` (built or filed)
- any card already written by another scout under `.claude/scan-sweep/challenge-2026-10-21/cards/`

## Registry

`.ai/registry-map.json` row for your context lists the governing subjects. Resolve a subject's
golden path via `../ai-registry/knowledge/<domain>/index.json` -> `subjects["<slug>"].file`
(never build a path from a slug). Read the golden path and the techniques whose `use_when`
matches. Name the technique a card leans on in its Description.

## The card - write it BEFORE you reply

Write a JSON array of your two cards to
`.claude/scan-sweep/challenge-2026-10-21/cards/<context>.json`. Each card:

```json
{"context":"<ctx>","slot":"A|B|B-architecture (no UI surface)","lens":"architecture-challenger|ux-elevation",
 "title":"<= 80 chars","size":"M|L","effort":7,"impact":8,"risk":6,
 "write_set":["<repo-relative path>"],"new_files":["<path>"],"shared_surfaces":[],
 "acceptance":["<case: input -> expected>"],
 "premise":["<path:line - fact>"],
 "rollback":"<how the commit series reverts cleanly>",
 "gate":"none|contract|policy-tighten|architecture|direction",
 "test_command":"<the exact command(s) that run the acceptance tests and the touched surface's gates>",
 "runner_up":"<the slot's second-best idea, one line>",
 "body":"## Summary\n...\n\n## Description\n...\n\n## Flow\n- ...\n\n## Expected impact\n...\n\n## Evaluation\nClaim: ...\nBefore: 0 of N acceptance cases pass (...)\nAfter: N of N\nMethod: gate - acceptance tests written first\nResult: better\nGate: ...",
 "evidence":"<the exact lines / grep output that prove the premise>"}
```

If your prompt names riders, either card may target a rider, but there are still exactly two
cards. Record each rider in `.claude/scan-sweep/challenge-2026-10-21/cards/<context>.riders.json`:
`[{"rider":"<ctx>","checked":"<files read, hypothesis traced, why no card went there or which card targets it>"}]`.

Gates in this repo (for `test_command`): Rust `cargo test -p <crate>`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo fmt --all -- --check`; TS `pnpm -C <pkg> typecheck|test|lint`,
`pnpm -C apps/<app> build`; Python `python -m unittest discover -t . -s tests` from `tools/voice`
or `tools/rig`, `python -m unittest test_depth_map test_learned_depth` from `tools/avatar`.

## Reply

After the file is written, reply with: the two titles, slot, size/effort/impact/risk, gate,
write-set size, and the two runner-ups. Under 200 words. No reasoning dump.
