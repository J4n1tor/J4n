# Security policy

J4n sits between you and agents that can run commands and write files, so a flaw in it matters. Thank you for reporting one
responsibly.

## Reporting a vulnerability

Use GitHub's **private vulnerability reporting**: on the repository's **Security** tab, choose **Report a vulnerability**.
Please do not open a public issue or pull request for a security problem.

A useful report says what you did, what you expected, what happened, the J4n version (`j4n --version`) and your system.
A minimal case that shows the problem is the most helpful thing you can send.

J4n is maintained by one person, so there is no fixed response time. Reports are read, and a confirmed problem is fixed
before it is described in public.

## What is in scope

- The **gate**: a way for an agent's write, command or network use to run without the approval or block it should meet.
- The **key guard and the redactor**: a way for a key, password or token that J4n holds to reach an agent, a model, the
  Log or the screen.
- The **box**: a way out of a boxed task's walls.
- A pack or an MCP server changing what J4n allows without you accepting it.
- J4n's own folder (`.j4n`) and git's internals, which are blocked to agents: a way for an agent to write there.

Out of scope: a weakness in an agent itself (report it to its vendor), what an agent does inside a `full` approval you
chose, and the login files an agent keeps for its own use, which J4n never reads or stores.

## What J4n does about secrets

A secret J4n holds (a key you gave it, a token in the environment it passes on) is kept out of the Log and out of what a
model is shown: it is replaced by a placeholder that keeps the kind, the length and a short identifier, so a task can still
tell what the value was without seeing it. Files that usually hold secrets (such as `.env` and key files) are shown to a boxed
task only as that redacted view. A run that has read untrusted content is kept from sending or writing anything sensitive,
and MCP tools are held to the description and schema you approved. The details and the known limits are in
[docs/GATE.md](docs/GATE.md).

J4n has no usage reporting. Anything it fetches itself goes through the one network client that honours your proxy
variables, and the update watch is off by default.

## Supported versions

Until a stable release, only the latest commit on the default branch is supported.
