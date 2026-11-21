# Using J4n

This guide is for someone who has J4n installed (see the [README](../README.md)) and wants to know what is on the screen and
what each part does. Features marked *experimental* are built but their benefit has not been measured; they are off by
default.

## 1. First run

```bash
cd your-project
j4n
```

The first time, J4n shows the **Setup card**. `⏎` opens **Agents › Setup**, one page of steps done with keys: an agent that
works (its sign-in, or its key), a model, the Decider, and a test that makes one short call to each part. `esc` puts the card
off until the next start. Once you have opened Setup from it, no project asks again. `:setup` opens the page at any time, and
a line typed while no agent works leads there. The [install guide](INSTALL.md) walks through the steps.

`j4n doctor` lists the same facts without starting the screen:

- each agent it found on your machine (Claude Code, Codex, Gemini CLI, OpenCode, Pi) and whether it is signed in, with the
  row of Agents › Setup that mends it;
- the keys it knows of (shown as "set", never the value) and the plan rows with their prices;
- the safety defaults: a write outside the project asks first, J4n's own folder and git's internals are blocked, a
  checkpoint is made before work, and an agent's writes and commands come to the gate first;
- which packs are off, and why.

J4n never reads or stores an agent's login. Sign an agent in from its row on Agents › Setup (`s` starts the program's own
sign-in and shows its link), or the way you always do; J4n uses the agent as it is.

