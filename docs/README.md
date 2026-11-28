# Documentation

| Doc | For | Content |
|---|---|---|
| [../README.md](../README.md) | everyone | what J4n is, install, use, status |
| [INSTALL.md](INSTALL.md) | users | install per platform, optional programs, update, uninstall |
| [GUIDE.md](GUIDE.md) | users | the screen, chats, approvals, cost, packs, orchestrate |
| [GATE.md](GATE.md) | users, reviewers | the gate's rules, keys and secrets, boxes, known attacks, limits |
| [PACKS-GUIDE.md](PACKS-GUIDE.md) | pack authors | pack layout, kinds, flows, scripts, `j4n pack check` |
| [ARCHITECTURE.md](ARCHITECTURE.md) | contributors | the eight parts, crates, protocol, life of a line, asks, settings, the Log |
| [CHARTER.md](CHARTER.md) | contributors | the laws the code keeps, each with its test |
| [TESTING.md](TESTING.md) | contributors | `scripts/t`, the gate, goldens, the size and speed ratchets |
| [../AGENTS.md](../AGENTS.md) | coding agents | a map of the code and how to change each thing |
| [reference/settings.md](reference/settings.md) | everyone | every setting, generated from the pack rows |
| [reference/commands.md](reference/commands.md) | everyone | every command a line can start with, generated |
| [reference/packs.md](reference/packs.md) | everyone | every built-in pack, its default and what it adds, generated |
| [BRAND.md](BRAND.md) | maintainers | the mark, wordmark, colours, rebuilding the art |
| [THANKS.md](THANKS.md) | everyone | the projects J4n is built with and the agents it works with |
| [THIRD-PARTY.md](THIRD-PARTY.md) | packagers | the crates in the binaries and their licenses (generated) |

The files under `reference/` are made by `crates/j4n/tests/laws/reference_docs.rs`; `J4N_BLESS=1` writes them again.
Never edit them by hand.

These docs are AI-generated under the maintainer's supervision and review.
