---
name: release-chores
when: changelog, release, version, bump, tag, tagged, commit, contributing, patch
description: Read before a task that asks for a changelog line, a version bump, a tag or a commit in the project's own format. It says how to do each chore exactly as the project and the task write it.
cost: 220
---
# Do the release chores as written

- Read `CONTRIBUTING.md` (or the README's part on contributing), `CHANGELOG.md`, and `git log --oneline` with `git tag` first. The project's own rule and the task's own words beat habit.
- Do the steps the task lists, in the form the project uses, and nothing more: no new files, no reformatting, no extra headings.
- Changelog: add one bullet in the project's bullet format, with the ticket id, in the place the task names. If the task says "under Unreleased", it stays under `## Unreleased`: do not rename that heading, do not open a new version heading, do not move the bullet. Leave every older section as it was.
- Version: grep for the old number and change it in every file that holds it, and nothing else in those files.
- Commit: one commit unless told otherwise, holding the fix, its test, the changelog line and the version bump. Follow the message format; read `.git/hooks/commit-msg` if there is one. Tag only what the task or the guide asks for, on that commit.
- Leave the tree clean: `git status --short` is empty (remove caches you made). Say which steps you did.
