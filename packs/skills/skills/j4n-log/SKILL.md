---
name: j4n-log
when: log, logs, events, event, trace, replay, debug, history, why did, .j4n/log, kind:read
where: crates/j4n-core/Cargo.toml
cost: 220
---
# Read a J4n Log

The Log is the project's `.j4n/log`: one JSON event per line, appended in order and chained by hashes. Every other view (chat, tasks, the money counts) is a fold of it.

- Each event has `seq`, `kind`, `actor`, a `thread` or `task`, and `data`. Kinds are dotted words: `task.started`, `run.tool_call`, `chat.say`, `brief.rendered`, `brief.skills`, `gate.verdict`.
- To follow one task, keep the events whose `task` is its id (`t12`). To follow a thread, keep its `thread`.
- `brief.rendered` says which sections of the brief a doer was given; `brief.skills` says which skills were picked and why.
- `gate.verdict` says what the Gate decided for an effect and which rule decided it.
- `jq -c 'select(.kind|startswith("run."))' .j4n/log/*.jsonl` is a quick filter.
- The Log is read, never edited: a file under `.j4n` is not written by a task.
