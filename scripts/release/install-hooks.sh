#!/usr/bin/env bash
# install-hooks.sh — use scripts/release/hooks for this repository and all its worktrees: pre-commit scans staged
# public files, pre-push refuses non-noreply authors and a tree that fails the leak scan.
set -euo pipefail
cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.."
git config core.hooksPath scripts/release/hooks
echo "hooks: core.hooksPath = scripts/release/hooks (pre-commit, pre-push)"
