#!/bin/bash
# full gate; each step's own exit code recorded, never piped
cd /c/Users/kazda/kiro/overlord
out=${1:-/dev/stdout}
: > "$out"
step() { name=$1; shift; "$@" > "${out}.${name}.log" 2>&1; rc=$?; echo "$name=$rc" >> "$out"; }
step fmt cargo fmt --all -- --check
step clippy cargo clippy --workspace --all-targets -- -D warnings
step cargotest cargo test --workspace
for p in packages/tokens packages/contract; do n=$(basename $p); step $n-check pnpm -C $p check; done
for p in packages/tokens packages/contract packages/stage-kit apps/stage apps/console; do n=$(basename $p)
  step $n-typecheck pnpm -C $p typecheck; step $n-test pnpm -C $p test; done
for p in packages/contract packages/stage-kit apps/console apps/stage; do step $(basename $p)-lint pnpm -C $p lint; done
step stage-build pnpm -C apps/stage build
step console-build pnpm -C apps/console build
(cd tools/voice && step voice-py python -m unittest discover -t . -s tests)
(cd tools/rig && step rig-py python -m unittest discover -t . -s tests)
(cd tools/avatar && step avatar-py python -m unittest test_depth_map test_learned_depth)
echo DONE >> "$out"