`j4n` in a terminal opens the screen on the **Chat** tab. In a folder it has not worked in, it makes `.j4n/` there (a
git-ignored folder that holds that project's Log) and says so in one row. It refuses to start in your home folder or in `/`
unless you add `--anywhere`: go into a project folder instead.

With no terminal, J4n reads lines from stdin: each line is one turn of one conversation. `j4n run "…"` runs one task and
prints its answer. In both, an approval is denied unless you say `--approval ask` (the next line answers) or
`--approval full` (yes to whatever the gate does not block).

Useful options: `--doer` and `--model` to pick the agent, `--mode read` for a task that may not write, `--place` for where
work happens, `--wall 15m` and `--usd 2` to bound a task. `j4n --help` lists them all.

## 2. Moving around

The screen has at most nine tabs. `⌥1`–`⌥9` go to a tab and `F1`–`F9` do the same; a tab that has sub-tabs moves to the next
one when you press its own number again (on Chat, `⌥1` again is the next chat), and `⌥0` shows the numbers of the tabs. On a Mac,
let the terminal send Option as Meta or Alt. The last row shows the keys of the page you are on, and `?` opens the whole list.
`⏎` opens a row, `/` finds, `u` undoes the last change you made on a page that has one.
Anything you can do with a key can also be asked for in words in the Chat, or by name with `:` (the palette lists every
command).

Five tabs stand out of the box:

| # | Tab | Sub-tabs | What it is for |
|---|---|---|---|
| 1 | **Chat** | | the conversation; approval cards appear here, above the line you type |
| 2 | **Usage** | | where each plan's limit windows and each gateway's spend stand (what `/usage` says), what was spent a day, and what each task cost this week |
| 3 | **Agents** | Setup · Overview · List · Models · Providers · Plans · Deciders | setting up an agent, a model and the Decider; what J4n is connected to, and a line to connect a new program; every model J4n can use and how it is had; each provider's key (never shown); plan prices |
| 4 | **Log** | | every event, as it happens |
| 5 | **Settings** | Features · All · Keys · Packs · Commands | the switches you change, each with one line on what it does; every setting; the keys; which packs are on |

**Tasks** (Tasks · Decisions) comes while `lanes` acts on its own (`auto`, `wide`, `orchestrate`). A pack's tabs and sub-tabs come
with the pack or its switch (Quality, Agents › Accounts, Kit, Skills, Reflexes, the reports under Usage…). Agents › Tools, Services, MCP
and Trust stand only while they list something, and Agents › Providers lists the providers with a key kept or whose program is here.
The switch **Settings › Features › tabs.all** brings back everything else: Tasks, **Grow** (Grow · Contribute · Updates · TODO), Agents ›
Lab, Settings › Look, This Session, Loop and Gates, every provider, and the pages that are still empty. Settings are documented
where you change them: each setting row shows its own explanation.

The marks (the icons of the tabs and rows) are `auto` out of the box: icons where the terminal draws them itself or a Nerd
Font is installed, plain Unicode where neither is found. The row `theme.marks` (Settings › Features) says which one `auto` chose and why. If the marks
show as boxes, pick `unicode` there; for the icons, install a Nerd Font and choose it as the terminal's font
([install guide](INSTALL.md#icons-and-fonts)).

## 3. The gate and approvals

Every effect an agent asks for (a write, a shell command, a network request, a tool call) goes through one gate before it
runs. The gate **blocks**, **asks** or **allows**, in that order of strength, and nothing loosens a block. When it asks, an
approval card appears over the Chat: `⏎` takes the recommended answer, a digit takes that option, `?` says why. An option that
widens what is allowed needs its digit pressed twice.

- **Places.** `here` works in your folder behind a checkpoint you can rewind to. `worktree` works in a copy and lays the
  result into your folder when it is over (the setting `work.apply`). `box` runs the task behind a wall (bubblewrap): the
  host is read-only, secrets are hidden and the project folder is writable. `worktree-box` is both.
- **Read-only.** A task started with `--mode read`, or a step that is only a review, may not write.
- **Grants.** An approval can hold once, for this task, for this session or always here. `:grant` lists them and takes one
  back.
- **Gates by pack.** A pack that adds a gate, a check or a box asks twice before it turns on, because it widens what J4n may
  do.

The setting pages under **Settings › Gates** and **Settings › Loop** (with `tabs.all` on) show every rule and every limit with its reason.
[docs/GATE.md](GATE.md) explains the gate, the key guard and the known attacks it watches for.

## 4. Cost and checks

Each task carries a cost line. For an agent on a subscription, J4n reads the limit windows the provider shows and says what a
task took of the plan; for a gateway, it says dollars. The setting `cost.show` picks `plan`, `list` or `both`. **Usage** shows
the week, per task and per account. `:usage` shows where each window stands and when it runs out at the current pace. Set the
price of a plan under **Agents › Plans**. `--usd N` bounds a task: past the limit it is parked.

After a task that changed files, J4n runs the project's own check where the work happened (for example `cargo test`, `go test`
or `pytest`, found from the files in the folder) and reports whether it passed. The setting `check.after` turns this off for
a project whose tests take long. A check that cannot run here (no network, a missing tool) is neither a pass nor a failure.

## 5. Grow

Say what you want in the Chat ("add a check for…", "route reviews to a cheaper model"). J4n writes a small pack file and lists
it under **Grow** as a proposal with its reason and what it widens. Nothing applies until you accept it (`a`); you can decline
(`d`), and an accepted proposal can be undone (`u`). Accepting a proposal that widens what J4n may do asks again first.

Under **Settings › Packs** each pack can be turned on or off (`⏎`). A pack acts from the next start.

## 6. Connect, models and accounts

**Agents › Setup** is where a first agent, its sign-in or key, a model and the Decider are set up, and where one short call
proves them. J4n connects a program by describing how it speaks, not by code. **Agents › Overview** lists what J4n is connected to and
the programs on this machine that it is not; use its line to add a program or a path. The **Connect** flow
looks at the program, drafts how to speak to it, tries it once in a box and proposes it as a pack row. Codex, OpenCode and Gemini
CLI come with the `connect` pack as samples of the same thing.

**Agents › Models** lists every model once, however it is reached: the connector, the account and its plan, a router. A model's row says
whether it can be used now (usable, limited, no key, not signed in, not installed) and what it costs. A provider's key is typed
on its row in **Agents › Providers**, kept by J4n and never shown again.

## 7. Packs

J4n basic loads `base`, `guard`, `grow`, `connect`, `lanes` and `crew`, plus any pack your folder already uses. `lanes` and `crew` act only under the word `orchestrate` of the setting `lanes`, the one switch of that mode. The setting `packs.full` loads
every built-in pack. A pack you turn on or off under **Settings › Packs** wins over both. Every pack below is *experimental*.

| Pack | What it adds | Default |
|---|---|---|
| `base` | the screen, the gate's default rules, settings, commands, plans and the loop | on |
| `guard` | redaction that keeps a secret's shape, secret files shown to a box as their view, MCP tools held to their pins, a run that read untrusted content kept from sending sensitive data, and an alert when something matches a known attack | on |
| `grow` | proposals, tips and the author flow | on |
| `connect` | the Connect flow and samples for Codex, OpenCode and Gemini CLI | on |
| `accounts` | several logins of one agent kept apart: each account has its own folder, browser window and, if needed, proxy | off |
| `agent` | the own agent: a model on your gateway key, with a work flow and tool connectors | off |
| `code` | plan, work and validate flows for coding, worktree defaults, review mode | off |
| `jev` | decider connectors and a small model that answers some of the questions of the loop (never an approval); with no key they are not made | off |
| `offload` | light chores (reads, searches, test runs) go by rule to a cheaper agent; the main agent stays the only writer | off |
| `lanes` | a planner that keeps the plan and hands work to branches | on (J4n basic); acts under `auto`, `wide` or `orchestrate` |
| `crew` | seats: a role, a way of working and an access for each chat the main agent briefs; `crew.on` is `auto`, so it follows `lanes = orchestrate` | on (J4n basic); acts under `orchestrate` |
| `kit` | a Python tool for the own agent, and `j4n py` for other agents | off |
| `reflexes` | commands that run by themselves when a sentence of yours holds, judged by a decider | off |
| `skills` | a library of skills; the few that fit a task go into the agent's brief | off |
| `contribute` | notes of what this J4n finds wrong with J4n; nothing leaves your machine in this version | off |
| `inbox` | lines typed while a task works get a tag and a receipt instead of steering it at once | off |
| `habits` | show-only detectors of waste: tiny steps, a cold cache, rereads, big outputs | off |
| `ledger` | one report per settled thread, read from the Log | off |
| `parts` | the parts of a document run one at a time as plan items, with a commit of each | off |
| `qc` | marks on code with a place, a chance and a severity; a mark never decides | off |
| `shape` | notes with sources are added under your line while you type; you keep the rows you want | off |
| `bench` | cases that run tasks and score them (they spend money; you start them) | off |
| `terminal` | tmux panes as tools of a model: list, read, type into, press a key in | off |
| `test` | a scripted echo agent for offline tests | off |

Several of these have a setting that must also be on before they act (for example `kit.mode`, `reflexes.on`, `inbox.mode`).
The pack's page says so.

## 8. Chats

The setting `chats.on` lets every prompt be a chat of its own beside the top conversation. `:new` opens one, `:chats` lists
them (the ones waiting for you first), `:top` returns to the orchestrator, `:archive` tidies the list. `:clear` empties what a
chat shows while the Log keeps every row. A chat that gets long can be shrunk at a settled boundary with `:compact`, and
`:context` shows what each doer holds and what the next line would cost.

### Orchestrate mode: one switch

Orchestrate mode makes the top conversation a main agent, Main, that keeps a plan and hands parts of a line to chats beside it.
One switch turns it on: the word `orchestrate` of the setting `lanes`. Under it the chats, the read of each line, the crew's seats
and the plan keeper act by themselves; `orchestrate.look`, `crew.on` and `crew.envoys` are `auto` and follow the switch, and an
`on` or `off` you set on one of them wins.

1. **Turn it on** in one of these ways:
   - type `:lanes orchestrate`, or run `j4n lanes orchestrate` in the project's folder;
   - Settings › `lanes`;
   - on **Settings › Packs**, the `crew` row's `g` key, *ready to go*: enabling the crew with it on sets `lanes = orchestrate`.

   The switch's row on the Tasks page says what it turned on and what is missing. `⏎` on it turns the mode off, `u` takes the last
   change back, and `:undo` takes back any setting edit.
2. **Answer the needs card if one shows.** A feature that lacks something says so before it acts, on one card: a model for the
   read, a key, a login, or a Decider. You can take the suggestion, pick another, go on without it (the card says what that means),
   or turn the feature off. The card does not come back once the need is met. A folder with no git commit gets its own card:
   chats that write work in a worktree cut from the latest commit, so it offers to make a first one (`git init` and an empty
   commit; a chat sees only what is committed), or to go on with Main doing the whole line.
3. **Type a line as usual.** The row *reading the line …* shows who reads it and for how long. A Decider reads first; the cheap
   tier's model (on claude, Haiku) answers only what is still open; with neither, the line goes to Main as typed, and the row says
   why. A line becomes one of three kinds:
   - an **answer**: Main answers it;
   - **one** piece of work: Main does it. When Main changed files, another doer reviews the change without having seen it made,
     and its concerns go back to Main, up to three rounds; what is left after that is yours (`orchestrate.review`, on);
   - a **split**: Main writes a brief for each part, and each part runs as a chat of its own. A new program with several features
     (a game with its pickups and its screen, an app with its pages) is a split: Main sets the modules and how they talk, and
     the chats build them side by side while Main writes its own part (its part is checked with theirs, at the join). When their
     work is laid in, Main joins the parts, runs the checks and the program, and that turn is reviewed too. A part that was
     stopped (at its cap, or sent back three times) is named to Main with the folder its work is held in, and laying it in is
     yours to decide. `orchestrate.chat_top` (`sonnet`, say) is the dearest model a chat runs on, whatever Main briefed.

   A chat that writes works in a worktree of its own. Its report is checked against its files and the commands it ran. Then it is
   checked, reviewed by another doer, and laid into your folder one at a time. A chat whose report does not hold is sent back,
   up to three times. The review sends it back only for a case that fails; its other points, and a point raised again after
   the chat answered it, go to Main. A digest row tells Main and you what came of each chat.
4. **Name a doer or a model only with exact syntax**: `@claude sonnet fix the bug`, `@pi …`, `@c2 …` for an open chat, or `:use`.
   No word in the middle of a line picks a doer.
5. **Pick by hand instead of a model**, if you like: put the ask point's id in `pick.hand` (for the read, `read:kind`). Each time, a
   one-key row shows the options numbered, with the suggested one first: a digit picks, Enter takes the suggestion, Esc takes the
   fallback.

The meter in the composer is a small circle that fills with the context used; pages with room show a short line, and only Usage
shows the long bar. `orchestrate.writers` (2 by default) caps how many writing chats run at once; a part over the cap stays with
Main. `crew.goal = quality` adds a review seat and a critic, and costs more. Envoys still act only under a charter you signed.

## 9. Where J4n keeps things

| Place | What |
|---|---|
| `<project>/.j4n/` | the project's Log, folds and memory; it is git-ignored and blocked to agents |
| `~/.j4n/` | your own layer: packs you install, settings, the look, accounts' folders; `J4N_HOME` moves it |

Settings you change are written for the project or for yourself, as a change that can be undone, and take effect at once
unless the row says "next start". Every network client J4n builds honours the `https_proxy` family of variables; an account's
own proxy replaces it for that account.

## 10. Writing and installing packs

A pack is a folder of TOML. `j4n pack new NAME` makes a template, `j4n pack check NAME` checks it, `j4n pack install NAME`
copies it into `~/.j4n/packs/`, off until you turn it on. [docs/PACKS-GUIDE.md](PACKS-GUIDE.md) is the full reference.
