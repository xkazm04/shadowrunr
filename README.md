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
- Console: `lint` also runs stylelint (tokens only, no raw colour or px), the 400-line file
  cap and the token-reference check. `test` holds the DOM-count budgets
  (`apps/console/budgets/dom.json`), and `build` ends with the per-route bundle budget
  (`apps/console/budgets/bundle.json`).

The console's visual fidelity gate is local-only. It needs the contest arena, which is not in
this repository, so CI never runs it.

## Licence

Apache-2.0. "Shadowrunr", "Overlord" and "Malakor" are project names, not licensed marks.
