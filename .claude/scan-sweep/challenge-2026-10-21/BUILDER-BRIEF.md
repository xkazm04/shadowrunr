# Builder brief - scan-sweep --challenge, run challenge-2026-10-21

You build ONE approved challenge card in a SHARED checkout: C:\Users\kazda\kiro\overlord, branch
`main`. Commit on `main`, NEVER push, never branch, never rebase, never stash, never reset,
never `git add -A/./-u`, never `git checkout -- <file>` on a file you did not change.
Other builders work in the same checkout at the same time on disjoint files, and an unrelated
session works in `.claude/worktrees/` - never touch it.

Your prompt names the card file and the card (context + slot). Read the card verbatim: its
`write_set`, `acceptance`, `premise`, `test_command`, and any `revision` the coordinator applied.

## Order - do not reorder

1. **Re-verify the premise** on the current tree. False -> write the return file with
   `status: "demoted"` and why; no code.
2. **Write the acceptance cases as tests FIRST** and run them: they must FAIL (red) on today's
   tree. Record how many were red. A case that pins behaviour the change must NOT alter is a
   GUARD case: declare it as such in your return before building; it is green before by design.
   Commit the tests only if they compile and the failure is the assertion (for Rust, a test that
   fails to compile because the API does not exist yet is fine to keep uncommitted until step 4 -
   then commit tests and implementation as `test(...)` then `feat|refactor(...)`, or as one commit
   if the tests cannot compile without the implementation; say which).
3. **Build** inside the write set. Coupled files the change cannot land without (a test, a doc,
   a mod declaration, an export) are allowed: name each in the return. Growing past the write set
   by more than that, or past size L, or failing a gate twice -> revert YOUR uncommitted work only
   (`git checkout -- <your paths>`, delete your new files) and return `demoted` with why.
4. **Gates, each in its own invocation, exit code asserted** - never `| tail`, never `;` before a
   commit. `cmd > log 2>&1; echo "rc=$?"` then read the rc; only commit when every rc is 0.
   - the card's `test_command`
   - Rust touched: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
     `cargo test -p <every crate you touched>` AND `cargo test --workspace` before the last commit
   - TS touched (per package touched): `pnpm -C <pkg> typecheck`, `pnpm -C <pkg> test`,
     `pnpm -C <pkg> lint` (contract, stage-kit, console have lint), `pnpm -C apps/<app> build`
     for apps; `pnpm -C packages/contract check` / `packages/tokens check` if you touched them.
     If you touch `packages/contract` or `packages/stage-kit` or `packages/tokens`, ALSO run
     typecheck+test for every app that imports it (apps/console, apps/stage).
   - Python touched: `python -m unittest discover -t . -s tests` from `tools/voice` or `tools/rig`;
     `python -m unittest test_depth_map test_learned_depth` from `tools/avatar`.
   - Before the last commit: the FULL suite of every package/crate you touched, not only your
     folder - this repo keeps guard tests (source guards, budgets, fidelity checks in
     `apps/console/src/quality`-like folders) away from what they guard.
   A whole-tree gate red on a path you did NOT touch is a sibling builder's in-flight work: wait
   ~60s, re-run once; if still red only there, note it in the return and proceed on the evidence
   scoped to your files. Never "fix" a file outside your write set.
5. **Commit** with pathspec only: `git add <new files>` then
   `git commit -m "<msg>" -- <exact paths>`. Message: `test(<ctx>): ...` / `feat(<ctx>): ...` /
   `refactor(<ctx>): ...`, body names the card title, `Lens: <lens> (challenge-2026-10-21)`, the
   acceptance figure `N of N cases green (M red before)`, and ends with:
   `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
   Pre-commit output about "consult-check" is informational, not a failure.
   Before each commit `git diff --cached --name-only` must list only your files.

## Console bundle budgets (updated after wave 2)

- `pnpm -C apps/console build` is GREEN at HEAD (door budget 540000 B). The repo's own rule
  (`apps/console/budgets/bundle.json` `$comment`): "A page builder who needs more raises its own
  line in the same change and says why." So if YOUR change pushes a route over, raise only that
  route's line, to the measured figure rounded up modestly, and append a measured sentence to the
  `$comment` in the same style (route, old -> new, measured bytes, what added them). Report it in
  your return. Net-deleting bytes is better than raising; never raise a line your change did not
  push over. `apps/console/budgets/bundle.json` is a shared surface: take the lock to edit it.
- An unrelated session also merges into main (e.g. 9ca1dc2 screens/hexmap). Its commits are not
  yours; re-verify your premise at HEAD.
- `tools/voice` `tests.test_pool.Pool.test_the_pool_never_runs_more_than_its_workers` is
  timing-flaky under machine load (passed, then failed 1 != 2 at the same sha). If it is the only
  red, re-run once; report it, do not edit it unless your card owns the pool.

## Generated / shared files

- `context-map.json`, `.ai/*`, `.claude/scan-history/*`, `.personas/*`: coordinator-owned. NEVER edit.
- `pnpm-lock.yaml`, `Cargo.lock`: do not add dependencies. If the card truly needs one, return demoted.
- Shared surfaces (`packages/contract/**`, `crates/shadowrunr-types/**`, `packages/tokens/**`, any
  generated file such as a schema/fixture produced by a script): only if the card's write_set
  names them. **Every commit must leave committed `main` compiling** (`cargo check --workspace`
  and the TS typecheck on the committed tree): a shared type change that needs its initializers
  or callers updated goes in ONE commit together with them - never commit a new required field
  alone (measured: 66040ff left main red for another session). Take the lock first: `mkdir C:/Users/kazda/kiro/overlord/.git/scan-sweep-challenge.lock`
  (retry every 10s until it succeeds; a lock older than 10 minutes is stale), edit, commit
  IMMEDIATELY and alone, `rmdir` the lock. If a file is generated (check for a generator script or
  a header saying so), regenerate it with its generator, never hand-edit.
- Never point caches into the repo. Before your last commit `git status --porcelain` must show no
  litter of yours.

## Return - write BEFORE replying

Write `.claude/scan-sweep/challenge-2026-10-21/returns/<ctx>-<slot>.json`:

```json
{"card":"<title>","context":"<ctx>","slot":"A|B","status":"landed|demoted|partial",
 "shas":["..."],"tests_added":["path::name"],"guard_cases":["..."],
 "cases_total":5,"cases_red_before":5,"cases_green_after":5,
 "gates":{"<command>":0},"files_changed":7,"lines_changed":420,
 "deviations_from_card":"...","notes":"..."}
```

Then reply in under 120 words: status, shas, cases red/green, gates, anything the coordinator
must know (e.g. a red gate that is not yours).
