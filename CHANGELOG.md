# Changelog

Notable changes, in the form of [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before 1.0 a minor version may
change anything, including the pack format (`api = 1`) and the commands.

## [Unreleased]

### Changed

- License: MIT OR Apache-2.0 (was MIT).
- README and docs rewritten as technical reference. `docs/ABOUT.md` and `docs/FEATURES.md` are gone; their content is in
  the README and in `docs/reference/`, which is generated from the pack rows.

### Added

- `AGENTS.md`: a map of the code for coding agents.
- `docs/reference/`: every setting, command and pack, kept equal to the pack rows by a test.

## [0.1.0] - 2026-11-16

First numbered version. `j4n --version` prints the number, the commit and its date.

### Added

- The gate: one rule set for writes, commands and network use, approved in one place; agents never see J4n's keys.
- Cost and result per task: money, or the share of a subscription's limit window; whether the project's checks passed.
- Connectors for Claude Code, Codex, Gemini CLI, OpenCode and Pi, and `/connect` for other CLIs.
- Grow: pack proposals that apply only when accepted.

### Experimental

Built and tested, not measured on enough real runs:

- the routes that pick a cheaper doer (plan, offload, kit);
- the connect lab's learning of a new CLI from its help text;
- the guard's reading of a risky command (a net, not a proof);
- packs J4n writes for the operator;
- `lanes = orchestrate`: Main as a chat with its own session, splitting work into chats. Off by default (`single`);
  `orchestrate.look`, `crew.on` and `crew.envoys` are `auto` and follow it.
