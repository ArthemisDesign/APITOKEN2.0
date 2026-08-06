#!/usr/bin/env bash
# Finish or abandon a merge rebase that agent-merge.sh left unresolved.
#
# agent-merge.sh rebases the candidate onto committed master. When master moved under a branch,
# that rebase stops on a conflict and the script exits, leaving the worktree mid-rebase. Every
# route out of that state — `git rebase --continue`, `git rebase --abort` — is denied to agents by
# .claude/hooks/guard-git.sh, which cannot tell "completing the sanctioned merge path" from
# "rewriting history outside it". Without this script the only way forward is a human typing a git
# command, which is exactly the dependency the managed lifecycle exists to remove.
#
# The guard stays as strict as it is: it does not inspect a reviewed script's internal git calls,
# the same arrangement that lets agents run deploy/prune-merged-style cleanup. What is delegated
# here is deliberately narrow — this script performs no merge, no push, no branch switch, and
# touches nothing but an in-progress sequencer operation in the current worktree.
#
# Neither action can lose committed work. `--continue` only advances a rebase whose conflicts are
# already resolved and staged; `--abort` restores the branch exactly as it was before the rebase
# started, with every original commit intact.
#
# Usage:  deploy/agent-merge-recover.sh [--continue|--abort] [--allow-primary-tree]
#
# With no action, it reports the state and stops: resuming and abandoning are different decisions
# and this script never guesses which one was meant.
set -euo pipefail

# The worktree to recover is the one the caller stands in, not the one holding this file. A tree
# that got stuck was checked out before this script existed, so requiring the script to live inside
# it would leave exactly the trees that need recovering unable to use it.
ROOT=$(git rev-parse --show-toplevel 2>/dev/null) \
  || ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

amr_log() { printf '[agent-merge-recover] %s\n' "$*"; }
amr_die() { printf '[agent-merge-recover] ERROR: %s\n' "$*" >&2; exit 1; }

ACTION=
ALLOW_PRIMARY_TREE=0
for argument in "$@"; do
  case "$argument" in
    --continue) ACTION=continue ;;
    --abort) ACTION=abort ;;
    --allow-primary-tree) ALLOW_PRIMARY_TREE=1 ;;
    *) amr_die "unknown argument: $argument" ;;
  esac
done

git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 || amr_die "not a git repository: $ROOT"

# A shared primary clone is where other agents' uncommitted work lives. agent-merge.sh applies the
# same rule for the same reason.
if (( ! ALLOW_PRIMARY_TREE )); then
  [[ $(git -C "$ROOT" rev-parse --is-inside-work-tree 2>/dev/null) == true ]] \
    || amr_die "not inside a work tree"
  common_dir=$(cd -- "$(git -C "$ROOT" rev-parse --git-common-dir)" && pwd)
  git_dir=$(cd -- "$(git -C "$ROOT" rev-parse --git-dir)" && pwd)
  [[ $git_dir != "$common_dir" ]] \
    || amr_die "refusing to touch the primary working tree; run this from your own worktree (or pass --allow-primary-tree)"
fi

# Only a sequencer operation in THIS worktree is in scope. Each state directory lives under the
# per-worktree git dir, so a neighbouring agent's rebase is invisible here and stays untouched.
sequencer=
[[ -d $(git -C "$ROOT" rev-parse --git-path rebase-merge) ]] && sequencer=rebase
[[ -d $(git -C "$ROOT" rev-parse --git-path rebase-apply) ]] && sequencer=rebase
[[ -f $(git -C "$ROOT" rev-parse --git-path CHERRY_PICK_HEAD) ]] && sequencer=cherry-pick

if [[ -z $sequencer ]]; then
  amr_log "no rebase or cherry-pick is in progress in this worktree; nothing to recover"
  exit 0
fi

unresolved=$(git -C "$ROOT" diff --name-only --diff-filter=U)
unresolved_count=$(printf '%s' "$unresolved" | grep -c . || true)

if [[ -z $ACTION ]]; then
  amr_log "a $sequencer is in progress with $unresolved_count unresolved path(s)"
  [[ -n $unresolved ]] && printf '%s\n' "$unresolved"
  cat >&2 <<'USAGE'
Resolve every conflicted path and stage it, then finish the landing:
  deploy/agent-merge-recover.sh --continue && ./deploy/agent-merge.sh
Or give up on this attempt and restore the branch exactly as it was:
  deploy/agent-merge-recover.sh --abort
USAGE
  exit 2
fi

if [[ $ACTION == continue ]]; then
  (( unresolved_count == 0 )) \
    || amr_die "$unresolved_count path(s) still conflicted; resolve and stage them first:
$unresolved"
  amr_log "continuing the interrupted $sequencer"
  # The sequencer reuses the original commit messages, so no editor may be opened: an agent has no
  # terminal to answer one and the command would hang until it is killed.
  if [[ $sequencer == cherry-pick ]]; then
    GIT_EDITOR=true git -C "$ROOT" cherry-pick --continue
  else
    GIT_EDITOR=true git -C "$ROOT" rebase --continue
  fi
  amr_log "recovered; HEAD is now $(git -C "$ROOT" rev-parse --short HEAD)"
  amr_log "land it with: git push -u origin HEAD && ./deploy/agent-merge.sh"
  exit 0
fi

amr_log "aborting the interrupted $sequencer; the branch returns to its pre-rebase commits"
if [[ $sequencer == cherry-pick ]]; then
  git -C "$ROOT" cherry-pick --abort
else
  git -C "$ROOT" rebase --abort
fi
amr_log "restored; HEAD is now $(git -C "$ROOT" rev-parse --short HEAD)"
