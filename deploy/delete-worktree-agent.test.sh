#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
AGENT=$ROOT/deploy/DELETE_WORKTREE.sh
MANAGER=$ROOT/deploy/agent-worktree.sh
TEMP=$(mktemp -d)
TEMP=$(cd -- "$TEMP" && pwd -P)
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf 'delete-worktree-agent.test: %s\n' "$*" >&2; exit 1; }
[[ -x $AGENT ]] || fail 'deploy/DELETE_WORKTREE.sh must be executable'
[[ -x $MANAGER ]] || fail 'deploy/agent-worktree.sh must be executable'
/bin/bash -n "$AGENT" || fail 'deploy/DELETE_WORKTREE.sh does not parse in macOS bash 3.2'

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
mkdir -p "$TEMP/home" "$TEMP/wt" "$TEMP/state"
export HOME=$TEMP/home
unset GITHUB_TOKEN

git_test() {
  git -c user.name='DELETE_WORKTREE Tests' -c user.email=delete-worktree@example.invalid \
    -c commit.gpgsign=false "$@"
}

ORIGIN=$TEMP/origin.git
git init --quiet --bare "$ORIGIN"
git --git-dir="$ORIGIN" symbolic-ref HEAD refs/heads/master
PRIMARY=$TEMP/primary
git_test clone --quiet "$ORIGIN" "$PRIMARY"
git_test -C "$PRIMARY" symbolic-ref HEAD refs/heads/master
git_test -C "$PRIMARY" config user.name 'DELETE_WORKTREE Tests'
git_test -C "$PRIMARY" config user.email delete-worktree@example.invalid
mkdir -p "$PRIMARY/deploy"
cp "$AGENT" "$PRIMARY/deploy/DELETE_WORKTREE.sh"
cp "$MANAGER" "$PRIMARY/deploy/agent-worktree.sh"
chmod +x "$PRIMARY/deploy/DELETE_WORKTREE.sh" "$PRIMARY/deploy/agent-worktree.sh"
printf 'fixture\n' >"$PRIMARY/fixture.txt"
printf '/target/\n' >"$PRIMARY/.gitignore"
git_test -C "$PRIMARY" add deploy/DELETE_WORKTREE.sh deploy/agent-worktree.sh fixture.txt .gitignore
git_test -C "$PRIMARY" commit --quiet -m fixture
git_test -C "$PRIMARY" push --quiet -u origin master

ACTIVE_PATHS=$TEMP/active-paths
: >"$ACTIVE_PATHS"

run_manager() {
  AGENT_WORKTREE_ROOT="$TEMP/wt" /bin/bash "$PRIMARY/deploy/agent-worktree.sh" "$@"
}

run_agent() {
  (
    cd /
    DELETE_WORKTREE_ACTIVE_PATHS_FILE="$ACTIVE_PATHS" \
      /bin/bash "$PRIMARY/deploy/DELETE_WORKTREE.sh" "$@" \
      --repo "$PRIMARY" --state-dir "$TEMP/state" --settle-seconds 0
  )
}

expect_failure() {
  local description=$1
  shift
  if "$@" >"$TEMP/unexpected-success.out" 2>&1; then
    fail "$description unexpectedly succeeded"
  fi
}

plist=$(run_agent render-plist)
for contract in \
  '<string>sale.apitoken.DELETE_WORKTREE</string>' \
  '<key>RunAtLoad</key>' \
  '<key>KeepAlive</key>' \
  '<string>daemon</string>' \
  "<string>$TEMP/state/runtime/DELETE_WORKTREE.sh</string>" \
  "<string>$PRIMARY</string>" \
  "<string>$TEMP/state</string>"; do
  grep -Fq "$contract" <<<"$plist" || fail "launchd plist lost contract: $contract"
done

if [[ $(uname -s) == Darwin ]]; then
  run_agent install --dry-run >"$TEMP/install-dry-run.plist"
  [[ ! -e $TEMP/state/runtime ]] \
    || fail 'install --dry-run created the runtime directory or executable'
  grep -Fq "<string>$TEMP/state/runtime/DELETE_WORKTREE.sh</string>" \
    "$TEMP/install-dry-run.plist" \
    || fail 'install --dry-run did not render the stable runtime executable path'
fi

eligible=$(run_manager create feat/eligible eligible)
git_test -C "$eligible" commit --quiet --allow-empty -m eligible
git_test -C "$eligible" push --quiet origin HEAD:master
git_test -C "$PRIMARY" fetch --quiet origin
run_agent once
[[ -d $eligible ]] || fail 'agent deleted a worktree on its first observation'
run_agent once
[[ ! -e $eligible ]] || fail 'agent retained a stable clean merged worktree'
git -C "$PRIMARY" show-ref --verify --quiet refs/heads/feat/eligible \
  && fail 'agent retained the finished worktree branch'

active=$(run_manager create feat/active active)
git_test -C "$active" commit --quiet --allow-empty -m active
git_test -C "$active" push --quiet origin HEAD:master
git_test -C "$PRIMARY" fetch --quiet origin
run_agent once
printf '%s/target/debug/artifact\n' "$active" >"$ACTIVE_PATHS"
run_agent once
[[ -d $active ]] || fail 'agent deleted a worktree with an open descendant path'
: >"$ACTIVE_PATHS"
run_agent once
[[ -d $active ]] || fail 'active protection did not reset the stability observation'
run_agent once
[[ ! -e $active ]] || fail 'agent did not clean a worktree after it became inactive and stable'

