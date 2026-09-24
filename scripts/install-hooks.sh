#!/usr/bin/env bash
# Install the lefthook git hooks — only from a primary checkout.
#
# In a linked worktree (`git worktree add`), the hooks directory belongs to the
# shared .git of every worktree, so `lefthook install` there would change the
# hooks of all of them. Refuse instead of doing that silently. Hooks are never
# installed implicitly: `pnpm install` does not run this (there is no `prepare`
# script, and lefthook's own postinstall is disabled in pnpm-workspace.yaml).
set -euo pipefail

git_dir=$(git rev-parse --path-format=absolute --git-dir)
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
if [ "$git_dir" != "$common_dir" ]; then
  echo "error: this is a linked git worktree; its hooks live in the shared '$common_dir'." >&2
  echo "Refusing to install lefthook hooks there. Run 'pnpm run hooks' from the primary checkout instead." >&2
  exit 1
fi

exec lefthook install "$@"
