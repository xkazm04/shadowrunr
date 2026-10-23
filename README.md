# Shadowrunr OS

An open-source runtime for AI characters that stream: a character with long-lived memory and
a fast conversational thread, background subagents for research and computation, table-driven
show segments, and a text simulation engine to test a character against simulated chat
audiences before it ever goes live.

Its flagship character is **Malakor** (*Overlord*): an original, theatrical skull-faced
sorcerer-villain who streams his plans for world domination, treats his viewers as minions,
and serves a mysterious Lord of the Darkness.

> Status: v0.1, the media-less phase. The engine, brain, segments and simulator come first;
> voice, avatar and broadcast come after the concept is proven in simulation.

## Layout

```
crates/shadowrunr-types     wire contract (see docs/spec/contract.md)
crates/shadowrunr-engine    universal LLM wrapper: Claude Code CLI fleet + fallbacks
crates/shadowrunr-brain     character mind: memory, prompt composer, worker pool, chat picker
crates/shadowrunr-segments  segments, commands, rank ladder, votes, scoring
crates/shadowrunr-sim       simulation engine and tournaments
crates/shadowrunr-daemon    the `shadowrunr` binary (CLI + daemon)
characters/                 character packages (Malakor first)
docs/concepts               open items and the owner's review loop
docs/research               feasibility research
docs/spec                   contracts
```

## Build

```
cargo build --workspace
cargo test --workspace
```

## Development

Every gate exits non-zero on a violation. CI (`.github/workflows/ci.yml`) runs all of them
on each push to `main` and on every pull request, with no secrets:

- Rust: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`. Tests use the scripted engine and a fake CLI binary, so they never
  spend quota.
- Drift: `pnpm -C packages/tokens check` (generated token CSS) and
  `pnpm -C packages/contract check` (TypeScript types and validators against the Rust schema).
- Per package (`packages/*`, `apps/stage`, `apps/console`): `pnpm -C <pkg> typecheck`,
  `pnpm -C <pkg> test`, and `pnpm -C <pkg> lint` where the package has it.
- Python: `python -m unittest discover -t . -s tests` in `tools/voice` and `tools/rig`, and
  `python -m unittest test_depth_map test_learned_depth` in `tools/avatar` (numpy and pillow;
  no model or engine binary).
- Lanes: `node scripts/ci-lanes.mjs --check` derives the lane set from `pnpm-workspace.yaml`,
  each package's gate scripts (`check`, `typecheck`, `test`, `lint`, `build`) and the `tools/*`
  Python suites, and fails when a lane is missing from `ci.yml`, a step names something that no
  longer exists, or a pnpm/python run line is in a form it cannot read. It prints its
  denominator (today 21 lanes: 18 package scripts and 3 Python suites; the Rust job is not
  counted). Adding a gate script to a package means adding its CI step.
- Console: `lint` also runs stylelint (tokens only, no raw colour or px), the 400-line file
  cap and the token-reference check. `test` holds the DOM-count budgets
  (`apps/console/budgets/dom.json`), and `build` ends with the per-route bundle budget
  (`apps/console/budgets/bundle.json`).

The console's visual fidelity gate is local-only. It needs the contest arena, which is not in
this repository, so CI never runs it.

## Licence

Apache-2.0. "Shadowrunr", "Overlord" and "Malakor" are project names, not licensed marks.
