---
name: culprit-commit
when: bisect, regression, introduced, culprit, history, broke, regressed
description: Read when something used to work and the task is to name the commit that introduced the bug. It says how to find the first bad commit without touching the working tree, and how to tell it from the commit that only exposed it.
cost: 230
---
# Find the commit that introduced a bug

- Do not check commits out in the working tree. Export each one (`git archive <sha> | tar -x -C <tmp>/<sha>`) or use `git worktree add`, and run the failing check there. Loop over `git rev-list --reverse HEAD`; keep a table of sha, subject, pass or fail.
- The first red commit is where the check goes red. It is not always where the wrong code was written: an older change can be harmless until a later one removes what hid it.
- Read the diff of the first red commit and of the ones just before it (`git show <sha>`). The culprit is the commit that wrote the wrong behaviour itself, which may be earlier than the first red one. Confirm it: undo only that change in an export and the check passes.
- Write the answer in the exact form the task asks (a full sha, not a short one: `git rev-parse <sha>`).
- Fix the code on top of HEAD, so that it is right for every caller, not only for the failing check. Do not rewrite history and do not edit the tests, unless the task says so.
