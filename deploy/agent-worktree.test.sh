#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
MANAGER=$ROOT/deploy/agent-worktree.sh
TEMP=$(mktemp -d)
TEMP=$(cd -- "$TEMP" && pwd -P)
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf 'agent-worktree.test: %s\n' "$*" >&2; exit 1; }
[[ -x $MANAGER ]] || fail 'deploy/agent-worktree.sh must be executable'
bash -n "$MANAGER" || fail 'deploy/agent-worktree.sh does not parse'

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
mkdir -p "$TEMP/home" "$TEMP/wt"
export HOME=$TEMP/home
unset GITHUB_TOKEN

git_test() {
  git -c user.name='Worktree Tests' -c user.email=worktree-tests@example.invalid \
    -c commit.gpgsign=false "$@"
}

ORIGIN=$TEMP/origin.git
git init --quiet --bare "$ORIGIN"
git --git-dir="$ORIGIN" symbolic-ref HEAD refs/heads/master
PRIMARY=$TEMP/primary
git_test clone --quiet "$ORIGIN" "$PRIMARY"
git_test -C "$PRIMARY" symbolic-ref HEAD refs/heads/master
git_test -C "$PRIMARY" config user.name 'Worktree Tests'
git_test -C "$PRIMARY" config user.email worktree-tests@example.invalid
mkdir -p "$PRIMARY/deploy"
cp "$MANAGER" "$PRIMARY/deploy/agent-worktree.sh"
printf 'fixture\n' >"$PRIMARY/fixture.txt"
printf '/target/\n' >"$PRIMARY/.gitignore"
git_test -C "$PRIMARY" add deploy/agent-worktree.sh fixture.txt .gitignore
GIT_AUTHOR_DATE='2000-01-01T00:00:00Z' \
  GIT_COMMITTER_DATE='2000-01-01T00:00:00Z' \
  git_test -C "$PRIMARY" commit --quiet -m fixture
git_test -C "$PRIMARY" push --quiet -u origin master
INITIAL_SHA=$(git -C "$PRIMARY" rev-parse HEAD)

run_manager() {
  AGENT_WORKTREE_ROOT="$TEMP/wt" \
    bash "$PRIMARY/deploy/agent-worktree.sh" "$@"
}

expect_failure() {
  local description=$1
  shift
  if "$@" >"$TEMP/unexpected-success.out" 2>&1; then
    fail "$description unexpectedly succeeded"
  fi
}

expect_failure 'protected-branch creation' run_manager create master protected
expect_failure 'invalid worktree name' run_manager create feat/invalid '../escape'
expect_failure 'overflowing grace period' run_manager doctor --grace-hours 9999999999
run_manager help >/dev/null
run_manager gc --grace-hours 000 >/dev/null

dirty=$(run_manager create feat/dirty dirty)
[[ $dirty == "$TEMP/wt/dirty" && -d $dirty ]] \
  || fail 'create did not return the managed worktree path'
[[ $(git -C "$dirty" rev-parse --abbrev-ref HEAD) == feat/dirty ]] \
  || fail 'create checked out the wrong task branch'
admin=$(git -C "$dirty" rev-parse --path-format=absolute --git-dir)
grep -Fxq 'version=2' "$admin/agent-worktree-managed-v1" \
  || fail 'create did not persist managed lifecycle metadata'
grep -Fxq "path=$dirty" "$admin/agent-worktree-managed-v1" \
  || fail 'managed lifecycle metadata lost the canonical path'
grep -Fxq "base=$INITIAL_SHA" "$admin/agent-worktree-managed-v1" \
  || fail 'managed lifecycle metadata lost the exact creation base'
unstarted_report=$(run_manager doctor --grace-hours 0)
printf '%s\n' "$unstarted_report" | grep -Fq $'UNSTARTED           \tfeat/dirty' \
  || fail 'doctor exposed a managed worktree before its first task commit to automatic cleanup'
git_test -C "$dirty" commit --quiet --allow-empty -m 'dirty branch work'
printf 'uncommitted\n' >"$dirty/untracked.txt"

invalid_metadata=$(run_manager create feat/invalid-metadata invalid-metadata)
invalid_admin=$(git -C "$invalid_metadata" rev-parse --path-format=absolute --git-dir)
awk '$0 !~ /^base=/' "$invalid_admin/agent-worktree-managed-v1" \
  >"$invalid_admin/agent-worktree-managed-v1.tmp"
mv "$invalid_admin/agent-worktree-managed-v1.tmp" "$invalid_admin/agent-worktree-managed-v1"
invalid_report=$(run_manager doctor --grace-hours 0)
printf '%s\n' "$invalid_report" | grep -Fq $'INVALID_METADATA    \tfeat/invalid-metadata' \
  || fail 'doctor did not fail closed for incomplete version 2 creation metadata'
run_manager finish "$invalid_metadata"
[[ ! -e $invalid_metadata ]] \
  || fail 'explicit finish did not remain available for manually reviewed metadata'

unmerged=$(run_manager create feat/unmerged unmerged)
git_test -C "$unmerged" commit --quiet --allow-empty -m 'unmerged branch work'
current_report=$(
  cd "$unmerged"
  AGENT_WORKTREE_ROOT="$TEMP/wt" bash deploy/agent-worktree.sh doctor --grace-hours 0
)
printf '%s\n' "$current_report" | grep -Fq $'CURRENT             \tfeat/unmerged' \
  || fail 'doctor did not protect the caller worktree'
