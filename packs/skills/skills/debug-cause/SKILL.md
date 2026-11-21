---
name: debug-cause
when: bug, bugs, failing, fails, failure, traceback, wrong, regression, debug, cause
description: Read when a test or a behaviour is wrong and the cause may be in another file than the one where it shows. It says how to reproduce, find where the wrong value is made, fix it there and add a test that fails on the old code.
cost: 230
---
# Fix a bug at its cause

- Reproduce first: run the failing check and read the message. Note the first wrong value, not the last symptom.
- Trace that value backwards, caller by caller (`grep -rn`), to the place that makes it wrong. Fix the code there, not where it shows, and not in the test. Do not edit tests or the spec unless the task says so.
- There may be more than one cause. After a fix, run the whole suite again, and keep going until it is green.
- After changing a function, grep its other callers: the fix has to be right for every one of them, not only for the failing case.
- When the task asks for a regression test, add it next to the existing tests, written so that it fails on the old code (run it before the fix) and passes after.
- Say what the cause was and what you changed, in two or three lines.
