---
name: log-hunt
when: logs, log, incident, outage, timeout, root
description: Read when the task is to find what failed first, or the root cause, in several log files. It says how to order events when the files come from different machines and how to avoid blaming the component that only shows the symptom.
cost: 230
---
# Hunt a failure through logs

- Look at the shape before the lines: `wc -l`, `head`, and one counting pass per file (`sed` away the ids and times, then `sort | uniq -c | sort -rn`). Do not read whole files.
- Files from different components may not share a clock. Do not order events by their time stamps across files. Order them by what is linked: a request id, a trace id, who called whom.
- The component that shows the first error is often not the origin. Follow one failing request id backwards through every file to the earliest component that saw something wrong; that one is the answer, and what it logged is the cause.
- Read how the README describes advisory lines and recoveries. A code that is followed by a recovery line, or after which requests kept working, is a blip and not the cause. The cause is the first error after which the failures do not stop, so check what happened to the component after each candidate.
- Use the cause names and mappings of the README or runbook in the folder, not your own words, when the task asks for a cause name.
- Check the answer against every file: the request you name must really have failed because of that cause, and must be the first such request in the linked order.
- Write the answer in the exact form the task asks. Do not change the logs.