expect_failure 'unmerged finish' run_manager finish "$unmerged"
[[ -d $unmerged ]] || fail 'finish removed an unmerged worktree'

finished=$(run_manager create feat/finished finished)
git_test -C "$finished" commit --quiet --allow-empty -m 'finished branch work'
git_test -C "$finished" push --quiet origin HEAD:master
git_test -C "$PRIMARY" fetch --quiet origin

recent_report=$(run_manager doctor --grace-hours 24)
printf '%s\n' "$recent_report" | grep -Fq $'RECENT_MERGED       \tfeat/finished' \
  || fail 'doctor did not protect a recently merged worktree during the grace period'

locked=$(run_manager create feat/locked locked)
git_test -C "$PRIMARY" worktree lock "$locked" --reason lifecycle-test

comp_tree=$TEMP/comp-owner
git_test -C "$PRIMARY" worktree add --quiet -b comp/test "$comp_tree" origin/master

detached=$TEMP/detached
git_test -C "$PRIMARY" worktree add --quiet --detach "$detached" origin/master

expect_failure 'locked finish' run_manager finish "$locked"
expect_failure 'protected finish' run_manager finish "$comp_tree"
expect_failure 'detached finish' run_manager finish "$detached"
expect_failure 'primary finish' run_manager finish "$PRIMARY"

missing=$(run_manager create feat/missing missing)
git_test -C "$missing" commit --quiet --allow-empty -m 'missing unique work'
rm -rf -- "$missing"

git_test -C "$PRIMARY" branch feat/orphan origin/master
git_test -C "$PRIMARY" branch feat/recent-old-tip "$INITIAL_SHA"

branch_grace_report=$(run_manager gc --grace-hours 24 2>&1)
printf '%s\n' "$branch_grace_report" | grep -Fq 'would delete old merged local branch feat/recent-old-tip' \
  && fail 'gc used old commit time instead of recent branch-ref activity for the grace period'

dry_run=$(run_manager gc --grace-hours 0 2>&1)
printf '%s\n' "$dry_run" | grep -Fq "would remove old clean merged worktree $finished" \
  || fail 'gc dry-run omitted an old clean merged worktree'
printf '%s\n' "$dry_run" | grep -Fq "would prune missing registration $missing" \
  || fail 'gc dry-run omitted a missing registration'
printf '%s\n' "$dry_run" | grep -Fq 'would delete old merged local branch feat/orphan' \
  || fail 'gc dry-run omitted an unowned merged local branch'
[[ -d $finished ]] || fail 'gc dry-run mutated a worktree'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/orphan \
  || fail 'gc dry-run mutated a branch ref'

run_manager gc --apply --grace-hours 0
[[ ! -e $finished ]] || fail 'gc --apply retained an eligible merged worktree'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/finished \
  && fail 'gc --apply retained the removed worktree branch'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/orphan \
  && fail 'gc --apply retained an eligible orphan merged branch'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/recent-old-tip \
  && fail 'gc --apply retained an eligible recent-ref branch when grace was explicitly disabled'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/missing \
  || fail 'gc deleted an unmerged branch whose directory disappeared'
if git -C "$PRIMARY" worktree list --porcelain | grep -Fqx "worktree $missing"; then
  fail 'gc retained a missing unlocked worktree registration'
fi
[[ -d $dirty && -d $unmerged && -d $locked && -d $comp_tree && -d $detached ]] \
  || fail 'gc removed a dirty, unmerged, locked, protected, or detached worktree'

report=$(run_manager doctor --grace-hours 0)
printf '%s\n' "$report" | grep -Fq $'DIRTY               \tfeat/dirty' \
  || fail 'doctor did not identify a dirty worktree'
printf '%s\n' "$report" | grep -Fq $'UNMERGED            \tfeat/unmerged' \
  || fail 'doctor did not identify an unmerged worktree'
printf '%s\n' "$report" | grep -Fq $'LOCKED              \tfeat/locked' \
  || fail 'doctor did not identify an explicitly locked worktree'
printf '%s\n' "$report" | grep -Fq $'PROTECTED_BRANCH    \tcomp/test' \
  || fail 'doctor did not protect a comp/* owner branch'
printf '%s\n' "$report" | grep -Fq $'DETACHED            \tdetached' \
  || fail 'doctor did not preserve a detached worktree for manual review'

read_only=$(run_manager create docs/read-only read-only)
mkdir -p "$read_only/target/debug"
printf 'disposable build output\n' >"$read_only/target/debug/artifact"
[[ -z $(git -C "$read_only" status --porcelain) ]] \
  || fail 'fixture build output was not ignored like a real Cargo target directory'
run_manager finish --dry-run "$read_only"
[[ -d $read_only ]] || fail 'finish --dry-run mutated its selected worktree'
run_manager finish "$read_only"
[[ ! -e $read_only ]] || fail 'finish retained a clean merged read-only worktree'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/docs/read-only \
  && fail 'finish retained the clean merged read-only branch'
[[ $(git -C "$PRIMARY" rev-parse HEAD) == $(git -C "$PRIMARY" rev-parse origin/master) ]] \
  || fail 'finish did not fast-forward a clean primary master'

expect_failure 'dirty finish' run_manager finish "$dirty"
[[ -d $dirty ]] || fail 'finish removed a dirty worktree'

if find "$PRIMARY/.git" -maxdepth 1 -type d -name 'agent-worktree-manager.lock' | grep -q .; then
  fail 'a lifecycle command left its manager lock behind'
fi

printf 'agent-worktree.test: ok\n'
