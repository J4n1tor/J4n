# What J4n is

J4n (J4n1tor in full) is a program you run in a terminal, in a project folder. It starts the coding agents you already
have, such as Claude Code, Codex, Gemini CLI, OpenCode, Pi or any other command-line agent, and sits between you and
them. Whatever the agents do passes through J4n first: it checks each step, writes it down, counts what it cost, and
can hand parts of the work to other agents.

J4n is not a model and not an agent of its own. Under it, each agent is still that agent, signed in the way you signed
it in. What J4n adds is one place around all of them.

This page describes J4n from several angles, because people come to it with different needs. Each section says what
works today and what does not yet. The feature list with defaults is in [FEATURES.md](FEATURES.md), and the screen is
explained in [GUIDE.md](GUIDE.md).

## Seven ways to look at it

### 1. A control layer for an agent you are required to use as it is

Some providers let you use their models only through their own agent program. J4n works within that. It does not
replace the agent, call the model behind it, or reuse its login: it starts the provider's own, unmodified CLI with the
sign-in that CLI already has. Around it you still get your own setup: the same approval rules, the same Log, cost per
task, checks after every edit, and, if you turn it on, a main agent that plans and hands parts to other chats.

You remain responsible for using each provider under its terms. J4n does not resell or proxy access, and it never reads or
stores a login.

### 2. Different providers for different work

You may want one agent for planning, a cheaper one for reading and test runs, and a third for a language it handles well.
J4n treats them as interchangeable doers:

- **Library** lists every model you can reach once, however it is reached (an agent, an account, a gateway key), with
  its price and whether it can be used right now.
- `@claude sonnet …`, `@pi …` or `:use` pick a doer for one line. Nothing guesses a doer from the words of a line.
- The `offload` pack (experimental) sends light chores to a cheaper agent by rule, while the main agent stays the only
  one that writes. It was cheaper in one measurement and not in another; it stays off by default.
- The `accounts` pack (experimental) keeps several logins of the same agent quarantined from each other: each has its
  own config folder, its own browser window for signing in, and its own proxy if you need one.

### 3. One screen instead of several

Each agent has its own interface, its own way of asking permission and its own idea of cost. In J4n they all appear on
one screen with nine tabs: Chat, Tasks, Library, Connect, Spend, Quality, Grow, Log and Settings. An approval looks the
same whichever agent asked for it. Cost is shown the same way: dollars for an API key, a share of the limit window for a
subscription. The Log keeps every turn, tool call, approval and cost of every agent in one readable list per project.

J4n also runs without the screen: `j4n run "…"` for one task, or one line per turn on stdin.

### 4. A safety layer that does not depend on the agent

Every write, command, network request and tool call an agent asks for goes through one gate. The gate blocks, asks or
allows, and nothing loosens a block. Writes outside the project ask first; J4n's own folder and git's internals are
blocked. Work happens behind a checkpoint you can rewind. Keys J4n holds are never passed to an agent, and secrets in
what is logged or sent to a model are replaced by a placeholder with the same shape. A task can also run in a worktree,
or in a `box`, a sandbox that sees only the paths it is given (Linux only).

The gate is pattern-based: a floor, not a promise. It reads the words of a step, not what a script does once it runs,
and not every agent asks it about everything (Codex does not ask about reads). Where that matters, a box is the wall.
[GATE.md](GATE.md) lists what it covers and what it does not.

### 5. Your own orchestrator

The `lanes` setting has an `orchestrate` mode. A main agent reads each line you type and decides whether it is an
answer, one piece of work, or a split. For a split, Main writes a brief per part, and each part runs as its own chat
in its own git worktree. Each chat's report is checked against the files it changed and the commands it ran, then
reviewed by another doer, then laid into your folder one at a time. The `crew` pack gives each chat a role, a way of
working and an access level.

This is experimental. In J4n's own benchmark runs, splitting has not yet beaten a single agent doing the whole job, so
it is off by default.

### 6. A base to build on, mostly without code

J4n is built from eight parts (Table, Log, Task, Doer, Ask, Gate, Rule, View; see [ARCHITECTURE.md](ARCHITECTURE.md)),
and everything else is a **pack**: a folder of TOML that declares connectors, rules, gates, screens, commands, settings
and flows. The screen you see, the gate's default rules and the connectors for each agent are packs too, so you change
them the same way you add something new.

- A new agent CLI is connected by describing how it speaks, not by code. `/connect <program>` probes the program,
  drafts that description, tries it once in a box and proposes it.
- Describe a change in the Chat ("add a check for…", "route reviews to a cheaper model") and J4n drafts a pack change
  as a proposal. Nothing applies until you accept it, and an accepted change can be undone.
- `j4n pack new` starts a pack by hand, and [PACKS-GUIDE.md](PACKS-GUIDE.md) is the reference.
- Conditions in rules, gates and views use a small expression language, so a pack needs no code. The few things TOML
  cannot say live in small Rust crates beside their packs.

### 7. A record of the work

The Log is append-only and kept per project in `.j4n/`. A fold turns it into tasks, spend, open questions and grants, so
the Spend tab, a thread's report and a restart all read the same events. After a restart the chat comes back where it
was.

## What J4n is not

- It does not make an agent smarter by itself. It adds checks, records and choices around the agent.
- It is not a hosted service. It is one binary on your machine; nothing leaves it except what the agents you run send.
- Most packs are *experimental*: built and covered by tests, but not yet shown to help on real tasks. The README's
  status table says which.

## What it could become

These are built only when someone needs them or contributes them. None of them exists yet.

| Idea | What it would do |
|---|---|
| The key never in the box | An agent in a box gets a dummy key of the same shape; the real one is swapped in at the proxy. |
| Remote places | Another machine as a place to run a task: a held ssh link, the project sent over, a diff back. |
| Contribute over GitHub | J4n already notes, on your machine only, what goes wrong with J4n. Next: with your sign-in, reports and fixes sent as issues and pull requests that you review first. |
| A standard bench | J4n run on a public set of terminal tasks, so its numbers can be compared. |
| Grow over its own source | Proposals that change J4n itself, not only packs, checked by its own tests. |
| Domain packs | Packs for work other than code, kept in repositories of their own. |
| More agents, better connected | Each CLI that J4n has run only a few turns with, used until its rough edges are known. |

If one of these matters to you, open an issue that says what you would use it for. [CONTRIBUTING.md](../CONTRIBUTING.md)
says how to propose a pack or a change.
