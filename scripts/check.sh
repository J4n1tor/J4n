#!/usr/bin/env bash
# check.sh — the gate of every change: the whole workspace, never a subset (docs/TESTING.md).
# Stops at the first failing stage with that stage's exit code.
set -euo pipefail
cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.."
used=$(df --output=pcent / | tail -1 | tr -dc '0-9')
if [ "${used:-0}" -ge 90 ]; then
  echo "check: / is ${used}% full — clear target/debug/incremental first" >&2
fi

# stage <name> <cmd…> — a header, then the command; its failure ends the gate with its exit code.
stage() { echo "== $1"; shift; "$@" || exit $?; }

# nothing private in the files the public repository takes (release/public.paths): no address, home folder, key or private term
stage "leakscan" python3 -I scripts/release/leakscan.py
stage "selftest-t" scripts/selftest-t
# the kit's library: standard library only, no network, no file written (-B: no bytecode beside the source); with no python3 the tests are skipped, as the kernel's are
if command -v python3 >/dev/null 2>&1; then
  for f in packs/*/tests/test_*.py; do [ -e "$f" ] && stage "python $f" python3 -I -B "$f"; done
else
  echo "check: python3 is not on the PATH, so the kit's Python tests are skipped" >&2
fi
# the pty cases run once, in their own stage below, not here as well
stage "tests" scripts/t test --workspace --locked --no-fail-fast -- --skip tty_cases
# the real binary on a pty, with the scripted doer: the screen reads keys, answers, jumps, resizes and gives the terminal back
stage "tty" scripts/t test -p j4n --test laws tty_cases
# the speed of the screen against its ratchet (fixtures/tui-bench.toml): a release build, so the slowest stage to start cold
stage "speed" scripts/t bench -p j4n --bench tui_speed

echo "== pack tests"
shopt -s nullglob
# a pack test file that a Rust test of the workspace reads runs in the "tests" stage above (it names its runner in its first lines)
covered=(packs/base/tests/model_words.toml packs/base/tests/named.toml packs/offload/tests/offload.toml)
pack_tests=()
for f in packs/*/tests/*.toml; do
  [[ " ${covered[*]} " == *" $f "* ]] || pack_tests+=("$f")
done
if [ "${#pack_tests[@]}" -gt 0 ]; then
  echo "check: ${#pack_tests[@]} pack test file(s) exist but no pack-test runner exists yet, so nothing runs them:" >&2
  printf '  %s\n' "${pack_tests[@]}" >&2
  exit 1
fi
echo "pack tests: ${#covered[@]} file(s) run by the tests stage, no others"

stage "clippy" scripts/t clippy --workspace --all-targets --locked -- -D warnings

echo "check: green"
