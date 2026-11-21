# Changelog

All notable changes to J4n are written here, in the shape of [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The number moves
at a release point, not at each merge; before 1.0 a minor bump may change anything.

## [0.1.0] - 2026-11-16

The first numbered version. `j4n --version` prints the number, the commit and the commit's date: `0.1.0 (<commit> <date>)`.

### What exists

Four promises:

- **One gate.** Run any agent; its risky steps are approved in one place; no agent sees a key.
- **Know the cost and the result.** Each task shows what it cost (dollars, or a share of the plan window) and whether it was checked.
- **The cheapest route that works.** Code first, a small model next, a big model last.
- **Grow it yourself.** Say what you want; J4n writes a small pack file; nothing applies before the operator presses Tab.

### Experimental

Everything is experimental: nothing is called stable until the maintainer says so. In particular the following have been built but not measured
against enough runs to be trusted:

- the routes that pick a cheaper doer (the plan, the offload, the kit) and the savings they claim;
- the connect lab's learning of a new agent or tool from its help text (it does well on the shapes it was built for and worse on others);
- the guard's reading of a risky command, which is a net and not a proof;
- the packs that J4n writes for the operator, and the settings that switch features on;
- the `orchestrate` word of the setting `lanes` (Main as a chat with its own session, a quick look that decides whether a line is answered,
  worked in one lane or split into chats, and the briefs and limits of those chats). It is off by default (`single`); the word is the one
  switch: the quick look (`orchestrate.look`), the crew's seats (`crew.on`) and the envoys (`crew.envoys`) are `auto` and follow it, and a
  value set by hand still wins. The packs `lanes` and `crew` are in J4n basic and change nothing under `single`.

The pack format (`api = 1`) and the commands may change between minor versions.
