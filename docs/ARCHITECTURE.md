# Architecture

How J4n is built, for someone who wants to read or change the code. The user's view is in [GUIDE.md](GUIDE.md); the rules the
code keeps are in [CHARTER.md](CHARTER.md).

J4n is a Rust workspace of one binary (`j4n`) and sixteen other crates. It has three layers:

```
   screen              core                    the world outside
 ┌─────────┐       ┌──────────────┐       ┌────────────────────────┐
 │ j4n-tui │ ◄───► │  j4n-core    │ ◄───► │ j4n-connect, j4n-reach │ ──► agents, tools, MCP, web
 └─────────┘ proto └──────────────┘       └────────────────────────┘
        j4n  wires them together and reads the command line
```

The **core** holds the decisions. The **screen** draws what the core sends and reports what you did, over a protocol
(`j4n-proto`) of plain serde types; it reads no file and calls nothing in the core directly. **Everything outside** (a
process, a network request, a sandbox) is reached through `j4n-connect` and `j4n-reach`, so the core can be tested with no
agent and no network.

## 1. The eight parts

Every feature is built from eight parts, and a new feature is a pack that uses them, not a ninth part. A part grows only
when none fits, by one field or step that two features need.

| Part | What it is |
|---|---|
| **Table** | everything declared, as data with an `api` number: connectors, flows, rules, gates, ask points, views, pages, commands, folds, settings and kinds. It loads layers in order (built-in packs, installed packs, your files, the project's files), merges them field by field, and is the one writer: a change is validated, passed to the gate, written without losing comments, and logged so it can be undone. |
| **Log** | everything that happened, one event per line, append-only, in two places (yours, and the project's). One thread writes. A **fold** reduces events to a ledger (tasks, spend, accounts' limit windows, open questions, grants, changes); a ledger is never stored apart from the Log, and starts from a snapshot plus the events after it. |
| **Task** | all work. A chat turn, a flow step, a validation, a bench case and every question are tasks. Six states only: queued, running, cooling, waiting, done, stopped. A task works in a **place** (here, worktree, box). |
| **Doer** | who does a task: a connector to an outside program, a flow of steps, or a person. A doer reports a stream of events (text, tool use, shell output with its real exit code, cost, an explicit end). |
| **Ask** | every decision, as a child task answered by a chain: a rule, a decider, a model, then you. An answer must be one of the offered choices, and an approval is never given to a machine. J4n runs with no key. |
| **Gate** | every effect, checked once and purely: block beats ask beats allow, floors cannot be removed, grants never loosen a block. See [GATE.md](GATE.md). |
| **Rule** | what to do when an event happens: steer a task, ask, propose, cool down, raise a notice. A rule's actions carry their cause, and a chain that is too deep or too fast is dropped with a notice. A rule cannot kill, spend or change the gate. |
| **View** | one query and template layer with two renderings: a **page** for the screen, and a **brief** for a model, with a token budget and a class of data on each section so the same egress check covers prompts. |

Values the parts share (money, durations, sets, expressions) live in `j4n-core` beside them. An expression language
(`expr.rs`) is what gate conditions, rule conditions and view cells are written in, so a pack needs no code.

## 2. Packs

A pack is a folder of TOML that declares things of the kinds above, plus any script it points at. There is no plug-in
code. The `base` pack is compiled into the binary; so are the other built-in packs, which load only when on. Every file
starts with `api = 1`, unknown keys are refused, and a pack that needs another is left out with a reason if that one is
off. A pack that adds a gate, a check or a box asks twice before it turns on. The kinds themselves are TOML schemas in
`packs/base/kinds/`, checked against the Rust structs by a test. [PACKS-GUIDE.md](PACKS-GUIDE.md) is the reference.

## 3. The crates

| Crate | Role |
|---|---|
| `j4n-proto` | the protocol types only, shared by the core and the screen |
| `j4n-core` | the eight parts, the pack loader, the gate and its matcher, the redactor, the front that turns the Log and the Table into what the screen shows |
| `j4n-connect` | everything outside: the process runner, places (worktree, box), dialects for speaking to a program (plain text, JSON, JSONL streams, ACP, Claude's stream, Pi's RPC, HTTP APIs), the session pool, the built-in file, shell and git tools, accounts' proxies |
| `j4n-reach` | tools of a model that reach out: the web (fetch policy, search, reading) and MCP servers |
| `j4n-memory` | a derived index of the Log (SQLite full-text search) behind `:remember` and `:recall` |
| `j4n-tui` | the screen framework: fields, parts, ask cards, themes, the headless backend the tests use. Depends only on `j4n-proto` and UI crates |
| `j4n` | the binary: wiring, the command line generated from the command table, the restart guard |
| `j4n-shape`, `j4n-qc`, `j4n-bench`, `j4n-contribute`, `j4n-inbox`, `j4n-habits`, `j4n-ledger`, `j4n-parts` | the Rust halves of the packs of the same names, for the few things TOML cannot say |
| `j4n-lab`, `j4n-forge` | the connect lab: `j4n-forge` makes fake command-line harnesses from a seed, and `j4n-lab` scores how well J4n connects to them. Development tools |

The dependency rule is one-way: the core depends on `j4n-proto` only; the outside crates depend on the core; the binary
depends on all of them; the screen depends on nothing but the protocol. A law checks the list of crates and their sizes.

## 4. The protocol the screen consumes

The screen holds a **tree of nodes** that the core sends: a node is a tab, a sub-tab, a section or a field, and which one is
decided by depth. Pages are declared in packs; a pack that adds a page under `home` adds a tab (at most nine). A node holds
a **field**, and a field is one component: values you can edit (text, number, choice, path, secret that shows only set or
unset) and displays bound to data (table, list, chat, log, card, ask, chart).

Data reaches a field through a **binding** the core resolves (`table:<kind>/<id>.<field>`, `view:<id>`, `fold:<id>`,
`log:<glob>`, `asks:open`): the screen never evaluates one. A long list is windowed, so cost stays proportional to what is
visible.

| From the core (`Down`) | From the screen (`Up`) |
|---|---|
| `Hello`, `Tree`, `Value`, `Rows`, `Stream`, `Asks`, `Toast`, `Focus`, `Keys`, `Theme` | `Hello`, `Watch`, `Unwatch`, `Window`, `Intent`, `Draft`, `Resize`, `Quit` |

What you do is a typed **intent** (answer this question, set this setting, say this line, open this node), never a string
the core has to guess at. The look (palette, marks, frame, density) is data too, so the screen restyles without a rebuild.
The core and the screen run on threads connected by channels today; the protocol has no assumption that stops it going over
a socket.

## 5. The life of a line

1. **Read.** A typed line is matched against the phrase table (`packs/base/phrases/`) and the `[[command]]` rows. A match
   runs that command; nothing else is started. The list is in [reference/commands.md](reference/commands.md).
2. **Route.** Otherwise the line goes to the doer of its thread. With `weigh.on`, a cheap check first sorts it into
   `answer` (a cheap model, no tools), `light` (a lean session) or `task`. With `lanes = orchestrate`, a read of closed
   questions decides the kind, whether it goes on, which chat and which doer (`front/read.rs`).
3. **Run.** A task is queued, then runs in its place (`here`, `worktree`, `box`) through its doer's connector. The doer
   streams events: text, tool calls, shell output with its exit code, cost, an end.
4. **Gate.** Every effect the doer asks for (a write, a shell line, a network call) is checked once (§6). An `ask` verdict
   makes the task wait on an approval.
5. **Settle.** When the doer ends, the task settles. Rules react to `task.settled`; the base pack's `lane-check` starts
   the project's check (`packs/base/rules/check.toml`) when files changed and `check.after` is on. The command is the
   first row of `packs/base/checks/detect.toml` whose files exist in the project.
6. **Fold.** Views, cost and the screen are folds over the events the task wrote.

## 6. Asks, approvals and grants

Every decision is an ask point (`packs/base/asks/points.toml`) with a chain of answerers and a fallback. The chain is
walked in order; the first answer within the offered choices wins.

| Point | Chain | Fallback | Meaning |
|---|---|---|---|
| `approval` | `rule:gate`, `human` | `safe` | an effect the gate asks about; a rule or a grant answers, otherwise you |
| `inside` | `decider:jev?min_conf=0.8` | `pick:ask` | whether a line the gate could not prove harmless stays inside the project (only with `approval.inside`) |

An approval never reaches a model (law 6). With `approval.inside` on, the gate itself says yes to effects it can prove
stay inside the project (edits, test runs, linters, removing build folders). Anything reaching outside, such as the
network, a remote shell, a deploy or `git push`, still asks.

A **grant** is a remembered yes, scoped to once, the task, the session or the project; `:grant` lists them and `:grant
back <id>` revokes one. A grant never lifts a **block**: secrets rules, taint rules after untrusted input, and J4n's own
folders stay blocked.

Machine answerers have two limits (`j4n-core/src/answer/`). An answerer that fails 3 times in a row is turned off for
24 hours. A task gets at most 6 machine answers, and the same question is not put to a machine twice.

## 7. Settings and layers

A setting is a `[[setting]]` row in a pack (`j4n-core/src/setting.rs`). Its value comes from layers, last one wins: the
pack's default, then your files (`~/.j4n/`), then the project's (`~/.j4n/projects/<key>/`), then the session. A row with
`live = false` takes effect at the next start. A row with `auto = "<setting>=<word>"` follows another setting when its
value is `auto`. Every row is in [reference/settings.md](reference/settings.md).

## 8. The Log

One JSONL file per project (`<project>/.j4n/log/0001.jsonl`) and one for you (`~/.j4n/`). One thread appends. Every event
has a `kind` and, when it belongs to one, a task id. The kinds a reader needs most:

| Kind | Written when |
|---|---|
| `task.queued`, `task.started`, `task.waiting`, `task.resumed`, `task.settled` | the task loop moves |
| `run.tool_call`, `run.shell`, `run.message` | a doer acts or speaks |
| `gate.verdict` | the gate judges an effect |
| `ask.step`, `ask.answered` | an answerer in a chain is tried, and the ask is answered |
| `decider` | a decider answers a question |
| `rule` | a rule fires |
| `edit.applied`, `change.recorded` | a file or a setting changes |

`j4n-memory` keeps a derived full-text index of it; nothing else stores state apart from the Log.

## 9. Threads, not async

J4n uses OS threads, channels and a blocking HTTP client, with no async runtime. A task is at most a few threads and a
workload is a handful of tasks, so the simpler model wins: child output is read on its own thread, cancellation kills the
process group, and every network client is built by one builder that honours the proxy variables.

## 10. The laws

The rules the code keeps are in [CHARTER.md](CHARTER.md), and each has a test in `crates/j4n/tests/laws`. The ones that shape
the layout: the eight parts and a size ratchet, one loop, one Log with one writer, one gate, one ask, one way to connect, a
screen that draws only what it is sent, no domain in the core, and one network builder. [TESTING.md](TESTING.md) says how
they run.
