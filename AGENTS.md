# J4n: a map for coding agents

This file is for any agent working on J4n's code, including J4n itself. It names where each thing lives and how to
change it. The design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The rules the code keeps are in
[docs/CHARTER.md](docs/CHARTER.md). Every setting, command and pack is in [docs/reference/](docs/reference/).

## Rules that are checked

| Rule | Checked by |
|---|---|
| No personal data in a tracked file, comment, test, fixture or commit: no e-mails, names, hosts, real IPs, home folders, keys, tokens, transcripts, prompts. Use `/home/user`, `example.com`, `203.0.113.7`. Made-up keys go only in the files of `release/fake-secrets.txt`. | `scripts/release/leakscan.py` (first stage of `scripts/check.sh`) |
| Comments say *why*, in product terms. No decision or bug ids, no dates, no "asked by". A comment that names a `.md` file names one that exists. | `crates/j4n/tests/laws/tidy.rs` |
| One cut word, read from the private record, may appear in no file and no file name. | `crates/j4n/tests/laws/cut_word.rs` |
| The crate list and each crate's size. | `crates/j4n/tests/laws/layout.rs`, `size.rs` |
| `docs/reference/*.md` equals what the pack rows say. | `crates/j4n/tests/laws/reference_docs.rs` (`J4N_BLESS=1` rewrites) |
| Every network client is built in `crates/j4n-connect/src/net.rs`. | `crates/j4n/tests/laws/proxy.rs` |
| `j4n-tui` depends only on `j4n-proto` and UI crates. | `crates/j4n/tests/laws/tui_boundary.rs` |

## Build and test

| Command | Use |
|---|---|
| `scripts/t check -p <crate> --tests` | type-check after a change to a type, a signature, `Cargo.toml` or a new file |
| `scripts/t test -p <crate> --test <target> <filter>` | the tests a change touches |
| `J4N_BLESS=1 scripts/t test …` | rewrite goldens and generated files after an intended change |
| `scripts/check.sh` | the full gate (leak scan, every test, the screen's speed ratchet); once per branch |

`scripts/t` runs cargo with an empty `HOME`, no agent CLIs on `PATH`, a dead proxy and a memory cap. A test that writes into
`HOME` fails. Make all the edits of one change before building; builds are slow. More: [docs/TESTING.md](docs/TESTING.md).

## Where things live

| Crate | Responsibility | Start reading at |
|---|---|---|
| `j4n` | the binary: command line, wiring, built-in pack list (`ON` = packs on by default) | `src/main.rs`, `src/cli.rs`, `src/wire.rs`, `src/pack.rs` |
| `j4n-core` | decisions: the Table, Log, tasks, asks, gate, rules, views, the pack loader | `src/lib.rs` |
| `j4n-core/src/task` | the task loop and engine: states, approvals, waiting, resuming | `engine.rs`, `asks.rs` |
| `j4n-core/src/gate` | the gate: shell-line reading, grants, the inside-the-project check | `check.rs`, `grant.rs`, `inside.rs` |
| `j4n-core/src/answer` | ask chains: who answers a question, breakers, the per-task cap | `chain.rs`, `state.rs`, `point.rs` |
| `j4n-core/src/front` | what the screen shows: chats, commands, completion, weigh/read routing, tiers, settings pages | `mod.rs`, `command.rs`, `serve.rs` |
| `j4n-core/src/brief.rs` | what a model is told (the Brief and its sections) | |
| `j4n-core/src/setting.rs` | the `[[setting]]` row and its type | |
| `j4n-connect` | the outside: processes, places (worktree, box), dialects, the session pool, built-in tools, project checks | `src/proc.rs`, `src/check.rs`, `src/claude/`, `src/pi/` |
| `j4n-reach` | web fetch and search, MCP | `src/web/`, `src/mcp/` |
| `j4n-memory` | the Log index (SQLite FTS) and the project card | `src/card.rs` |
| `j4n-proto` | serde types between core and screen (`Down`, `Up`, `Node`) | `src/lib.rs` |
| `j4n-tui` | the terminal screen | `src/app.rs`, `src/term.rs`, `src/headless.rs` |
| `j4n-shape`, `j4n-qc`, `j4n-bench`, `j4n-contribute`, `j4n-inbox`, `j4n-habits`, `j4n-ledger`, `j4n-parts` | the Rust halves of the packs of the same name | |
| `j4n-lab`, `j4n-forge` | development tools: fake CLIs from a seed, and a scorer for connecting to them | |

| Folder | Holds |
|---|---|
| `packs/<id>/pack.toml` | a pack's manifest: id, summary, `use_when`, `needs`, stability |
| `packs/<id>/<kind>/*.toml` | a pack's rows of one kind: `settings`, `commands`, `rules`, `gates`, `flows`, `connectors`, `pages`, `views`, `asks`, `checks`, … |
| `packs/base/kinds/` | the TOML schema of every kind, checked against the Rust structs |
| `fixtures/` | test inputs and goldens |
| `scripts/` | `t`, `check.sh`, release scripts |
| `docs/reference/` | generated; never edit by hand |

## How to change a thing

Prefer a pack row over Rust. A change to Rust is right only when no row can say it.

| To change | Edit | Then |
|---|---|---|
| a setting (default, words, doc) | `packs/<id>/settings/*.toml` | bless `reference_docs` |
| a command a line can start with | `packs/<id>/commands/*.toml`, or `packs/base/phrases/commands.toml` | bless `reference_docs` |
| when a shell line is allowed, asked or blocked | `packs/<id>/gates/*.toml`; reading of lines is in `j4n-core/src/gate/` | `scripts/t test -p j4n --test laws guard` |
| what happens on an event (start a check, steer a task) | `packs/<id>/rules/*.toml` | the pack's flow test |
| who answers a question | `packs/base/asks/points.toml` (`chain`, `fallback`) | `answer_cases`, `ask_rows` |
| how a project's tests are found | `packs/base/checks/detect.toml` | `j4n-connect` check tests |
| a new agent CLI | a `[[connector]]` in `packs/connect/connectors/`; a new wire format is a dialect in `j4n-connect` | `connect_flow` |
| a screen page or view | `packs/<id>/pages/`, `packs/<id>/views/`; parts in `crates/j4n-tui/src/parts/` | the screen goldens |
| what a model is told | `j4n-core/src/brief.rs`, templates in `packs/*/templates/` | `clean_text` (no budget words in a model-bound text) |

## Running J4n on a project

J4n keeps per-project state in `<project>/.j4n/` (the Log, `log/0001.jsonl`, one JSON event per line) and per-project
settings in `~/.j4n/projects/<key>/*.toml`. To find out why something happened, read the Log: each event has a `kind`
(`task.started`, `ask.step`, `gate.verdict`, `decider`, …) and the task id it belongs to.
