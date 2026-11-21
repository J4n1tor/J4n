---
name: j4n-docs
when: docs, decision, decisions, row, rows, plan, report, bug, bugs, docs/, kind:plan
where: crates/j4n-core/Cargo.toml
cost: 240
---
# Keep the docs' rows straight

The plan lives in `docs/`. A step is a row there; a choice is a row in the project's decision log; a bug is a row in its bug log. Each row has an id of its log's own form.

- Use only the ids the step was given. A spare id is taken only when no one else has taken it, and the report says so.
- A decision row says what was chosen, why, and what would change it. A rise of a crate's size ceiling (the file under `fixtures/` that holds them) needs a decision row of its own.
- Mark a step built in its own doc with the decision ids and what was left open.
- A bug found on the way gets a row; it is fixed in the step only when the step is about it.
- A commit message says the step and what changed in words.
