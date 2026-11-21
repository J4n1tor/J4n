# The gate, the key guard and the known attacks

J4n runs agents that can write files, run commands and use the network. This page says what stands between an agent and
those effects, what J4n does about secrets, and where its protection stops. The pack that carries most of it is `guard`,
which is on by default. Everything here is marked *experimental* until the maintainer says otherwise: it has tests and
drills, not an independent review.

## 1. One gate

Every effect calls the gate once before it runs: a file write, an applied change, a shell line, a network request, a tool
call, a message to another chat. The gate answers one of three things, and the strongest rule that matches wins:

| Answer | What happens |
|---|---|
| **block** | the effect does not run. No grant and no approval changes a block. |
| **ask** | an approval card appears. You answer once, for this task, for this session, or always in this project. `:grant` lists your grants and takes one back. A yes for a file inside the project also offers the folder, and a yes for a program you already let run in this folder offers that program (`python3 …`), both for this session only, and never for what the Gate blocks or asks about by its own rule (`rm -r`, a line of several commands, a write outside the project). |
| **allow** | it runs. |

J4n starts Claude Code so that its tools that change things or reach out are put to the gate whatever allow rules your own
Claude settings hold. Other agents are held by the settings J4n gives them, or by a box.

Rules are data: TOML rows in packs (`gate` rows) with a condition, a verdict and a reason that is shown to you. **Floors**
no pack can remove: a block outranks an ask; a grant never loosens a block; an approval is never given to a machine
answerer, only to you; the choices a question offers are the only ones accepted. A pack that adds a gate, a check or a box
asks you twice before it turns on, because it widens what J4n may do.

### What the default rules do

| Effect | Default |
|---|---|
| A step you marked read-only (`--mode read`, a review) writing or running a line that writes | blocked |
| A task that only looks read-only writing | asked |
| Reading an environment file, a private key, or a credentials folder | blocked (example, sample and template files and public keys are not) |
| A shell line that reads, copies, uploads or prints a credentials file or login | blocked |
| Writing outside the project | asked |
| Writing into git's internals or J4n's own folder (`.j4n`) | blocked |
| Wiping a disk or a home folder, fork bombs, piping a download into a shell | blocked |
| A push of any kind | blocked: pushing is yours to do |
| Deleting recursively, discarding git changes, publishing a package, installing, killing processes, `sudo`, rebooting | asked |
| A line the matcher cannot read | asked: an unreadable line is never assumed safe |
| Sending data classed as secret to a provider | blocked |
| Sending private or unclassified data beyond your machine | asked |
| Plain reads, `git status`/`log`/`diff` and the project's test runners | allowed |

The list is data you can read and change under **Settings › Gates**. Where a pack of yours says more, a higher layer wins.

## 2. Keys and secrets

**No agent is shown a key that J4n holds.** A key you give J4n (typed on its row in Agents › Providers, or in the
environment J4n starts) is kept by J4n. Everything J4n writes to its Log and everything it shows a model goes through the
redactor first.

The redactor is local and deterministic; no model is involved. It finds a secret from:

- the values you registered;
- about twenty-five vendor shapes (OpenAI, Anthropic, OpenRouter, Stripe, GitHub, GitLab, Slack, AWS, Google, Hugging Face,
  npm, PyPI, Twilio, JWTs and others);
- structure: private keys, passwords in URLs, `Authorization` headers, connection strings and `--token X` flags;
- names, in configuration files and for quoted values in code (`api_key = "…"`).

It leaves look-alikes and ordinary code alone, so that a log stays readable. What replaces a secret keeps the schema and
nothing random, so an agent can still tell what it is looking at:

```
OPENAI_API_KEY=«REDACTED openai_key starts=sk-proj- length=104 charset=alnum same-as-id=271d»
```

It keeps the kind, the vendor's fixed public prefix, the length and the character set; `same-as-id` is four hex characters
of a keyed hash that differ per machine, so two files that hold the same secret show the same id and no value can be
recovered from it.

