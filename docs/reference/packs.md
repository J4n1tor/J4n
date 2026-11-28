<!-- Made from the pack rows by crates/j4n/tests/laws/reference_docs.rs; do not edit. J4N_BLESS=1 writes it again. -->

# Packs

Everything J4n does beyond its core is a pack: TOML rows for settings, rules, gates, flows, connectors, pages and commands (see [PACKS-GUIDE](../PACKS-GUIDE.md)). *On* yes: on when nothing was set; the others are turned on with `:pack on <id>`.

| Pack | On | Stability | Needs | What it adds | What it is |
|---|---|---|---|---|---|
| `accounts` |  | experimental | base | commands, connectors, events, failures, flows, folds, kinds, pages, views | several logins of one harness kept apart: each account has a folder, a browser window and, if it needs one, a proxy of its own |
| `agent` |  | experimental | base | connectors, flows | the own agent: react and work flows, builtin tool connectors |
| `base` | yes | experimental |  | asks, boxes, checks, commands, connectors, failures, flows, folds, gates, hosts, keys, kinds, look, needs, pages, phrases, plans, probes, providers, rank, rules, settings, sources, templates, tests, views | kind schemas, core ask points, loop rules, default gates, views, pages, commands, phrase tables |
| `bench` |  | experimental | base | cases, commands, events, flows, folds, kinds, pages, views | the case kind, the runner flow, results views |
| `code` |  | experimental | base, agent | flows, phrases | the coding harness: plan, work and validate flows, checks, worktree defaults, review mode |
| `connect` | yes | experimental | base | asks, commands, connectors, events, flows, kinds, pages, phrases, rules, settings, sniffs, views | the Connect flow (`/connect`: look at a program, draft how to speak to it, try it once, propose it), the rules for how a program speaks, and samples (Codex, OpenCode, Gemini) |
| `contribute` |  | experimental | base | commands, events, folds, pages, rules, settings, views | every J4n finds what is wrong with J4n and keeps the fixes it makes; in this version nothing leaves the machine |
| `crew` | yes | experimental | base | commands, kinds, pages, seats, seeds, settings, views | seats: a role, a way of working and an access for each chat Main starts |
| `grow` | yes | experimental | base | folds, rules, settings | observers, tips and the author flow |
| `guard` | yes | experimental | base | gates, incidents, rules, settings | redaction that keeps a secret's shape (what the Log writes and what a model is shown has a key, a password or a token replaced by a placeholder that says what it was), secret files shown to a box as their view, MCP tools held to their pins at each call, and a run that has read untrusted content kept from sending or writing anything sensitive, and a row that names the known attack (s1ngularity, GitHub MCP, postmark-mcp, Supabase MCP, Amazon Q) when something looks like it; off, the older rules run |
| `habits` |  | experimental | base | commands, flows, folds, pages, rules, settings, views | show-only detectors of waste the tools make that J4n can see: tiny steps, a cold cache, rereads, big outputs, a chore typed again, a late compaction; off by default, so nothing is said that was not |
| `inbox` |  | experimental | base | commands, events, folds, kinds, pages, settings, views, words | lines typed while a task works get a tag and a receipt instead of steering it at once; off by default, so a line steers as it always did |
| `jev` |  | experimental | base | connectors, models | decider connectors (Jev, Span-01) and a small model for the ask points the base pack names; with no key they are not made |
| `kit` |  | experimental | base | commands, events, flows, gates, needs, pages, prompts, py, recipes, rules, settings, tests, views | the kit: code first, then a decider, then a model; the tool `py` and its Python kernel, one per task, behind the setting kit.mode (off by default, so nothing of it exists) |
| `lanes` | yes | experimental | base | kinds, rules | the main lane keeps the plan and hands work to branches of its own classes |
| `ledger` |  | experimental | base | commands, events, folds, needs, pages, rules, settings, views | one report per settled thread, read from the Log: the facts, a receipt for every line, the answers merged under their lines; off by default, so a thread ends as it always did |
| `offload` |  | experimental | base | chores, commands, kinds, needs, settings, tests | light chores (reads, searches, test runs, digests) go by rule to a cheaper doer and come back as rows; the main lane stays the only writer |
| `parts` |  | experimental | base | commands, events, gates, settings | the parts of a document you name run one at a time as plan items: each part's gate, then a commit of only what it changed, never pushed; off by default, so a plan runs as it always did |
| `qc` |  | experimental | base | commands, events, flows, folds, keeps, kinds, needs, pages, patterns, rubrics, rules, score, settings, views | quality control of code: marks with a place, a chance and a severity from patterns, and a score for display; a mark never decides, and only a pattern with the numbers steers a lane (off by default) |
| `reflexes` |  | experimental | base | commands, needs, pages, reflexes, settings, views | reflexes: a command that runs by itself when a sentence of yours holds, judged by one decision model; off behind the setting reflexes.on, and both shipped reflexes are off |
| `shape` |  | experimental | base | events, folds, kinds, needs, phrases, rules, settings, templates, texts | prompt shaping: while you type, notes with sources are added under the line; you keep the rows you want, and the line goes as typed (off by default) |
| `skills` |  | experimental | base | commands, pages, settings, skills, views | the skill library: J4n's own skills, a page to search, read and switch them, and the few that fit a task go into that doer's brief; the pick has one switch, `skills.pick` |
| `terminal` |  | experimental | base | connectors | tmux panes as tools of a model: list, read, type into, press a key in (each command asked about, as any command is) |
| `test` |  | experimental | base |  | a scripted echo connector for offline tests |
