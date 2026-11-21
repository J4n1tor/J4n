# Testing J4n

What a contributor runs, and what each test layer proves. Everything here runs offline with no agent installed. The
commands are in [CONTRIBUTING.md](../CONTRIBUTING.md); this page says what stands behind them.

## 1. The hermetic runner: `scripts/t`

Every cargo command goes through it. A bare `cargo test` could start a real agent or write into your real home, and the
runner is what stops that. It runs one cargo command from the repository root with:

- an empty `HOME` and a private `TMPDIR` in a run directory under `~/.cache` (`J4N_T_ROOT` moves it), removed at the end;
- a `PATH` of the system tools with no agent CLI on it (`claude`, `codex`, `opencode`, `pi`, `gemini`, `omp` and the like);
- a private `CARGO_HOME` that mirrors the real one (registry, git, config) with an empty `bin/`, because `rustup` puts
  `$CARGO_HOME/bin` on the `PATH` of every tool it starts;
- a dead proxy (`127.0.0.1:9`, every spelling), two build jobs, no debug info, `TZ=UTC`;
- a memory cap of 5 GB (`systemd-run --user --scope`) when a systemd user manager exists, announced as `J4N_CAP=on|off`;
- `J4N_HERMETIC=1`. The laws binary fails without it, so a bare `cargo test` cannot pass them.

Writing into `HOME` **fails the run** (exit 1, `t: WROTE n entries into HOME`). A test that needs a home makes its own
temporary one. It runs cargo under `env -i`, so an exported `RUSTFLAGS` is dropped: use `--config 'build.rustflags=[…]'`.
`J4N_BLESS` and `J4N_SWEEP` are passed through when set.

`scripts/selftest-t` proves each property above on a fixture crate (`fixtures/hermetic/`) and runs first in the gate.

## 2. The gate: `scripts/check.sh`

The whole workspace, never a subset. It stops at the first failing stage:

1. `selftest-t`, the runner's own test;
2. the kit's Python tests (`packs/*/tests/test_*.py`, standard library only; skipped with a note when `python3` is absent);
3. every workspace test, including every law and the size ratchet (`scripts/t test --workspace --locked`);
4. the screen on a real pty with the scripted agent (`tty_cases`): keys, answers, jumps, resizes, and the terminal given back;
5. the speed ratchet of the screen (`scripts/t bench -p j4n --bench tui_speed`), a release build and so the slowest stage
   to start cold;
6. a check that every pack test file is run by some Rust test, so none sits unrun;
7. `clippy --workspace --all-targets -D warnings`.

A new test is shown to fail on the old code before the fix lands.

## 3. What the layers prove

| Layer | Where | What it proves |
|---|---|---|
| Unit and flow tests | `crates/<name>/tests` (one integration-test binary per crate) | one part behaves |
| Core scenarios | `crates/j4n-core/tests/core`, `crates/j4n-connect/tests/connect` | the core keeps its promises with no model and no network: a scripted agent and a scripted model are driven through the same entry the screen and `j4n run` use, and the test asserts on the disk and the Log, never on prose |
| Laws | `crates/j4n/tests/laws` | each rule of the [charter](CHARTER.md): the size ratchet, the layout of the crates, one network builder, no domain in core, clean agent text, nine tabs, and more. A law is a detector over text, a test that runs it on the repository, and tests that run it on bad input |
| Screen tests | `crates/j4n-tui/tests` | the screen draws what the protocol sends, headless, against recorded text screens (goldens) |
| Sweep | `crates/j4n/tests/sweep` | every page at several widths, palettes and icon tiers |
| Pty cases | `crates/j4n/tests/tty` | the real binary on a pseudo-terminal |
| Pack tests | `packs/*/tests` | a pack's own cases and goldens (`j4n pack check`) |
| Speed | `crates/j4n/benches/tui_speed.rs`, `fixtures/tui-bench.toml` | frame and key times do not rise; a ceiling only goes down |

Judge a task by the world's state (files, tests, the Log), never by what an agent said.

### Goldens

A screen test compares the screen with a recorded text file. When a change moves a screen on purpose,
`J4N_BLESS=1 scripts/t test …` writes the new files; read `git diff` and keep only the words you meant to change. The
sweep writes its pages only when `J4N_SWEEP=<dir>` is set (and `J4N_SWEEP_ONLY=<look>` runs one look); without it the
sweep does nothing.

### The size ratchet

`fixtures/size-budget.toml` holds, per crate, the most non-test, non-blank source lines. A ceiling rises only in a change
that says why in its commit message. Deleting code needs no entry.

## 4. Speed and waiting in a test

The gate runs while other builds share the machine, so what a test holds must not depend on how busy it is.

- **A speed is the CPU the test's thread spent** (`crates/j4n-core/tests/core/cpu.rs`: `of`, `least_of(n, …)`), never the
  time that passed: a thread waiting for a core is slower on the clock and no slower in the code. A short budget is held to
  the least of three goes. A guard against a quadratic compares two lengths far apart with a factor between linear and
  squared, and does not use a number.
- **A wait is for the event or the state that proves the thing** (a subscription made before the spawn, a row of the book,
  a line in the Log), with 20 s as the place where it gives up, never "sleep a while and look". An agent that must be caught
  mid-way is held there, not paused for a while.
- **A real clock is a real clock.** The process's `--wall` and the front's deadline count real time; a test that uses them
  gives them seconds, and runs the task's own end on the manual clock.

## 5. Live checks

A few tests run the same turn against the real installed agent. They run only with `J4N_LIVE=1`, **outside** the hermetic
runner (which keeps agents off the `PATH`), and they spend your subscription or key:

```bash
J4N_LIVE=1 cargo test -p j4n-connect --test connect live -- --nocapture
```

They are not part of the gate. The benchmark suites that compare J4n with other harnesses are not in this repository.
