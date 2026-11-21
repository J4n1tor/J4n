# Notes for coding agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. The rules below matter most.

- **No personal data, ever.** Never write e-mail addresses, real names, hostnames, real IPs, real home folders, keys,
  tokens, passwords, session transcripts, or the user's prompts and notes into a tracked file, a comment, a test, a
  fixture or a commit message. Use placeholders: `/home/user`, `example.com`, `203.0.113.7`, made-up keys only in the
  files of `release/fake-secrets.txt`. `scripts/release/leakscan.py` is the check; it runs in `scripts/check.sh`.
- **Comments say why, not who.** No decision or bug ids, no "the user asked on <date>", no links to documents that are
  not in the tree.
- **Build in batches.** Make all the edits of one change, then `scripts/t check -p <crate> --tests`. Run the focused
  tests, and `scripts/check.sh` once per branch.
