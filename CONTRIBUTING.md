# Contributing to J4n

Thank you for looking at J4n. This page is how to build it, test it and send a change that passes. Contributions are taken
dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option, with no additional terms.

For a bug, open an issue with the template. For a vulnerability, follow [SECURITY.md](SECURITY.md) instead. For a larger
change, open an issue first and say what you want to do, so the work is not wasted.

## Packs first, Rust last

J4n is a small core of eight parts (Table, Log, Task, Doer, Ask, Gate, Rule, View) and a large set of packs: folders of TOML
that add settings, pages, commands, gates, flows, connectors and the words the screen uses. A new feature is a pack. The core
grows only when no existing part can say it, and then by one field or one step kind that two features need.

So, before you write Rust, ask in this order:

1. Can a pack file say it? ([docs/PACKS-GUIDE.md](docs/PACKS-GUIDE.md))
2. Can an existing part of the screen show it? (`crates/j4n-tui/src/parts`)
3. Only then: a new part, with a test of its own.

The screen only draws what the core sends; it never reads files or calls the core directly. A law checks this.

## Build and test

You need the Rust toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (`rustup` installs it). `python3` is optional (the gate skips
the kit's Python tests without it), and a box around a task needs `bwrap`.

Run cargo **through the two scripts**, not bare:

| Command | What it does |
|---|---|
| `scripts/t <cargo args>` | one cargo command in a hermetic run: an empty `HOME`, a `PATH` without any agent CLI, a dead proxy, a 5 GB memory cap (when a systemd user manager exists). A test that writes into `HOME` fails the run. |
| `scripts/check.sh` | the gate: the runner's self-test, every workspace test and law, the screen on a pty, the speed ratchet, the pack tests and `clippy -D warnings`. |

```bash
scripts/t check -p j4n-core --tests                         # type-check only: fast, use it while editing
scripts/t test -p j4n --test laws nine_tabs                 # one test target, filtered by name
scripts/t test --workspace --locked                         # everything, without the pty and speed stages
scripts/check.sh                                            # the full gate
```

Two facts about `scripts/t`:

- It runs cargo under `env -i`, so a `RUSTFLAGS` you export is **dropped without a word**. Pass flags with
  `--config 'build.rustflags=["-Wunused"]'` instead.
- It keeps its run directories under `~/.cache` (set `J4N_T_ROOT` to move them), and removes them when it ends.

`scripts/b` is an optional wrapper with a memory cap; it passes on `https_proxy` when you set one. Use `scripts/t`.

### Work in batches

Builds are slow. Make all the edits of one piece of work, then build once.

- After a change other code builds on (a type, a trait, a signature, a macro, a manifest, a new file), run
  `scripts/t check -p <crate> --tests` once.
- When the piece is done, run the focused tests it touches.
- Run `scripts/check.sh` once per branch, when everything is committed. If it is red, fix and re-run the failed target
  first, then the gate again.
- A documentation-only change needs no build, only `scripts/t test -p j4n --test laws`.

### Golden screens

The screen is tested against recorded text screens under `crates/j4n-tui/tests/golden`. If a change moves a screen on
purpose, write the new ones with `J4N_BLESS=1 scripts/t test …`, then read `git diff`: only the words you meant to change
may differ. A golden you did not mean to move is a bug, not something to bless.

## The laws

[docs/CHARTER.md](docs/CHARTER.md) lists the rules J4n keeps, and each has a test in `crates/j4n/tests/laws`. The ones you
will meet first:

- **Size.** Each crate has a ceiling of source lines in `fixtures/size-budget.toml`. A change that adds lines raises the
  ceiling of its crate in the same commit and says why in the commit message. Deleting code needs no entry.
- **Layout and the one-writer rules.** The Log has one writer; every network client is built by the one builder that honours
  the proxy variables; only the Log opens a file for append.
- **No domain in core.** The core names no program, language or field of work; that belongs in a pack.
- **Clean agent text.** A template a model reads has no budget words and no claims about what it cannot do.
- **The nine tabs.** The screen has at most nine tabs; a page's id never changes, only its title, group and order.

A law failure prints what it expects. Read it before you change the law.

## Words the user reads

Anything on the screen, in a doctor line or in a refusal is professional, short and plain: the shortest true word, no
internal name, no id, no path to a development file. A refusal says what to press or where to go, never "run this in a
shell" or "edit that file by hand". Every command is reachable from the palette or by plain words.

## Comments

A comment is for a reader on GitHub who has never seen the project's history. It says one of:

1. **Why** the code is the way it is, when the code alone would look like a mistake;
2. **An invariant or a contract** that must hold before or after;
3. **Why it is safe**: each `unsafe` has a `// SAFETY:` line, and an `unwrap` that can be reached says why it cannot fail;
4. **What it is**: one line per module (`//!`), one per public item whose name does not say.

It does not say who found a bug, which ticket asked for the code, what the code used to be, or what the next line does. Code
that is commented out is deleted.

## A change that is ready

- the focused tests pass, and `scripts/check.sh` is green on the committed tree;
- a bug fix comes with a test that fails without the fix;
- the commit message says what changed and why, in the first line and a short paragraph;
- the pull request template is filled in.

## No personal data in the repository

This applies to people and to coding agents. Never commit any of these:

- e-mail addresses, names, hostnames or IP addresses of real people or machines;
- real home folders (`/home/<you>/…`). Tests use the placeholders `/home/user`, `/home/me` or `/home/ana`;
- keys, tokens, passwords, or a recorded session that holds one;
- notes, prompts or transcripts from your own work.

Test secrets are made up, and only in the files listed in `release/fake-secrets.txt`. Example addresses use
`example.com` or a `.test` domain. Example IPs use the documentation ranges (`192.0.2.0/24`, `198.51.100.0/24`,
`203.0.113.0/24`) or private ones.

`scripts/check.sh` runs `scripts/release/leakscan.py` and fails on a hit. To scan staged files before each commit:

```bash
scripts/release/install-hooks.sh
```
