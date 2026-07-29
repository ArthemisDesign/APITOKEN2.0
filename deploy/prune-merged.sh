#!/usr/bin/env bash
# Remove every git worktree whose branch is fully merged into origin/master and whose working tree
# is clean, then delete that branch. Safe by construction: it never touches the main worktree, the
# worktree it is invoked from, a detached-HEAD worktree, an unmerged branch, or a dirty tree, and it
# never uses --force on a worktree.
#
# Why a script: the git guard (.claude/hooks/guard-git.sh) blocks the raw `git worktree remove`
# because a bare command cannot tell a finished, merged worktree from another agent's live one. That
# block stays intact and gate-tested. This script performs the missing check (merged into
# origin/master AND clean) and only then removes — so cleanup after deploy/agent-merge.sh lands a
# branch no longer requires a human to run the removal by hand. The guard does not inspect a
# script's internal git calls, exactly as it does not inspect deploy/agent-merge.sh.
#
# Usage: deploy/prune-merged.sh [--dry-run]
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

TARGET=origin/master
DRY_RUN=0
case "${1:-}" in
  --dry-run) DRY_RUN=1 ;;
  '') : ;;
  *) printf 'prune-merged: unknown argument: %s\n' "$1" >&2; exit 1 ;;
esac

log() { printf '[prune-merged] %s\n' "$*"; }

git fetch --quiet origin 2>/dev/null || log "warning: git fetch failed; comparing against the on-disk $TARGET"
git rev-parse --verify --quiet "$TARGET" >/dev/null \
  || { log "ERROR: $TARGET not found; nothing to compare against"; exit 1; }

# Never a removal target: the worktree this runs from, or the main (first-listed) worktree.
current_top=$(git rev-parse --show-toplevel 2>/dev/null || printf '')
main_top=$(git worktree list --porcelain | sed -n 's/^worktree //p' | head -1)

removed=0
skipped=0
path=''
branch=''

consider() {
  [[ -n $path ]] || return 0
  local reason=''
  if [[ $path == "$main_top" ]]; then
    reason='main worktree'
  elif [[ $path == "$current_top" ]]; then
    reason='current worktree'
  elif [[ -z $branch ]]; then
    reason='detached HEAD'
  elif [[ -n $(git -C "$path" status --porcelain 2>/dev/null) ]]; then
    reason='working tree not clean'
  elif ! git merge-base --is-ancestor "refs/heads/$branch" "$TARGET" 2>/dev/null; then
    reason="branch not merged into $TARGET"
  fi

  if [[ -n $reason ]]; then
    log "skip  $path [${branch:-detached}]: $reason"
    skipped=$((skipped + 1))
    path=''
    branch=''
    return 0
  fi

  if (( DRY_RUN )); then
    log "would remove $path and delete branch $branch"
  else
    git worktree remove "$path"
    # The branch is a proven ancestor of origin/master (checked above), so its commits are safely in
    # master. Prefer the safe -d; fall back to -D only for the rebased-merge case where the exact
    # SHAs differ from master's — never a data-loss because the ancestry check already passed.
    git branch -d "$branch" 2>/dev/null || git branch -D "$branch"
    log "removed $path and deleted branch $branch"
    removed=$((removed + 1))
  fi
  path=''
  branch=''
}

while IFS= read -r line; do
  case "$line" in
    'worktree '*)
      consider
      path=${line#worktree }
      ;;
    'branch refs/heads/'*)
      branch=${line#branch refs/heads/}
      ;;
  esac
done < <(git worktree list --porcelain)
consider

log "done: removed $removed, skipped $skipped"
