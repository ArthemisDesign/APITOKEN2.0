#!/usr/bin/env bash
# Behaviour of the interrupted-merge recovery path, on real repositories in a scratch directory.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RECOVER=$ROOT/deploy/agent-merge-recover.sh
TEMP=$(mktemp -d)
TEMP=$(cd -- "$TEMP" && pwd -P)
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf 'agent-merge-recover.test: %s\n' "$*" >&2; exit 1; }
[[ -x $RECOVER ]] || fail 'deploy/agent-merge-recover.sh must be executable'
bash -n "$RECOVER" || fail 'deploy/agent-merge-recover.sh does not parse'

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export HOME=$TEMP/home
mkdir -p "$HOME"

git_test() {
  git -c user.name='Recover Tests' -c user.email=recover-tests@example.invalid \
    -c commit.gpgsign=false "$@"
}

# A repository whose master moved under a branch that touched the same line — the exact shape
# agent-merge.sh leaves behind when its rebase stops.
REPO=$TEMP/repo
git_test init --quiet -b master "$REPO"
printf 'base\n' > "$REPO/file.txt"
git_test -C "$REPO" add file.txt
git_test -C "$REPO" commit --quiet -m base
git_test -C "$REPO" branch task
printf 'master side\n' > "$REPO/file.txt"
git_test -C "$REPO" commit --quiet -am 'master moves'
git_test -C "$REPO" checkout --quiet task
printf 'task side\n' > "$REPO/file.txt"
git_test -C "$REPO" commit --quiet -am 'task moves'
TASK_TIP=$(git_test -C "$REPO" rev-parse HEAD)

# --- nothing in progress -------------------------------------------------------------------
output=$(cd "$REPO" && "$RECOVER" --allow-primary-tree 2>&1) \
  || fail 'a clean tree must exit 0'
[[ $output == *'nothing to recover'* ]] || fail "clean tree reported: $output"

# --- a conflicted rebase is reported, never silently resolved ------------------------------
set +e
(cd "$REPO" && git_test rebase master >/dev/null 2>&1)
set -e
[[ -n $(git_test -C "$REPO" diff --name-only --diff-filter=U) ]] \
  || fail 'the fixture failed to produce a conflicted rebase'

set +e
output=$(cd "$REPO" && "$RECOVER" --allow-primary-tree 2>&1)
status=$?
set -e
(( status == 2 )) || fail "an unresolved rebase must exit 2, got $status"
[[ $output == *'unresolved path'* ]] || fail "state report missing: $output"

# --- continue refuses while a path is still conflicted --------------------------------------
set +e
output=$(cd "$REPO" && "$RECOVER" --continue --allow-primary-tree 2>&1)
status=$?
set -e
(( status != 0 )) || fail 'continue must refuse while conflicts remain'
[[ $output == *'still conflicted'* ]] || fail "refusal message missing: $output"
[[ -n $(git_test -C "$REPO" diff --name-only --diff-filter=U) ]] \
  || fail 'a refused continue must leave the conflict untouched'

# --- abort restores every original commit ---------------------------------------------------
(cd "$REPO" && "$RECOVER" --abort --allow-primary-tree >/dev/null 2>&1) \
  || fail 'abort must succeed on a conflicted rebase'
[[ $(git_test -C "$REPO" rev-parse HEAD) == "$TASK_TIP" ]] \
  || fail 'abort must restore the branch tip exactly'
[[ -z $(git_test -C "$REPO" status --porcelain) ]] || fail 'abort must leave a clean tree'

# --- continue completes a resolved rebase ---------------------------------------------------
set +e
(cd "$REPO" && git_test rebase master >/dev/null 2>&1)
set -e
printf 'resolved\n' > "$REPO/file.txt"
git_test -C "$REPO" add file.txt
(cd "$REPO" && "$RECOVER" --continue --allow-primary-tree >/dev/null 2>&1) \
  || fail 'continue must finish a resolved rebase'
[[ -z $(git_test -C "$REPO" status --porcelain) ]] || fail 'a finished rebase must leave a clean tree'
git_test -C "$REPO" merge-base --is-ancestor master HEAD \
  || fail 'the recovered branch must sit on top of master'
[[ $(cat "$REPO/file.txt") == resolved ]] || fail 'the resolution must survive'

# --- the shared primary tree is protected by default ----------------------------------------
set +e
output=$(cd "$REPO" && "$RECOVER" --abort 2>&1)
status=$?
set -e
(( status != 0 )) || fail 'the primary tree must be refused without --allow-primary-tree'
[[ $output == *'primary working tree'* ]] || fail "primary-tree refusal missing: $output"

printf 'agent-merge-recover.test: ok\n'