dirty=$(run_manager create feat/dirty dirty)
git_test -C "$dirty" commit --quiet --allow-empty -m dirty
git_test -C "$dirty" push --quiet origin HEAD:master
printf 'untracked\n' >"$dirty/untracked.txt"
unmerged=$(run_manager create feat/unmerged unmerged)
git_test -C "$unmerged" commit --quiet --allow-empty -m unmerged
locked=$(run_manager create feat/locked locked)
git_test -C "$locked" commit --quiet --allow-empty -m locked
git_test -C "$locked" push --quiet origin HEAD:master
git_test -C "$PRIMARY" worktree lock "$locked" --reason delete-worktree-test
detached=$TEMP/detached
git_test -C "$PRIMARY" worktree add --quiet --detach "$detached" origin/master
run_agent once
run_agent once
[[ -d $dirty && -d $unmerged && -d $locked && -d $detached ]] \
  || fail 'agent removed a dirty, unmerged, locked, or detached worktree'

CLONE=$TEMP/merged-clone
git_test clone --quiet "$ORIGIN" "$CLONE"
git_test -C "$CLONE" config user.name 'DELETE_WORKTREE Tests'
git_test -C "$CLONE" config user.email delete-worktree@example.invalid
run_agent register-clone "$CLONE"
run_agent once
[[ -d $CLONE ]] || fail 'agent deleted a registered clone on its first observation'
run_agent once
[[ ! -e $CLONE ]] || fail 'agent retained a registered clone whose refs were all in master'
! grep -Fqx "$CLONE" "$TEMP/state/clones" \
  || fail 'agent retained the registration of a deleted clone'

UNIQUE_CLONE=$TEMP/unique-clone
git_test clone --quiet "$ORIGIN" "$UNIQUE_CLONE"
git_test -C "$UNIQUE_CLONE" config user.name 'DELETE_WORKTREE Tests'
git_test -C "$UNIQUE_CLONE" config user.email delete-worktree@example.invalid
git_test -C "$UNIQUE_CLONE" switch --quiet -c unique-local
git_test -C "$UNIQUE_CLONE" commit --quiet --allow-empty -m unique
run_agent register-clone "$UNIQUE_CLONE"
run_agent once
run_agent once
[[ -d $UNIQUE_CLONE ]] || fail 'agent deleted a clone with a unique local branch commit'

STASH_CLONE=$TEMP/stash-clone
git_test clone --quiet "$ORIGIN" "$STASH_CLONE"
git_test -C "$STASH_CLONE" config user.name 'DELETE_WORKTREE Tests'
git_test -C "$STASH_CLONE" config user.email delete-worktree@example.invalid
printf 'changed\n' >>"$STASH_CLONE/fixture.txt"
git_test -C "$STASH_CLONE" stash push --quiet
run_agent register-clone "$STASH_CLONE"
run_agent once
run_agent once
[[ -d $STASH_CLONE ]] || fail 'agent deleted a clone with a stash'

TAG_CLONE=$TEMP/local-tag-clone
git_test clone --quiet "$ORIGIN" "$TAG_CLONE"
git_test -C "$TAG_CLONE" tag local-only-tag
run_agent register-clone "$TAG_CLONE"
run_agent once
run_agent once
[[ -d $TAG_CLONE ]] || fail 'agent deleted a clone with a local-only tag ref'
run_agent unregister-clone "$TAG_CLONE"

OTHER_ORIGIN=$TEMP/other-origin.git
git init --quiet --bare "$OTHER_ORIGIN"
git --git-dir="$OTHER_ORIGIN" symbolic-ref HEAD refs/heads/master
OTHER=$TEMP/other-clone
git_test clone --quiet "$OTHER_ORIGIN" "$OTHER"
expect_failure 'different-origin clone registration' run_agent register-clone "$OTHER"

IGNORED_CLONE=$TEMP/ignored-clone
git_test clone --quiet "$ORIGIN" "$IGNORED_CLONE"
mkdir -p "$IGNORED_CLONE/target/debug"
printf 'ignored but potentially valuable\n' >"$IGNORED_CLONE/target/debug/artifact"
expect_failure 'implicit ignored-file deletion consent' run_agent register-clone "$IGNORED_CLONE"
run_agent register-clone "$IGNORED_CLONE" --allow-ignored
run_agent unregister-clone "$IGNORED_CLONE"
[[ -f $IGNORED_CLONE/target/debug/artifact ]] \
  || fail 'ignored-file registration safety test lost clone data'

run_agent unregister-clone "$UNIQUE_CLONE"
! grep -Fqx "$UNIQUE_CLONE" "$TEMP/state/clones" \
  || fail 'unregister-clone retained the selected path'
[[ -d $UNIQUE_CLONE ]] || fail 'unregister-clone deleted the clone'

status=$(run_agent status)
grep -Fq $'label\tsale.apitoken.DELETE_WORKTREE' <<<"$status" \
  || fail 'status omitted the launch agent label'
grep -Fq $'registered_clones\t1' <<<"$status" \
  || fail 'status reported the wrong registered clone count'

if find "$TEMP/state" -maxdepth 1 -type d -name run.lock | grep -q .; then
  fail 'agent left its run lock behind'
fi

printf 'delete-worktree-agent.test: ok\n'