**Secret files behind a box.** For a task in a `box` or `worktree-box`, the files named by the setting `guard.files` (by
default `.env`, `*.pem`, `*.key`, `id_rsa`, `credentials.json`, `.npmrc`, `.netrc` and similar) are mounted over with their
redacted view, read-only for a task that only reads. J4n checks after mounting that a reader gets the view, and does not
start the program otherwise. If an agent edits such a file, J4n puts the real values back only where every placeholder is
unchanged, and refuses the write otherwise.

**Untrusted content (taint).** Once a run has read content that is not yours (a web page, an MCP server's result, an
endpoint's answer, a `curl` or `gh` line), the rest of that run may not send anything out or write where someone else runs
it later: no network lines or publishing, no calls to connected endpoints, no MCP tool that writes, and no writes to shell
startup files, `.ssh`, key files, CI workflows, editor tasks or `.j4n`. These are blocks, so no grant covers them. The run
ends with your next line.

**Programs J4n starts get a cleaned environment.** If J4n itself runs inside another agent's terminal (the Claude desktop
app, Codex, OpenCode, Pi, Gemini CLI, Herdr), it inherits that host's session variables: messaging sockets and tokens,
session and account identifiers, pane and workspace ids. Passed on, they would let a doer reach the host's other sessions.
J4n removes them before it starts anything: every doer, shell line, tool, Python kernel, MCP server, relay, the connect
lab, its own relaunch and the quality runner. The names are data (`host` rows in `packs/base/hosts`, each with where it was
read from), and a host J4n does not know is yours to add as a row of your own. Your own URLs and config (a gateway's base
URL, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`) are kept. A law lists every place J4n starts a program and fails if
one is added without the cleaning, or is listed without cause; the few that carry no agent (`git`, `kill`, the box probes)
are named in it.

**Your own keys go only to the program that uses them.** A variable of the environment you started J4n in whose name ends in
`_KEY`, `_TOKEN`, `_SECRET` or `_PASSWORD` (`OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, `GITHUB_TOKEN`) is taken out of every
program J4n starts, and given back only to the one whose connector names it (its `pass`, `secrets` or `probe.signed`): Claude
Code gets `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN` and the AWS keys of Bedrock, Codex
`OPENAI_API_KEY` and `CODEX_API_KEY`, Gemini CLI its Google keys, Pi and OMP the providers' keys they read, OpenCode the common
ones (Anthropic, OpenAI, OpenRouter, Gemini, Google). Every agent program also gets the names of the setting `keys.pass`
(Settings › `keys.pass`; `GH_TOKEN GITHUB_TOKEN` by default, so an agent's own `gh` and `git` stay signed in). A tool, a shell line of
J4n's own agent, the Python kernel, an MCP server, git and the box's probe get none. J4n's own agent and the Deciders read their
keys inside J4n, as before. A program that reads a key nothing names no longer finds it in its environment: add the name to
`keys.pass`, or to your own row for it (`pass`).

**Another project's state is refused to a doer; its own stays readable.** J4n and the agent CLIs keep state per project:
every transcript under `~/.claude/projects`, each project's `.j4n` (Log, memory, folds) and the rules and checkpoints under
`~/.j4n`. A doer working in one project may not read what is kept of another. The gate decides this from the folder the doer
really works in, by whole path components (`…-app` is not `…-app-old`), and a read of the doer's own state is allowed
outright so that it does not cost a card for every tool result. A read of another project's state is a block, with no card
and no grant, as for a credential. For shell lines every word is examined (arguments, redirects, an interpreter's code,
`cd`), `$HOME` and `~` are expanded, and a relative word is read from the doer's folder until the line changes directory.
Claude Code is also asked about these paths through its own settings, so the gate answers instead of the agent.

**MCP tools are pinned.** A tool of an MCP server is offered only if the hash of its description and schema is one you
pinned. A tool that is new, or that changed after you pinned it, is not offered and the pin is proposed to you.

## 3. Boxes and places

A task works in a place: `here` (your folder, with a checkpoint to rewind to), `worktree` (a copy laid back when it ends),
`box` (here, behind a wall) or `worktree-box`. The box is bubblewrap: the host is read-only, `/tmp` is private and empty,
credentials and J4n's own folder are empty, the project folder is writable, and the process dies with J4n. Network use in a
box is the setting `box.net`. A box needs `bwrap`; where none can run, the setting `box.unavailable` says whether the task
is refused or runs unboxed with a notice.

## 4. Known attacks

When what happens looks like a known attack, J4n shows an alert row that says which one ("like s1ngularity · blocked") and
writes it to the Log. These are patterns over the events the gate already wrote; each match is a block that already
happened, so the alert tells you what it was, it does not decide anything. They are data in the `guard` pack and update
like any pack.

| Attack | What happened | What J4n does | Source |
|---|---|---|---|
| **s1ngularity** (Nx, Aug 2025) | malicious npm packages used the installed Claude, Gemini and Q CLIs to search the machine for secrets and publish them | blocks reading, copying or changing a key or login file; alerts | https://blog.gitguardian.com/the-nx-s1ngularity-attack-inside-the-credential-leak/ |
| **GitHub MCP** (May 2025) | a public issue got an agent to leak private repositories into a public pull request | after untrusted content, blocks network lines and MCP tools that write; alerts | https://invariantlabs.ai/blog/mcp-github-vulnerability |
| **postmark-mcp** (Sep 2025) | a malicious MCP server copied every mail to the attacker after a later version changed a tool | the changed tool is not offered; alerts | https://snyk.io/blog/malicious-mcp-server-on-npm-postmark-mcp-harvests-emails/ |
| **Supabase MCP** (Jul 2025) | a support ticket made an agent run SQL with a privileged key | after untrusted content, blocks database tools that write and calls to connected endpoints; alerts | https://simonwillison.net/2025/Jul/6/supabase-mcp-lethal-trifecta/ |
| **Amazon Q** (Jul 2025) | a wiper prompt shipped in a signed release of the extension | blocks a line that wipes a disk or a home folder; alerts | https://cyberinsider.com/amazons-ai-coding-assistant-for-vsc-infected-with-data-wiper/ |

## 5. What this does not do

Say it plainly:

- **Outside a box, J4n cannot hide a file from an agent's own read tools.** The gate blocks the shell lines and tool calls
  that hand a credentials file over and asks about the rest, but the redacted view is a mount, and a mount needs a box.
- **An agent's own login file** (for example the credentials an agent keeps in its home folder) is read by that agent to
  work. J4n never reads or stores a login, and cannot hide it from the program that needs it.
- **Keys are kept from agents only where J4n holds them.** A key an agent finds in your environment or files by itself is
  outside what J4n controls, except as above. An agent passes its own environment, the key J4n gave it included, to the
  commands it runs itself.
- **J4n's memory is closed to other programs on Linux only.** There J4n makes itself not dumpable when it starts: it leaves
  no core dump, and no other program of yours, an agent among them, may read its memory or its environment or attach a
  debugger. `J4N_DUMPABLE=1` in the environment J4n starts in leaves it open, for a debugger. On other systems J4n leaves
  this to the system's own rules.
- **A pattern match on paths does not close everything.** The protection for other projects' state, like the one for
  credentials, reads the words of a line. It does not see into a script that an agent writes and then runs, a symbolic link,
  `find /` or a recursive read from a folder above both projects, a glob only the shell expands (`~/.cl*/proj*`), a variable
  (`$X/projects`), a relative word after a `cd` that left the folder, or a search tool given an absolute pattern. It also does
  not cover the transcripts that Codex, Pi and OpenCode keep in their own folders, and Codex does not ask the gate for reads.
  Where a box is on, the box is the wall for these cases.
- **The redactor is shape- and name-based.** A short password under a name that does not say "password" can be missed, and
  nothing here understands what a secret *means*.
- **A decider model is never asked about an approval,** but anything you let a decider see goes to the model you chose for
  it. Settings that send text to a decider (`kit.judge`, `reflexes.judge`, `qc.jev`) are off until you allow them per project.
- A `--approval full` run says yes to everything the gate does not block. Use it only where you would use any unattended agent.

To report a problem in the gate, follow [SECURITY.md](../SECURITY.md).
