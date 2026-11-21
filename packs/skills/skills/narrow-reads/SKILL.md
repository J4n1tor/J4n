---
name: narrow-reads
when: log, logs, summarize, summarise, digest, search, grep, find, which, where
description: Read before reading logs or other long outputs, or before a where-or-which search of a project. It says how to look narrowly: filter and count first, never print whole files.
cost: 100
---
# Read narrowly

- For logs and other long outputs use `grep`, `head`, `tail` and `sort | uniq -c`, and report counts and the few lines that matter. Do not print a whole file into the conversation.
- For a where or which question, run one `grep -rn` over the project rather than opening files one by one.
- Say what you looked at and what you did not.
