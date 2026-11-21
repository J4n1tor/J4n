#!/usr/bin/env bash
# ship.sh [patch|minor|major] [--push] — make the next numbered release of J4n.
#
# Versions follow semver; below 1.0.0 every release is a beta (the number says so; GitHub gets a full release, so the
# latest-release link install.sh reads finds it). The first ship is the
# version Cargo.toml already holds; each later one bumps the last number (0.1.0, 0.1.1, 0.1.2 …), as many times a day
# as you like. `minor` starts the next stage (0.2.0), `major` is 1.0.0 and later.
#
# The count is never lost: the last version is the highest of the `release/v*` tags of this repository and the `v*`
# tags of the public copy and of its remote, so a re-cloned public copy or a fresh checkout still counts on.
#
# What it does, stopping at the first problem:
#   1. this checkout is on main with no uncommitted changes in tracked files;
#   2. works out the next version; if it differs from Cargo.toml, writes it into Cargo.toml and Cargo.lock and commits;
#   3. tags the private commit `release/vX.Y.Z` (a local tag; this repository has no remote);
#   4. exports the public tree (export.sh: leak scans before and after) with the message "J4n vX.Y.Z";
#   5. tags the public commit vX.Y.Z;
#   6. with --push: pushes the public main and the tag, which makes the release workflow build the binaries.
#
#   J4N_PUBLIC_DIR   the public copy (a separate clone, outside this repository); default: docs/dev/release.env
#
# Run it after a green gate (scripts/check.sh) on the commit you ship.
set -euo pipefail
cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.."
[ -f docs/dev/release.env ] && . docs/dev/release.env
pub=${J4N_PUBLIC_DIR:?set J4N_PUBLIC_DIR (or docs/dev/release.env)}
part=patch
push=no
for a in "$@"; do
  case "$a" in
    patch|minor|major) part=$a ;;
    --push) push=yes ;;
    *) echo "usage: ship.sh [patch|minor|major] [--push]" >&2; exit 2 ;;
  esac
done

[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "ship: not on main" >&2; exit 1; }
git diff --quiet HEAD || { echo "ship: uncommitted changes in tracked files; commit them first" >&2; exit 1; }

current=$(sed -n 's/^version = "\([0-9.]*\)"$/\1/p' Cargo.toml | head -1)
[ -n "$current" ] || { echo "ship: no workspace version in Cargo.toml" >&2; exit 1; }
tags() {
  git tag -l 'release/v*' | sed 's|^release/||'
  if [ -d "$pub/.git" ]; then
    git -C "$pub" tag -l 'v*'
    if git -C "$pub" remote get-url origin >/dev/null 2>&1; then
      git -C "$pub" ls-remote --tags --refs origin 'v*' | sed 's|.*refs/tags/||' || { echo "ship: cannot read the tags of the public remote" >&2; exit 1; }
    fi
  fi
}
all=$(tags)
last=$(printf '%s\n' "$all" | sed -n 's/^v\([0-9]*\.[0-9]*\.[0-9]*\)$/\1/p' | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)

if [ -z "$last" ]; then
  next=$current   # the first ship is the version Cargo.toml holds
else
  IFS=. read -r ma mi pa <<<"$last"
  case "$part" in
    patch) next="$ma.$mi.$((pa + 1))" ;;
    minor) next="$ma.$((mi + 1)).0" ;;
    major) next="$((ma + 1)).0.0" ;;
  esac
fi
echo "ship: last released ${last:-none}, Cargo.toml $current, next v$next"
if git rev-parse -q --verify "refs/tags/release/v$next" >/dev/null; then
  echo "ship: tag release/v$next already exists here" >&2
  exit 1
fi

if [ "$next" != "$current" ]; then
  python3 -I - "$current" "$next" <<'EOF'
import re, sys
old, new = sys.argv[1], sys.argv[2]
with open("Cargo.toml") as f:
    s = f.read()
s, n = re.subn(rf'(?m)^version = "{re.escape(old)}"$', f'version = "{new}"', s, count=1)
assert n == 1, "Cargo.toml: workspace version not found"
with open("Cargo.toml", "w") as f:
    f.write(s)
# the workspace's own crates: a lock entry with no `source` line
with open("Cargo.lock") as f:
    s = f.read()
s, n = re.subn(rf'(name = "j4n[^"]*"\n)version = "{re.escape(old)}"\n(?!source)', rf'\1version = "{new}"\n', s)
assert n > 0, "Cargo.lock: no workspace crate found"
with open("Cargo.lock", "w") as f:
    f.write(s)
print(f"ship: version {old} -> {new} in Cargo.toml and {n} Cargo.lock entries")
EOF
  git add Cargo.toml Cargo.lock
  git commit -q -m "J4n: version $next

Models-used: none (scripts/release/ship.sh)"
fi
git tag "release/v$next"

scripts/release/export.sh "$pub" "J4n v$next"
git -C "$pub" tag -a "v$next" -m "J4n v$next"
echo "ship: v$next tagged here and in $pub"

if [ "$push" = yes ]; then
  git -C "$pub" push origin main
  git -C "$pub" push origin "v$next"
  echo "ship: pushed; the release workflow builds the binaries (gh run list)"
else
  echo "ship: not pushed. To publish: git -C $pub push origin main && git -C $pub push origin v$next"
fi
