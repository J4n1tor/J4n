<!-- Made from the pack rows by crates/j4n/tests/laws/reference_docs.rs; do not edit. J4N_BLESS=1 writes it again. -->

# Commands

What a line can start with, per pack. A command of a pack that is off leads to turning the pack on.

## accounts

| Words | What it does |
|---|---|
| `:account` `/account` | Keep an account: `:account add claude work [plan=max5\|pro] [proxy=<url>]`, `:account use claude` (your own login, named, nothing copied), `:account signin work`, `:account open work`, `:account check work`, `:account remove work`; `:account add` with no name opens Agents › Add account. |
| `:accounts` | Show the accounts: whose login each is, its route, and where it stands. |

## base

| Words | What it does |
|---|---|
| `:fresh` | Forget the thread's doer and model and start the thread again: the next line chooses once more. |
| `:switch` `:doer` | Change the thread's doer, and its model: `:switch claude haiku`. The next line goes to the new one. |
| `:stop` `:abort` | Stop the task of the thread; its session is kept, so the next line carries on in it. |
| `:kill` | Stop the task of the thread and take its process down. |
| `:status` | Say which doer and model the thread is bound to, and what it is doing. |
| `:help` `:?` | List the commands. |
| `:quit` `:q` `:exit` | Leave J4n. |
| `:restart` | Restart: J4n is the build on disk now, with the chat where it was; if that build does not come up, the one before it comes back. A row that says `⟳ next start` has `r` for it, and the note of such an edit `⏎`. |
| `:probe` | Put the probe cases to the deciders and models that answer for the ask points, and say how each did: `:probe` or `:probe decider:jev`. One that has not passed does not answer a question. |
| `:accept` | Say yes to a proposal: `:accept p3`, or `:accept` when only one is open. What it changes is written, and `:undo` takes it back. A proposal that widens what J4n may do asks twice. |
| `:decline` | Say no to a proposal: `:decline p3`. It comes back only if its evidence has doubled. |
| `:undo` | Take back an accepted proposal (`:undo p3`), or, with no name, the newest change in effect. |
| `:tips` | List what J4n has proposed for itself, and what became of it, in the chat. |
| `:ask` | Send the line after it to the model as it is: `:ask show me the usage` is not taken for `/usage`. |
| `:pack` `:packs` | List the packs: which are on, which were left out and why. |
| `:lanes` | Say what acts on its own, or set it: `single` (a line is a task), `auto` (the plan keeper and scout and review branches), `wide` (also pieces in held worktrees) or `orchestrate` (Main, its chats and the crew: the one switch). Settings › All › lanes does the same, and `u` there takes it back. |
| `:grant` `:grants` | The grants you gave that still count (once, this task, this session, always here): `:grant` lists them, `:grant back <id>` takes one back, and the thing is asked again. Settings › Gates `x` does the same. |
| `:key` | Remove a provider's key that J4n keeps: `:key remove own`. Agents › Providers `x` does the same, on the card. A key is typed on its row there (`⏎`), never on a line: a line is kept in the Log. |
| `:calibrate` | Say what a request has to cost to teach J4n what a window of your plan is worth, and, with `yes`, run it. It spends part of the window, so it never runs by itself. |
| `:usage` `/usage` | Show where each subscription's windows stand (used, reset, pace, when it runs out at that pace) and what each gateway was spent today, as bars in the chat that stay up to date. Answered from the Log: no doer, no model. |
| `:context` `/context` | Show what each chat's doer holds and what the next line costs: per `doer · model`, how full its window is, the session's floor (what its first turn read before any word) and history (what it has grown by), and whether the next line finds the provider's cache warm or cold, as a row in the chat that stays up to date. Answered from the Log: no doer, no model. |
| `:resume` | Take the session the last change left back: the next line, once, carries on in the doer's old session and not in the new one `session.mode` began. With no change to undo, it keeps the session there is for the next line even if `auto` would have ended it. `:fresh` forgets it. |
| `:compact` `/compact` | Shrink this chat's doer session now, at a settled boundary (or at the next one when a task runs: law 9, never mid-turn): plain, `junk` (drop tool output nothing reads again, failed tries, repeats) or `keep <words>`. The doer's own compaction does it; one with none says so and nothing changes. `/context` shows before and after. |
| `:chart` `/chart` | Show a chart as a row of the chat that stays up to date: `spend` (a day's dollars and tokens), `wall` (task time), `bench` (how the cases pass) or `lanes` (branches a line). Answered from the Log: no doer, no model. |
| `:term` | Show the row of the project's docs that an id in an answer stands for (`ADR-12`, `BUG-7`): its words, the doc and the line, and the row as written. |
| `:watch` | Say what the tools J4n follows stand at (running, latest, tested), from what it knows; with `once`, ask them now, quietly, and say what changed (Grow › Updates `r`). It does nothing while `watch.upstream` is off. |
| `:config` `:settings` | Show every setting and what it holds. |
| `:proposals` | Show what J4n has proposed for itself, to accept or decline. |
| `:roster` | Show the Agents sub-tab of the Agents tab: the agents first, then the tools, services, MCP servers and deciders. |
| `:setup` `:doctor` | Open Agents › Setup: the agent, the model and the Decider, each with where it stands and the key that sets it, a key typed on its row and shown to no one (`:setup again` asks the programs again, `:setup test` makes one short call to each part that is set, `:setup doer <id>` picks the main lane's doer, `:setup doer <id> <account>` with the account it runs as). |
| `:signin` | Sign a program in with its own login, in its own folder (no account is made): `:signin claude`. J4n starts the program's sign-in, shows the link it gives and asks before it runs; Agents › Setup has it as `s` on the program's row. |
| `:plans` | Show the plan rows and their prices. |
| `:providers` | Show Agents › Providers: the connectors that speak a provider's API, and where each one's key comes from (never the key). `⏎` on a row types its key, `x` removes it. |
| `:library` `:models` | Show the models there are and how each is had. |
| `:look` | Say how the screen looks (palette, marks, frame, corners, title_place, density, motion, mouse); it is written for this project and changes at once. |
| `:plan` | Show the plan of the thread, edit it (`drop`, `state`, `set`, `add`; an edit wins over the planner), or begin one from the words after it. `replace` says yes to a new plan. |
| `:new` | Start a new chat (chats.on): `:new` opens one and goes into it, `:new <words>` opens one, sends it the words, and leaves you where you are. With the chats off it is `:fresh`, as it was. |
| `:clear` `/clear` | Empty what this chat shows and nothing else (chats.on): the Log keeps every row, it is a chain, and `:history` brings them back. In the orchestrator it empties its rows and not the list of chats. |
| `:history` | Bring back the rows `:clear` emptied in this chat (chats.on). |
| `:archive` `/archive` | Tidy the list of the chats (chats.on): `:archive c3` archives that chat, in a chat `:archive` archives it, and in the orchestrator `:archive` or `/archive` archives every chat J4n's own rules pick (its turns over and ok, nothing waiting, cooling or queued, no open plan item, idle for `chats.archive.idle`), saying which in one line first; no model is asked. A chat that runs, cools or waits for you is not archived, and the orchestrator never is. The archive is the last row of the list, closed; a line typed in an archived chat brings it back. |
| `:unarchive` | Take a chat out of the archive (chats.on): `:unarchive c3`. |
| `:chats` | List the chats, the ones that wait for you first (chats.on): `⏎` goes into one, `esc` closes the list. |
| `:top` | Go to the orchestrator, the top of the chats (chats.on). |
| `:remember` | Keep a note in the memory: `:remember the build needs --release`. |
| `:recall` | Find what was noted, said or reported by its words: `:recall release build`. |

## bench

| Words | What it does |
|---|---|
| `:bench` `/bench` | Run a case of the bench: `:bench note doer=claude`, `:bench all doer=own full`. `full` answers the doer's questions yes, `again` runs a hold-out once more, `bare` gives the doer no Brief (the doer alone, not what the packs add). |
| `:results` | Show how each case of the bench went for each doer. |

## code

| Words | What it does |
|---|---|
| `/plan` | A plan for what follows, and nothing is written: `/plan add a --verbose flag`. |
| `/design` `:design` | A design for what follows, and nothing is written: `/design a retry for the uploader`. |
| `/build` `:build` | Plan, build in a worktree, check, have another doer look, and bring it over on your yes: `/build add a --verbose flag`. |

## connect

| Words | What it does |
|---|---|
| `/connect` `:connect` | Connect a program J4n has never seen, with no code: `/connect gemini`. It looks at the program, drafts how to speak to it, runs it once, and proposes the connector for your yes. |
| `:connector` | Run a connection's canary once, in the box (`:connector test claude`), or turn it off (`:connector off claude`, which asks first, and takes effect at the next start) or on again. Agents › Overview has `t` and `o` on its row, and `r` learns it again. |
| `:connections` | Show Agents › Overview: what J4n is connected to, the programs on this machine that it is not, and a line to add one. |
| `:lab` | Show Agents › Lab: the runs of the connect lab, and the table of each. The lab is run from the command line, never from here. |

## contribute

| Words | What it does |
|---|---|
| `:wrong` | Say what J4n got wrong, in your own words: kept as a finding on this machine, with a fingerprint and a count. `:wrong <words>`. |
| `:contribute` | Show what this J4n found wrong with J4n. |

## crew

| Words | What it does |
|---|---|
| `:envoy` | Open an envoy (crew.envoys, `lanes` orchestrate): a chat that talks the work through with you, drafts the charter you sign in it, and tells Main. Every question of its work shows in its chat. |

## habits

| Words | What it does |
|---|---|
| `:habit` | How the habit detectors have done: `:habit list` says each rule's labels and precision; `:habit keep h4521` says a row was right, `:habit wrong h4521` that it was not. |

## inbox

| Words | What it does |
|---|---|
| `:inbox` | The lines typed while a task works: `:inbox` lists the open ones; `:inbox tag <id> <steer\|side\|note\|hold>` says what a line is (the same tag says the rule was right); `:inbox pin <id>`; `:inbox show\|auto\|off` sets the switch until J4n ends. |

## kit

| Words | What it does |
|---|---|
| `:kit` | The kit's notebook: `:kit export` writes the cells of the newest task that ran any as <task>.ipynb in the project folder (nbformat 4, plain JSON; Jupyter, VS Code and Spyder open it); `:kit export t12` writes another task's. |

## ledger

| Words | What it does |
|---|---|
| `:report` | The ledger: `:report` says the newest report of this thread again, rebuilt from the Log; `:report <thread>` another thread's; `all` the answers whole and every file and command. the reports under Usage lists the same. |

## offload

| Words | What it does |
|---|---|
| `:offload` | Say whether light chores (reads, searches, test runs, digests) go to a cheaper doer on their own, or turn it `on` or `off`; `yes` sends the review that waits. A proposal like any other: `:undo` takes it back. |

## parts

| Words | What it does |
|---|---|
| `:parts` | The part runner: `:parts <document>` reads its parts into a Draft; `:parts keep 3 1 2` / `:parts drop 2` change it; `:parts go` runs the next `parts.max` of them as plan items, each gated and committed; `:parts next` commits a green part when `parts.mode` is `show`; `:parts` says the Draft again. |

## qc

| Words | What it does |
|---|---|
| `:qc` `/qc` | Check the quality of what a path holds or a git range changed: `:qc src/`, `:qc HEAD~2..HEAD`. `:qc on` and `:qc off` are the switch; `:qc dismiss m1a2b3c4` and `:qc keep m1a2b3c4` label a mark; a dismissal may say why: `:qc dismiss m1a2b3c4 because intended` (`intended`, `pre-existing`, `wrong`, `later`; only `wrong` counts against the pattern). |
| `:quality` | Show the checks, the marks, the patterns and how they were labelled. |
| `:refactor` `/refactor` | Do a task that must keep what the project lists (public names, tests, doc headings and links) and be told what is gone after: `:refactor split parse.rs in two`. |

## reflexes

| Words | What it does |
|---|---|
| `:reflex` `/reflex` | Reflexes: `:reflex add <id> do <command> when <your sentence> check <condition> [at task.settled\|chat.say]` makes one (on), `:reflex off <id>` / `:reflex on <id>` switches one, `:reflex remove <id>`, `:reflex case <id> fire\|hold <state in words>` adds a probe case; `:reflex` alone lists them with the judge's last reason. They run only while the setting reflexes.on is on. `:toggle` and `:toggles`, the old names, still work. |
| `:reflexes` | Show the reflexes: what each does, when, whether it is on, and what the judge last said. |

## shape

| Words | What it does |
|---|---|
| `:shape` | Turn prompt shaping on or off: `:shape on`, `:shape off`, `:shape` to see which. |

## skills

| Words | What it does |
|---|---|
| `:skill` | The skill library: `:skill` lists the skills, `:skill tests` searches them, `:skill show <name>` reads one, `:skill on\|off <name>` switches one, as `x` on Agents › Skills does (`:undo` takes it back). |
| `:skills` | Open the Skills page: every skill, where it came from, and whether it is on. |
