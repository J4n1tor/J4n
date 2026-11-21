#!/usr/bin/env bash
# export.sh DEST "message" — lay the public J4n tree into DEST and commit it there, never pushing.
#
# The public repository is a separate git repository with its own history: one commit per export. This repository,
# its history and everything outside release/public.paths stay private. The tree is taken from HEAD (committed
# files only), scanned with leakscan.py (the private deny list included) before and after it is laid in, and
# committed under the public identity. A hit stops the export with nothing committed.
#
#   J4N_PUBLIC_NAME / J4N_PUBLIC_EMAIL   the public author; default: docs/dev/release.env (private)
#
# First time: DEST may be missing or empty; it becomes a repository on branch main with the hooks installed.
# Push yourself afterwards: git -C DEST push origin main
set -euo pipefail
cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.."
src=$PWD
dest=${1:?usage: export.sh DEST "commit message"}
msg=${2:?usage: export.sh DEST "commit message"}

[ -f docs/dev/release.env ] && . docs/dev/release.env
name=${J4N_PUBLIC_NAME:?set J4N_PUBLIC_NAME (or docs/dev/release.env)}
email=${J4N_PUBLIC_EMAIL:?set J4N_PUBLIC_EMAIL (or docs/dev/release.env)}
case "$email" in *noreply*) ;; *) echo "export: the public e-mail must be a noreply address, not $email" >&2; exit 1 ;; esac

if [ -n "$(git status --porcelain -- $(python3 -I scripts/release/leakscan.py --list))" ]; then
  echo "export: public files have uncommitted changes; commit them first (the export takes HEAD)" >&2
  exit 1
fi
echo "== scan of the public paths at HEAD"
python3 -I scripts/release/leakscan.py

mkdir -p "$dest"
dest=$(readlink -f "$dest")
case "$dest" in "$src"|"$src"/*) echo "export: DEST must be outside this repository" >&2; exit 1 ;; esac
if [ ! -d "$dest/.git" ]; then
  git -C "$dest" init -q -b main
fi
git -C "$dest" config user.name "$name"
git -C "$dest" config user.email "$email"

echo "== laying the tree into $dest"
find "$dest" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git archive --format=tar HEAD | tar -x -C "$work"
python3 -I scripts/release/leakscan.py --list > "$work/.public-list"
(cd "$work" && tar -c -T .public-list) | tar -x -C "$dest"

echo "== scan of the laid tree"
python3 -I scripts/release/leakscan.py --tree "$dest"

install -m 0755 scripts/release/hooks/pre-push "$dest/.git/hooks/pre-push"
printf '%s\n' "$src/docs/dev/release-deny.txt" > "$dest/.git/j4n-deny-path"

git -C "$dest" add -A
if git -C "$dest" diff --cached --quiet; then
  echo "export: nothing changed since the last export"
  exit 0
fi
git -C "$dest" commit -q -m "$msg"
git -C "$dest" log -1 --format='export: committed %h as %an <%ae>: %s'
echo "export: review it (git -C $dest show --stat), then push it yourself"
