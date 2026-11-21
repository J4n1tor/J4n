# Features

J4n is a core and a set of packs. The core and six packs are on when J4n starts. The others are built in and off until
you enable them under **Settings › Packs** (or with `:pack on ID`). The setting `packs.full` turns them all on.

*Experimental* means built and tested, but not yet shown to help on real tasks.

## Core

| Feature | What it does |
|---|---|
| Gate | Every write, command and network use is checked by one rule set before it runs: allowed, asked on an approval card, or blocked. Writes outside the project ask first; J4n's own folder and git internals are blocked. See [GATE.md](GATE.md). |
| Key guard | Keys J4n holds are never passed to an agent. Secrets in what the log stores and what a model reads are replaced by a placeholder that keeps their shape. |
| Checkpoints | Work happens behind a checkpoint; `:undo` and rewind take it back. |
| Cost | Each task shows its cost: money for API use, the share of the limit window for subscription plans. |
| Checks | After edits, the project's own checks run (its test command, detected per language). |
| Log | Every task's events in one append-only log per project (`.j4n/log/`). |
| Chats | Each prompt can be its own chat, with its own agent session. |
| Three ways to run | The terminal UI (`j4n`), one task (`j4n run "…"`), or one line per turn on stdin. |
| Marks | The icons on the screen follow the machine: Nerd Font icons where the terminal draws them itself or such a font is installed, plain Unicode where neither is found; ASCII and words on request (`theme.marks`). |

## Packs on by default

| Pack | What it does |
|---|---|
| `base` | The rule schemas, default gates, screens, commands and phrase tables. |
| `guard` | Shape-keeping redaction. Secret files are shown to a sandbox as a redacted view. MCP tools are held to their pinned definitions. |
| `grow` | Observers and tips. J4n drafts a pack change from a description; nothing changes until you `:accept`, and `:undo` reverts it. |
| `connect` | Connectors for Claude Code, Codex, Gemini CLI, OpenCode and Pi. `/connect <program>` probes an unknown CLI, drafts a connector, runs it once and proposes it. |
| `lanes` | A main lane that keeps the plan and hands work to branches. It does nothing until `lanes = "orchestrate"` is set (experimental). |
| `crew` | Seats: the role, way of working and access of each chat the main lane starts. Used by orchestrate mode. |

## Packs off by default

| Pack | What it does | State |
|---|---|---|
| `accounts` | Several logins of one agent kept apart. Each account has its own folder, browser window and optional proxy. | experimental |
| `offload` | Light chores (reads, searches, test runs, digests) go to a cheaper agent by rule. The main lane stays the only writer. | experimental |
| `code` | Plan, work and validate flows with checks and worktrees, plus a review mode. | experimental |
| `kit` | A Python tool (`py`) for J4n's own agent, with one kernel per task. | experimental |
| `agent` | J4n's own agent on a model key of yours: react and work flows. | experimental |
| `jev` | Small decider models for the questions J4n would otherwise ask you. | experimental |
| `qc` | Quality marks on code from patterns: place, likelihood, severity. A mark never decides. | experimental |
| `shape` | Prompt shaping: while you type, notes with sources are added under the line. | experimental |
| `inbox` | Lines typed while a task runs get a tag and a receipt instead of steering it at once. | experimental |
| `habits` | Show-only detectors of waste: tiny steps, cold cache, rereads, big outputs. | experimental |
| `reflexes` | A command that runs by itself when a sentence of yours matches, judged by one decision model. | experimental |
| `ledger` | One report per finished thread, read from the log, with a receipt for every line. | experimental |
| `parts` | The parts of a document run one at a time as plan items, each checked and committed (never pushed). | experimental |
| `skills` | A skill library: search, read, switch on and off; the few that fit a task go into the agent's brief. | experimental |
| `terminal` | tmux panes as tools of a model: list, read, type, press keys, each asked about. | experimental |
| `bench` | Cases of J4n's own behaviour, written down and run. | for development |
| `contribute` | J4n records what goes wrong with J4n and keeps its own fixes. Nothing leaves the machine. | experimental |

## Places

A task runs in one of several places: in the project folder behind a checkpoint, in a git worktree, or in a `box`, a
bubblewrap sandbox that sees only the paths it is given (Linux only). See [GATE.md](GATE.md) §3.
