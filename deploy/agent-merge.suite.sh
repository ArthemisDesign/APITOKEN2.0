#!/usr/bin/env bash
# Hermetic tests for the serialized merge path and the agent git guard.
#
# Everything runs against throwaway repositories under a temporary directory: no network, no
# pnpm/cargo, no access to the real merge lock, and nothing outside $TEMP is written. The gate and
# the GitHub status and trusted-validation operations are injected through AGENT_MERGE_*_CMD.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

MERGE=$ROOT/deploy/agent-merge.sh
GUARD=$ROOT/.claude/hooks/guard-git.sh
WATCHDOG_LIB=$ROOT/deploy/watchdog-lib.sh
SCCACHE_WRAPPER=$ROOT/deploy/sccache-cargo.sh
WORKTREE_MANAGER=$ROOT/deploy/agent-worktree.sh
CHANGE_PLAN=$ROOT/deploy/change-plan.sh
REPOSITORY_INVARIANTS=$ROOT/deploy/repository-invariants.py
DOCS_CHECK=$ROOT/deploy/docs-check.sh
DOCS_CHECK_IMPL=$ROOT/deploy/docs-check.py
CONTROL_API_ACCEPTANCE=$ROOT/tests/control_api_engine_client_acceptance.sh
CONTROL_API_CLIENT=$ROOT/packages/engine-client/acceptance/control-api.mjs
ROUTER_REPLAY=$ROOT/tests/router_engine_replay.py
ROUTER_REPLAY_MOCK=$ROOT/tests/router_engine_replay_mock.py
ROUTER_REPLAY_SEMANTICS=$ROOT/tests/router_engine_replay_semantics.test.py
ROUTER_REPLAY_FIXTURE=$ROOT/tests/fixtures/router-engine-replay-v1.json

[[ -x $MERGE ]] || wd_die 'deploy/agent-merge.sh must be executable'
[[ -x $GUARD ]] || wd_die '.claude/hooks/guard-git.sh must be executable'
[[ -f $WATCHDOG_LIB ]] || wd_die 'deploy/watchdog-lib.sh is required'
[[ -x $SCCACHE_WRAPPER ]] || wd_die 'deploy/sccache-cargo.sh must be executable'
[[ -x $WORKTREE_MANAGER ]] || wd_die 'deploy/agent-worktree.sh must be executable'
bash -n "$MERGE" || wd_die 'deploy/agent-merge.sh does not parse'
bash -n "$GUARD" || wd_die '.claude/hooks/guard-git.sh does not parse'
bash -n "$WORKTREE_MANAGER" || wd_die 'deploy/agent-worktree.sh does not parse'

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

# Neutralize the developer's own git configuration. Without this the credential-fallback scenario
# would consult the real credential helper and pull a contributor's live GitHub token into a test
# fixture, and unrelated global settings could change what the scenarios exercise. HOME is redirected
# too, so the isolation holds on a git older than the 2.32 that introduced GIT_CONFIG_GLOBAL.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
mkdir -p "$TEMP/home"
export HOME=$TEMP/home
# A token inherited from the surrounding environment (a CI runner, a developer's profile) must never
# decide what these scenarios exercise.
unset GITHUB_TOKEN
git_quiet() { git -c user.name=test -c user.email=test@example.com -c commit.gpgsign=false "$@"; }

# A bare origin plus a primary clone, exactly the topology contributors and agents share.
ORIGIN=$TEMP/origin.git
git init --quiet --bare "$ORIGIN"
git --git-dir="$ORIGIN" symbolic-ref HEAD refs/heads/master
PRIMARY=$TEMP/primary
git_quiet clone --quiet "$ORIGIN" "$PRIMARY"
# The merge rebases with plain `git`, so the identity must live in the fixture repository itself.
# Relying on ambient identity passes on a developer machine, where git auto-detects user@hostname,
# and fatals on a host whose hostname has no domain.
git_quiet -C "$PRIMARY" config user.name 'Merge Path Tests'
git_quiet -C "$PRIMARY" config user.email 'tests@example.invalid'
git_quiet -C "$PRIMARY" symbolic-ref HEAD refs/heads/master
printf 'base\n' >"$PRIMARY/file.txt"
git_quiet -C "$PRIMARY" add file.txt
git_quiet -C "$PRIMARY" commit --quiet -m 'base'
git_quiet -C "$PRIMARY" push --quiet origin master

# The merge script resolves its own repository from its location, so every scenario gets a copy of
# the scripts it may load inside the throwaway tree.
install_scripts() {
  mkdir -p -- "$1/deploy" "$1/.claude/hooks" "$1/tests/fixtures" \
    "$1/packages/engine-client/acceptance"
  cp -- "$MERGE" "$1/deploy/agent-merge.sh"
  cp -- "$GUARD" "$1/.claude/hooks/guard-git.sh"
  cp -- "$WATCHDOG_LIB" "$1/deploy/watchdog-lib.sh"
  cp -- "$SCCACHE_WRAPPER" "$1/deploy/sccache-cargo.sh"
  cp -- "$CHANGE_PLAN" "$1/deploy/change-plan.sh"
  cp -- "$REPOSITORY_INVARIANTS" "$1/deploy/repository-invariants.py"
  cp -- "$DOCS_CHECK" "$1/deploy/docs-check.sh"
  cp -- "$DOCS_CHECK_IMPL" "$1/deploy/docs-check.py"
  cp -- "$CONTROL_API_ACCEPTANCE" "$1/tests/control_api_engine_client_acceptance.sh"
  cp -- "$CONTROL_API_CLIENT" "$1/packages/engine-client/acceptance/control-api.mjs"
  cp -- "$ROUTER_REPLAY" "$1/tests/router_engine_replay.py"
  cp -- "$ROUTER_REPLAY_MOCK" "$1/tests/router_engine_replay_mock.py"
  cp -- "$ROUTER_REPLAY_SEMANTICS" "$1/tests/router_engine_replay_semantics.test.py"
  cp -- "$ROUTER_REPLAY_FIXTURE" "$1/tests/fixtures/router-engine-replay-v1.json"
}
install_scripts "$PRIMARY"
git_quiet -C "$PRIMARY" add deploy/agent-merge.sh deploy/watchdog-lib.sh \
  deploy/sccache-cargo.sh deploy/change-plan.sh deploy/repository-invariants.py \
  deploy/docs-check.sh deploy/docs-check.py tests/control_api_engine_client_acceptance.sh \
  packages/engine-client/acceptance/control-api.mjs tests/router_engine_replay.py \
  tests/router_engine_replay_mock.py tests/router_engine_replay_semantics.test.py \
  tests/fixtures/router-engine-replay-v1.json .claude/hooks/guard-git.sh
git_quiet -C "$PRIMARY" commit --quiet -m 'tooling'
git_quiet -C "$PRIMARY" push --quiet origin master

# Each agent gets its own worktree, which is the whole point of the workflow.
new_agent_worktree() {  # $1 = name, $2 = branch
  local tree=$TEMP/$1
  git_quiet -C "$PRIMARY" worktree add --quiet -b "$2" "$tree" origin/master
  install_scripts "$tree"
  printf '%s\n' "$1" >"$tree/$1.txt"
  git_quiet -C "$tree" add "$1.txt"
  git_quiet -C "$tree" commit --quiet -m "$1 work"
  git_quiet -C "$tree" push --quiet -u origin "$2"
  printf '%s' "$tree"
}

# Runs agent-merge.sh with a stubbed gate and status source. Never touches the real lock.
run_merge() {  # $1 = tree, rest = extra env assignments / flags
  local tree=$1; shift
  ( cd "$tree" && env \
      HOME="$TEMP/home" \
      AGENT_MERGE_LOCK="$TEMP/lock" \
      AGENT_MERGE_LOCK_WAIT_S=2 \
      AGENT_MERGE_POLL_S=1 \
      AGENT_MERGE_DEPLOY_WAIT_S=6 \
      AGENT_MERGE_STALE_S=0 \
      AGENT_MERGE_GATE_CMD="${GATE_STUB-true}" \
      AGENT_MERGE_TYPESCRIPT_GATE_CMD="${TYPESCRIPT_GATE_STUB-}" \
      AGENT_MERGE_RUST_GATE_CMD="${RUST_GATE_STUB-}" \
      AGENT_MERGE_DEPLOYMENT_GATE_CMD="${DEPLOYMENT_GATE_STUB-}" \
      AGENT_MERGE_STATIC_GATE_CMD="${STATIC_GATE_STUB-}" \
      AGENT_MERGE_STATUS_CMD="${STATUS_STUB-printf success}" \
      AGENT_MERGE_STAGE_STATUS_CMD="${STAGE_STATUS_STUB-printf success}" \
      AGENT_MERGE_ELIGIBLE_STATUS_CMD="${ELIGIBLE_STATUS_STUB-printf pending}" \
      AGENT_MERGE_FAILURE_LOG_CMD="${FAILURE_LOG_STUB-true}" \
      AGENT_MERGE_VALIDATION_REQUEST_CMD="${VALIDATION_REQUEST_STUB-printf 1}" \
      AGENT_MERGE_VALIDATION_STATUS_CMD="${VALIDATION_STATUS_STUB-printf success}" \
      "$@" bash "$tree/deploy/agent-merge.sh" ${MERGE_FLAGS-} ) 2>&1
}

expect_failure() {  # $1 = description, $2 = expected substring, rest = command
  local description=$1 expected=$2; shift 2
  local output status=0
  output=$("$@" 2>&1) || status=$?
  (( status != 0 )) || wd_die "expected a refusal: $description"
  grep -Fq -- "$expected" <<<"$output" \
    || wd_die "wrong refusal for $description: $output"
}

# --- preflight refusals -------------------------------------------------------------------------
tree_a=$(new_agent_worktree agent-a feat/agent-a)

expect_failure 'merging from the primary working tree' 'primary working tree' \
  run_merge "$PRIMARY"

git_quiet -C "$PRIMARY" checkout --quiet master
expect_failure 'running on the deployment branch itself' 'refusing to run on master' \
  bash -c "cd '$PRIMARY' && AGENT_MERGE_LOCK='$TEMP/lock' bash deploy/agent-merge.sh --allow-primary-tree"

printf 'dirty\n' >"$tree_a/file.txt"
expect_failure 'a dirty working tree' 'working tree is dirty' run_merge "$tree_a"
git_quiet -C "$tree_a" checkout --quiet -- file.txt

git_quiet -C "$PRIMARY" worktree add --quiet --no-track -b feat/no-upstream "$TEMP/no-upstream" origin/master
install_scripts "$TEMP/no-upstream"
expect_failure 'a branch that was never pushed' 'has no upstream' run_merge "$TEMP/no-upstream"

# --- the gate runs before anything is pushed, and its failure is fatal ---------------------------
before=$(git --git-dir="$ORIGIN" rev-parse master)
GATE_STUB='exit 7' expect_failure 'a red gate' '' run_merge "$tree_a"
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$before" ]] \
  || wd_die 'a failing gate still pushed to master'
[[ ! -d $TEMP/lock ]] || wd_die 'a failing gate left the merge lock behind'

# --- the default local gate selects only relevant lanes and fails closed -------------------------
new_gate_worktree() {  # $1 = name, $2 = changed path
  local name=$1 path=$2 tree=$TEMP/gate-$name
  git_quiet -C "$PRIMARY" worktree add --quiet -b "feat/gate-$name" "$tree" origin/master
  install_scripts "$tree"
  mkdir -p -- "$(dirname -- "$tree/$path")"
  printf '# %s\n' "$name" >>"$tree/$path"
  git_quiet -C "$tree" add "$path"
  git_quiet -C "$tree" commit --quiet -m "gate $name"
  git_quiet -C "$tree" push --quiet -u origin "feat/gate-$name"
  printf '%s' "$tree"
}

assert_gate_selection() {  # $1=name $2=path $3=typescript $4=rust $5=deployment $6=typescript_full
  local name=$1 path=$2 typescript=$3 rust=$4 deployment=$5 typescript_full=$6
  local tree lane lane_expected
  local lane_log=$TEMP/gate-$name.lanes output=$TEMP/gate-$name.log
  tree=$(new_gate_worktree "$name" "$path")
  GATE_STUB='' MERGE_FLAGS=--dry-run \
    TYPESCRIPT_GATE_STUB="printf '%s\\n' typescript >>'$lane_log'" \
    RUST_GATE_STUB="printf '%s\\n' rust >>'$lane_log'" \
    DEPLOYMENT_GATE_STUB="printf '%s\\n' deployment >>'$lane_log'" \
    STATIC_GATE_STUB="printf '%s\\n' static >>'$lane_log'" \
    run_merge "$tree" >"$output" \
    || wd_die "path-aware gate scenario $name failed: $(cat "$output")"
  grep -Fq "local gate selection: typescript=$typescript typescript_full=$typescript_full rust=$rust deployment=$deployment static=1" \
    "$output" || wd_die "path-aware gate scenario $name selected the wrong lanes: $(cat "$output")"
  for lane in typescript rust deployment static; do
    lane_expected=0
    case "$lane" in
      typescript) lane_expected=$typescript ;;
      rust) lane_expected=$rust ;;
      deployment) lane_expected=$deployment ;;
      static) lane_expected=1 ;;
    esac
    if (( lane_expected == 1 )); then
      grep -Fxq "$lane" "$lane_log" \
        || wd_die "path-aware gate scenario $name did not execute selected $lane lane"
    elif grep -Fxq "$lane" "$lane_log"; then
      wd_die "path-aware gate scenario $name executed unselected $lane lane"
    fi
  done
}

assert_gate_selection docs docs/path-aware.md 0 0 0 0
assert_gate_selection typescript apps/path-aware.ts 1 0 0 0
assert_gate_selection rust crates/path-aware.rs 0 1 0 0
assert_gate_selection infrastructure deploy/path-aware.test.sh 0 0 1 0
assert_gate_selection workflow AGENTS.md 0 0 1 0
assert_gate_selection cursor-ssh-rule .cursor/rules/production-ssh-observe.mdc 0 0 1 0
assert_gate_selection unknown mystery/runtime.xyz 1 1 1 1
assert_gate_selection gate-machinery deploy/sccache-cargo.sh 1 1 1 1
assert_gate_selection worktree-manager deploy/agent-worktree.sh 1 1 1 1

# A feature SHA that fails the production host's trusted gate must never reach the merge lock or
# master, even if its local gate passed.
host_gate_log=$TEMP/host-gate.log
GATE_STUB="printf gate >>$host_gate_log" VALIDATION_STATUS_STUB='printf failure' \
  expect_failure 'a red trusted host gate' 'trusted host validation' run_merge "$tree_a"
[[ -s $host_gate_log ]] || wd_die 'the local gate did not run alongside trusted host validation'
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$before" ]] \
  || wd_die 'a host-rejected feature SHA still reached master'
[[ ! -d $TEMP/lock ]] || wd_die 'a failed trusted host gate reached the merge lock'

# The host's bounded, sanitized failure reason must reach the agent instead of collapsing into a
# generic red verdict that forces blind retries with new SHAs.
VALIDATION_STATUS_STUB=$'printf "failure\tphase=testing; TypeScript candidate lane failed (exit 1)"' \
  expect_failure 'an explained trusted host failure' \
    'phase=testing; TypeScript candidate lane failed (exit 1)' run_merge "$tree_a"

# --- production is checked before the expensive gate ---------------------------------------------
preflight_gate_log=$TEMP/preflight-gate.log
GATE_STUB="printf gate >>$preflight_gate_log" STATUS_STUB='printf failure' \
  expect_failure 'stacking onto a red master' 'is RED' run_merge "$tree_a"
GATE_STUB="printf gate >>$preflight_gate_log" \
  STATUS_STUB=$'printf "failure\tphase=migrating; line=1684; exit=1; candidate quarantined"' \
  expect_failure 'an explained red master' \
    'phase=migrating; line=1684; exit=1; candidate quarantined' run_merge "$tree_a"
FAILURE_LOG_STUB='printf "error: could not compile crates/router"' \
  GATE_STUB="printf gate >>$preflight_gate_log" \
  STATUS_STUB=$'printf "failure\tphase=testing; TypeScript candidate lane failed (exit 1)"' \
  expect_failure 'a red master includes the host failure log' \
    'error: could not compile crates/router' run_merge "$tree_a"
[[ ! -s $preflight_gate_log ]] || wd_die 'the full gate ran before the existing deployment was checked'
GATE_STUB="printf gate >>$preflight_gate_log" STATUS_STUB='printf pending' \
  expect_failure 'pushing before a deploying parent is green' 'could not verify a green' \
  run_merge "$tree_a"
[[ -s $preflight_gate_log ]] \
  || wd_die 'a pending committed parent did not overlap its rollout with speculative gates'
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$before" ]] \
  || wd_die 'speculative gates pushed before their parent deployment was green'
[[ ! -d $TEMP/lock ]] || wd_die 'a pending parent left the merge lock behind'

# --- happy path: unchanged SHA reuses its exact gate, lock released -------------------------------
gate_log=$TEMP/gate.log
validation_log=$TEMP/happy-validation.log
GATE_STUB="test -s $validation_log; git rev-parse HEAD >>$gate_log" \
  VALIDATION_REQUEST_STUB="bash -c 'printf \"%s\\n\" \"\$1\" >>\"$validation_log\"; printf 1' _" \
  run_merge "$tree_a" >"$TEMP/happy.log" \
  || wd_die "the happy path failed: $(cat "$TEMP/happy.log")"
pushed=$(git --git-dir="$ORIGIN" rev-parse master)
[[ $pushed != "$before" ]] || wd_die 'the happy path did not advance master'
[[ $pushed == "$(git_quiet -C "$tree_a" rev-parse HEAD)" ]] \
  || wd_die 'master does not point at the branch tip that was tested'
tail -n 1 "$gate_log" | grep -Fxq "$pushed" \
  || wd_die 'the gate did not run on the exact SHA that was pushed'
[[ $(wc -l <"$gate_log" | tr -d ' ') == 1 ]] \
  || wd_die 'an unchanged SHA was wastefully tested more than once'
[[ $(wc -l <"$validation_log" | tr -d ' ') == 1 ]] \
  || wd_die 'an unchanged SHA requested trusted host validation more than once'
grep -Fxq "$pushed" "$validation_log" \
  || wd_die 'trusted host validation was not requested for the exact pushed SHA'
grep -Fq 'reusing the local and trusted host gates already passed by unchanged SHA' "$TEMP/happy.log" \
  || wd_die 'the unchanged-SHA dual-gate reuse was not reported'
[[ ! -d $TEMP/lock ]] || wd_die 'the merge lock was not released'
grep -Fq 'GREEN' "$TEMP/happy.log" || wd_die 'the merge did not wait for a green deployment'
[[ $(git_quiet -C "$PRIMARY" rev-parse refs/heads/master) == "$pushed" ]] \
  || wd_die 'merge did not fast-forward local master to the SHA it pushed'

# --- local master follows GitHub even when primary is detached ---------------------------------
lagged=$(git_quiet -C "$PRIMARY" rev-parse HEAD)
git_quiet -C "$PRIMARY" checkout --quiet --detach
git_quiet -C "$PRIMARY" update-ref refs/heads/master "$lagged^"
[[ $(git_quiet -C "$PRIMARY" rev-parse refs/heads/master) != $(git --git-dir="$ORIGIN" rev-parse master) ]] \
  || wd_die 'fixture expected local master to lag GitHub before the detached merge'
tree_sync=$(new_agent_worktree agent-sync feat/agent-sync)
run_merge "$tree_sync" >"$TEMP/local-master.log" \
  || wd_die "detached-primary merge failed: $(cat "$TEMP/local-master.log")"
grep -Fq 'fast-forwarded local master' "$TEMP/local-master.log" \
  || wd_die "merge did not report a local master fast-forward: $(cat "$TEMP/local-master.log")"
[[ $(git_quiet -C "$PRIMARY" rev-parse --abbrev-ref HEAD) == HEAD ]] \
  || wd_die 'merge checked a branch out in a detached primary'
[[ $(git_quiet -C "$PRIMARY" rev-parse HEAD) == "$lagged" ]] \
  || wd_die 'merge moved a detached primary working tree'
[[ $(git_quiet -C "$PRIMARY" rev-parse refs/heads/master) == $(git --git-dir="$ORIGIN" rev-parse master) ]] \
  || wd_die 'merge did not fast-forward local master while primary was detached'

# --- a red deployment is reported and never silently accepted --------------------------------------
# The SHA is on master by the time the host reports on it, so the contract here is a loud failure
# and a released lock, not a rollback.
tree_b=$(new_agent_worktree agent-b feat/agent-b)
STATUS_STUB="bash -c 'test \"\$1\" = $(git --git-dir="$ORIGIN" rev-parse master) && printf success || printf failure' _" \
  expect_failure 'a red deployment of our own SHA' 'is RED' run_merge "$tree_b"
[[ ! -d $TEMP/lock ]] || wd_die 'a red deployment left the merge lock behind'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)
[[ $pushed == "$(git_quiet -C "$tree_b" rev-parse HEAD)" ]] \
  || wd_die 'the red-deployment scenario did not actually land its SHA'

# --- concurrency: a held lock blocks a second merge, a stale one is broken --------------------------
mkdir -p "$TEMP/lock"
printf '%s\n' "$$" >"$TEMP/lock/pid"
hostname >"$TEMP/lock/host"
expect_failure 'a merge lock held by a live process' 'held the lock' run_merge "$tree_b"
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'a merge pushed while another held the lock'

# A dead owner on this host past the staleness window is recoverable, so agents cannot wedge forever.
printf '%s\n' 999999 >"$TEMP/lock/pid"
run_merge "$tree_b" >"$TEMP/stale.log" || wd_die "a stale lock was not broken: $(cat "$TEMP/stale.log")"
grep -Fq 'breaking a stale lock' "$TEMP/stale.log" || wd_die 'the stale lock was not reported'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)

# --- a target that moves under us is rebased onto, not force-pushed over ----------------------------
tree_c=$(new_agent_worktree agent-c feat/agent-c)
tree_d=$(new_agent_worktree agent-d feat/agent-d)
# agent-d lands first, so agent-c rebases before either exact-SHA gate.
run_merge "$tree_d" >/dev/null || wd_die 'agent-d could not land'
landed_d=$(git --git-dir="$ORIGIN" rev-parse master)
stale_gate_log=$TEMP/stale-gate.log
stale_validation_log=$TEMP/stale-validation.log
GATE_STUB="git rev-parse HEAD >>$stale_gate_log" \
  VALIDATION_REQUEST_STUB="bash -c 'printf \"%s\\n\" \"\$1\" >>\"$stale_validation_log\"; printf 1' _" \
  run_merge "$tree_c" >"$TEMP/stale-target.log" \
  || wd_die 'agent-c could not land after the target moved'
landed_c=$(git --git-dir="$ORIGIN" rev-parse master)
git --git-dir="$ORIGIN" merge-base --is-ancestor "$landed_d" "$landed_c" \
  || wd_die 'a merge discarded a commit that had already landed on master'
[[ $(wc -l <"$stale_gate_log" | tr -d ' ') == 1 ]] \
  || wd_die 'a knowingly stale candidate was wastefully gated before its preflight rebase'
[[ $(wc -l <"$stale_validation_log" | tr -d ' ') == 1 ]] \
  || wd_die 'a knowingly stale candidate requested host validation before its preflight rebase'
tail -n 1 "$stale_gate_log" | grep -Fxq "$landed_c" \
  || wd_die 'the rebased SHA was not the last tree tested before push'
tail -n 1 "$stale_validation_log" | grep -Fxq "$landed_c" \
  || wd_die 'the rebased SHA was not the last tree validated by the trusted host'
grep -Fq 'rebasing the candidate onto committed master' "$TEMP/stale-target.log" \
  || wd_die 'the pre-gate target rebase was not reported'

# An out-of-band push between the gate and our push (a human bypassing the lock) must be absorbed by
# the retry loop, never by a force push.
tree_e=$(new_agent_worktree agent-e feat/agent-e)
race_marker=$TEMP/race.count
race_clone=$TEMP/race-clone
git_quiet clone --quiet "$ORIGIN" "$race_clone"
git_quiet -C "$race_clone" config user.name 'Out-of-band Writer'
git_quiet -C "$race_clone" config user.email 'race@example.invalid'
race_hooks=$TEMP/race-hooks
mkdir -p "$race_hooks"
cat >"$race_hooks/pre-push" <<RACE
#!/usr/bin/env bash
set -euo pipefail
# Git exports its own repository variables to hooks. Clear them before invoking the independent
# writer clone, or commands scoped to that clone would mutate the pushing worktree's index instead.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_COMMON_DIR
# Move master during the first real push. The original push must be rejected, then the retry must
# absorb, rebase onto, and re-test this commit before trying again.
if [[ ! -e "$race_marker" ]]; then
  : >"$race_marker"
  git -C "$race_clone" fetch --quiet origin master
  git -C "$race_clone" reset --hard --quiet origin/master
  printf 'out-of-band\n' >"$race_clone/interloper.txt"
  git -C "$race_clone" add interloper.txt
  git -C "$race_clone" commit --quiet -m 'out-of-band commit'
  git -C "$race_clone" push --quiet origin HEAD:master
fi
RACE
chmod +x "$race_hooks/pre-push"
git_quiet -C "$tree_e" config core.hooksPath "$race_hooks"
rm -f -- "$race_marker"
git_quiet -C "$PRIMARY" fetch --quiet origin
git_quiet -C "$PRIMARY" reset --hard --quiet origin/master
race_gate_log=$TEMP/race-gate.log
race_validation_log=$TEMP/race-validation.log
GATE_STUB="git rev-parse HEAD >>$race_gate_log" \
  VALIDATION_REQUEST_STUB="bash -c 'printf \"%s\\n\" \"\$1\" >>\"$race_validation_log\"; printf 1' _" \
  run_merge "$tree_e" >"$TEMP/race.log" \
  || wd_die "the push race was not absorbed: $(cat "$TEMP/race.log")"
grep -Fq 'moved under us, retrying' "$TEMP/race.log" || wd_die 'the push race did not trigger a retry'
[[ $(wc -l <"$race_gate_log" | tr -d ' ') == 2 ]] \
  || wd_die 'a push race that changed the SHA did not force exactly one new gate'
[[ $(wc -l <"$race_validation_log" | tr -d ' ') == 2 ]] \
  || wd_die 'a push race that changed the SHA did not force exactly one new trusted host gate'
git --git-dir="$ORIGIN" cat-file -e "master:interloper.txt" \
  || wd_die 'the retry force-pushed over an out-of-band commit'
git --git-dir="$ORIGIN" merge-base --is-ancestor "$landed_c" "$(git --git-dir="$ORIGIN" rev-parse master)" \
  || wd_die 'the retry discarded previously landed history'
[[ $(git --git-dir="$ORIGIN" log --oneline master | wc -l | tr -d ' ') -ge 7 ]] \
  || wd_die 'landed history is missing commits'
git_quiet -C "$tree_e" config --unset core.hooksPath

# --- portability: no tools that are absent on a stock macOS contributor machine ---------------------
# The mtime helper must return digits on THIS platform. GNU stat exits 0 while printing `File: "..."`
# for the BSD flag, and that text reaching $(( )) is an unbound-variable crash under set -u, not a
# fallback. This is the failure that quarantined three SHAs: it cannot reproduce on macOS.
mtime_probe=$(bash -c "$(sed -n '/^am_mtime()/,/^}/p' "$MERGE"); am_mtime /tmp" 2>&1)
[[ $mtime_probe =~ ^[0-9]+$ ]] \
  || wd_die "am_mtime must return digits on this platform, got: $mtime_probe"
(( mtime_probe > 0 )) || wd_die 'am_mtime returned 0, so lock staleness would be meaningless'
grep -Eq 'stat -f %m[^\n]*\|\|[[:space:]]*stat -c' "$MERGE" \
  && wd_die 'the BSD stat form must not be tried first: GNU stat exits 0 with non-numeric output'
grep -Eq '(^|[^-])\bflock\b' "$MERGE" && wd_die 'agent-merge.sh depends on flock, absent on macOS'
grep -q '\bgh \(api\|pr\|auth\)' "$MERGE" && wd_die 'agent-merge.sh depends on the gh CLI'
grep -Fq 'stat -f %m' "$MERGE" || wd_die 'agent-merge.sh must read mtime portably on BSD'
grep -Fq 'stat -c %Y' "$MERGE" || wd_die 'agent-merge.sh must read mtime portably on GNU'
grep -Fq -- '--allow-primary-tree' "$MERGE" \
  || wd_die 'human contributors in a plain clone need an escape hatch'
grep -Fq 'GIT_CONFIG_GLOBAL=/dev/null' "${BASH_SOURCE[0]}" \
  || wd_die 'this suite must never consult a real credential helper'
# The production gate runs this suite on a host provisioned for node, not python.
for portable in "$MERGE" "$GUARD" "$WORKTREE_MANAGER" "${BASH_SOURCE[0]}"; do
  grep -Eq 'python[0-9]?[[:space:]]+-' "$portable" \
    && wd_die "$(basename "$portable") invokes python, which the deployment host does not provide"
done

# --- the git guard blocks exactly what it should ----------------------------------------------------
guard() {  # $1 = command line -> exit status of the hook
  printf '{"tool_input":{"command":%s}}' "$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1")" \
    | bash "$GUARD" >/dev/null 2>&1
}
for blocked in \
  'git checkout master' \
  'git switch -c other' \
  'cd /tmp && git checkout comp/forward' \
  'git stash' \
  'git reset --hard origin/master' \
  'git clean -fd' \
  'git merge master' \
  'git rebase -i HEAD~3' \
  'git add -A' \
  'git add .' \
  'git add --all' \
  'git push origin HEAD:master' \
  'git push origin HEAD:stage' \
  'git push origin refs/heads/stage' \
  'git worktree add ~/wt/task -b feat/task origin/master' \
  'git worktree remove ../other-agent' \
  'MODE=repair git checkout master' \
  'cargo build && git checkout master'; do
  guard "$blocked" && wd_die "the git guard allowed a destructive command: $blocked"
done
for allowed in \
  'git status --short' \
  'git add crates/forward/src/codex/chat.rs' \
  'git commit -m "forward: sanitize upstream errors"' \
  'git diff --stat origin/master...HEAD' \
  'git push -u origin HEAD' \
  'git worktree list --porcelain' \
  './deploy/agent-merge.sh' \
  './deploy/agent-merge-stage.sh' \
  './deploy/agent-worktree.sh create feat/task' \
  './deploy/agent-worktree.sh finish ~/wt/task' \
  './deploy/agent-worktree.sh doctor' \
  './deploy/agent-worktree.sh gc' \
  'cargo test --locked --workspace' \
  'MODE=inspect git status --short' \
  'MODE=inspect' \
  $'node - <<\'JS\'\npath=\'crates/authbot/src/main.rs\'\nconsole.log(path)\nJS' \
  'grep -rn "git merge" BRANCHES.md' \
  'echo "run git checkout to switch branches"'; do
  guard "$allowed" || wd_die "the git guard blocked a legitimate command: $allowed"
done
guard '' && true  # an empty command must not crash the hook
printf '{"tool_input":{}}' | bash "$GUARD" >/dev/null 2>&1 \
  || wd_die 'the git guard must ignore payloads without a command'
printf 'not json' | bash "$GUARD" >/dev/null 2>&1 \
  || wd_die 'the git guard must fail open on an unparseable payload'

# --- the workflow is actually wired into the repository ---------------------------------------------
# Back in the production gate, having been verified on the host itself rather than only on macOS.
grep -Fq 'deploy/agent-merge.suite.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the merge-path suite'
grep -Fq 'deploy/lib.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the activation-journal suite'
grep -Fq 'deploy/contour-config.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the contour-config suite'
grep -Fq 'deploy/stage-unit-renderer.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the stage unit renderer suite'
grep -Fq 'deploy/staging-foundation.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the staging foundation suite'
grep -Fq 'deploy/codex-homes-migrate.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the Codex home migration suite'
! grep -Fq 'deploy/codex-app-servers.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate still runs the removed Codex app-server ownership suite'
grep -Fq 'deploy/sccache-cargo.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not test serialized sccache startup'
grep -Fq 'deploy/agent-worktree.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not test managed worktree lifecycle safety'
grep -Fq 'deploy/delete-worktree-agent.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not test persistent worktree cleanup safety'
grep -Fq 'deploy/next-cache.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the persistent Next.js cache suite'
grep -Fq 'deploy/typescript-build-contexts.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the component-aware build suite'
grep -Fq 'deploy/typescript-test-groups.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the parallel TypeScript-group suite'
grep -Fq 'deploy/local-test-databases.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the local sales/openkeys database helper suite'
grep -Fq 'TYPESCRIPT_TEST_COMPONENTS="$context_list"' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the local TypeScript gate no longer selects database test components'
grep -Fq 'deploy/local-test-databases.sh" ensure' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the local TypeScript gate no longer creates compose.yaml sales/openkeys databases'
grep -Fq 'TEST_SALES_DATABASE_URL="${TEST_SALES_DATABASE_URL:-postgresql://commerce:commerce-local-only@127.0.0.1:5433/sales}"' \
  "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the local TypeScript gate no longer defaults TEST_SALES_DATABASE_URL to compose.yaml'
grep -Fq 'deploy/commerce-release-bundle.test.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run the compact release-bundle suite'
grep -Fq 'deploy/sccache-cargo.sh" cargo test --locked --workspace' \
  "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge gate does not run Rust tests through the shared compilation cache'
for sccache_contract in \
  'git-common-dir' \
  'worktree list --porcelain' \
  'SCCACHE_BASEDIRS' \
  'SCCACHE_SERVER_UDS' \
  'sc_start_server_serialized' \
  'continuing uncached' \
  'CARGO_INCREMENTAL=0' \
  'CARGO_BUILD_BUILD_DIR' \
  'worktree-local Cargo artifacts' \
  'SCCACHE_CACHE_SIZE' \
  'SCCACHE_VERSION=0.15.0'; do
  grep -Fq -- "$sccache_contract" "$ROOT/deploy/sccache-cargo.sh" \
    || wd_die "shared Rust cache lost required contract: $sccache_contract"
done
for next_cache_contract in \
  'git-common-dir' \
  'codex-tools/next-cache' \
  'deploy/next-cache.sh" restore "$ROOT"' \
  'deploy/next-cache.sh" save "$ROOT"'; do
  grep -Fq -- "$next_cache_contract" "$ROOT/deploy/agent-merge.sh" \
    || wd_die "merge path lost persistent Next.js cache contract: $next_cache_contract"
done
for parallel_gate_contract in \
  'am_gate_typescript "$base" "$target" "$typescript_full" & typescript_pid=$!' \
  'am_gate_rust & rust_pid=$!' \
  'am_gate_deployment "$base" "$target" & deployment_pid=$!' \
  'am_gate_static "$base" "$target" & static_pid=$!' \
  'wait "$typescript_pid" || typescript_rc=$?' \
  'wait "$rust_pid" || rust_rc=$?' \
  'wait "$deployment_pid" || deployment_rc=$?' \
  'wait "$static_pid" || static_rc=$?' \
  'local gate lanes failed (typescript=$typescript_rc rust=$rust_rc deployment=$deployment_rc static=$static_rc)'; do
  grep -Fq -- "$parallel_gate_contract" "$ROOT/deploy/agent-merge.sh" \
    || wd_die "merge path lost parallel local-gate contract: $parallel_gate_contract"
done
for path_gate_contract in \
  'diff --name-only --no-renames --diff-filter=ACDMRTUXB' \
  'wd_range_has_unknown_validation_path' \
  'next-cache.sh' \
  'typescript-scope.mjs' \
  'typescript-build-contexts.sh' \
  'typescript-test-groups.sh' \
  'typescript_full=1' \
  'local gate machinery changed; forcing every expensive lane' \
  'am_gate "$previous" "$candidate"'; do
  grep -Fq -- "$path_gate_contract" "$ROOT/deploy/agent-merge.sh" \
    || wd_die "merge path lost fail-closed path-aware contract: $path_gate_contract"
done
grep -Fq 'agent-merge.suite.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the merge-path suite'
grep -Fq 'agent-worktree.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the managed worktree lifecycle suite'
grep -Fq 'delete-worktree-agent.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the persistent worktree cleanup suite'
grep -Fq 'deploy/lib.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the activation-journal suite'
grep -Fq 'deploy/contour-config.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the contour-config suite'
grep -Fq 'deploy/stage-unit-renderer.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the stage unit renderer suite'
grep -Fq 'deploy/staging-foundation.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the staging foundation suite'
grep -Fq 'deploy/codex-homes-migrate.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the Codex home migration suite'
grep -Fq 'deploy/host-image-gate.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the host-image wiring suite'
! grep -Fq 'deploy/host-image-gate.sh"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production host must not run the privileged Ubuntu host-image container'
grep -Fq 'deploy/host-image-gate.sh' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the local merge gate does not run the Ubuntu host-image proofs'
! grep -Fq 'deploy/codex-app-servers.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate still runs the removed Codex app-server ownership suite'
# Требуем сам запуск с набором пакетов, а не дословную цитату аргументов: точное
# написание уже однажды сломалось от правки переносимости (пустой массив под set -u
# в bash 3.2), хотя гарантия — «гейт гоняет параллельные TypeScript-тесты» — не менялась.
for typescript_group_gate in \
  "$ROOT/deploy/agent-merge.sh:\$ROOT" \
  "$ROOT/deploy/watchdog.sh:\$candidate"; do
  typescript_group_file=${typescript_group_gate%:*}
  typescript_group_root=${typescript_group_gate##*:}
  grep -Fq -- "typescript-test-groups.sh\" \"$typescript_group_root\"" "$typescript_group_file" \
    || wd_die "deployment gates lost parallel TypeScript tests: $typescript_group_file"
  grep -Fq -- 'test_packages[@]' "$typescript_group_file" \
    || wd_die "deployment gates no longer pass the selected packages: $typescript_group_file"
done
for trusted_gate_contract in \
  'transient_environment: true' \
  'production_environment: false' \
  'candidate-validation' \
  'rebase "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET"' \
  'am_publish_validation_sha "$candidate"' \
  'am_wait_for_validation "$validation_id" "$candidate"' \
  'candidate == "$GATED_SHA" && $candidate == "$VALIDATED_SHA"'; do
  grep -Fq -- "$trusted_gate_contract" "$ROOT/deploy/agent-merge.sh" \
    || wd_die "merge path lost trusted exact-SHA validation contract: $trusted_gate_contract"
done
[[ ! -e $ROOT/deploy/agent-merge.test.sh ]] \
  || wd_die 'the report-only shim outlived its purpose; the installed watchdog no longer calls it'
grep -Fq 'guard-git.sh' "$ROOT/.claude/settings.json" \
  || wd_die 'the git guard is not registered as a PreToolUse hook'
node -e '
const settings = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
const timeout = settings?.hooks?.PreToolUse?.[0]?.hooks?.[0]?.timeout;
if (timeout !== 15) process.exit(1);
' "$ROOT/.claude/settings.json" \
  || wd_die 'the git guard must have a bounded Claude Code timeout'
grep -Fq '.claude/worktrees/' "$ROOT/.gitignore" \
  || wd_die 'agent worktrees under .claude/ would be committed'
for document in CLAUDE.md AGENTS.md BRANCHES.md CONTRIBUTING.md; do
  grep -Fq 'agent-merge.sh' "$ROOT/$document" \
    || wd_die "$document does not point agents at the serialized merge path"
  grep -Fq 'worktree' "$ROOT/$document" \
    || wd_die "$document does not require an isolated worktree"
  grep -Fq 'agent-worktree.sh' "$ROOT/$document" \
    || wd_die "$document does not route agents through the managed worktree lifecycle"
done
grep -Fq 'git checkout comp/forward' "$ROOT/BRANCHES.md" \
  && wd_die 'BRANCHES.md still teaches agents to switch branches in a shared tree'

# --- the verdict is one named context, never the combined state -------------------------------------
# GitHub reports a combined "success" as soon as the contexts posted so far are green, so a SHA whose
# gate has not started yet looks green while Vercel is the only reporter. Treating that as a finished
# deployment is what announced 316691e as green a minute before deploy/tests failed.
mkdir -p "$TEMP/bin"
curl_log=$TEMP/curl-stdin.log
response=$TEMP/curl-response.json
cat >"$TEMP/bin/curl" <<CURL
#!/usr/bin/env bash
# Records the curl configuration handed to it on stdin, then replays the next canned status payload
# (the last one repeats), so a scenario can model a target that changes verdict between calls.
cat >>"$curl_log"
call=\$(( \$(cat "$TEMP/curl-calls" 2>/dev/null || printf 0) + 1 ))
printf '%s' "\$call" >"$TEMP/curl-calls"
total=\$(grep -c '' "$response")
(( call <= total )) || call=\$total
sed -n "\${call}p" "$response"
CURL
chmod +x "$TEMP/bin/curl"
# Sets the canned status payloads, one per call, and resets the call counter.
set_responses() { printf '%s\n' "$@" >"$response"; printf 0 >"$TEMP/curl-calls"; }
git_quiet -C "$PRIMARY" config credential.helper \
  '!f() { test "$1" = get && printf "username=x\npassword=fake-keychain-token\n"; }; f'

partial='{"state":"success","statuses":[{"context":"Vercel","state":"success"}]}'
finished='{"state":"success","statuses":[{"context":"Vercel","state":"success"},{"context":"deploy/watchdog","state":"success"}]}'
broken='{"state":"failure","statuses":[{"context":"deploy/watchdog","state":"failure"}]}'

tree_f=$(new_agent_worktree agent-f feat/agent-f)
pushed=$(git --git-dir="$ORIGIN" rev-parse master)
# The existing SHA first has only Vercel's partial combined success, then gets its watchdog verdict.
# Its descendant may be gated immediately, but the merge must still wait for the named verdict
# before pushing and continue without asking anybody.
set_responses "$partial" "$finished"
STATUS_STUB='' run_merge "$tree_f" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN='' >"$TEMP/credential.log" \
  || wd_die "the credential fallback path failed: $(cat "$TEMP/credential.log")"
grep -Fq 'still deploying; starting speculative exact-SHA gates' "$TEMP/credential.log" \
  || wd_die 'a pending committed target did not start speculative validation'

# --- the token comes from git's own credential store, and never leaks -------------------------------
# A contributor who can push normally already has a credential for this remote, so the status checks
# work with no GITHUB_TOKEN and no setup at all.
grep -Fq 'Authorization: Bearer fake-keychain-token' "$curl_log" \
  || wd_die 'the status check did not reuse the credential git already holds'
grep -Fq 'fake-keychain-token' "$TEMP/credential.log" \
  && wd_die 'the token leaked into the merge output'
grep -Fq 'GREEN' "$TEMP/credential.log" || wd_die 'a finished green deployment was not recognized'
grep -Eq -- '-H[[:space:]]*.?Authorization' "$MERGE" \
  && wd_die 'the token must reach curl through -K, never argv, where the process list exposes it'
grep -Fq -- '-K -' "$MERGE" || wd_die 'the status check must read its credential header from stdin'
grep -Fq 'GIT_TERMINAL_PROMPT=0' "$MERGE" \
  || wd_die 'a contributor without a credential helper must never be prompted or hung'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)

# A transient API/JSON failure after the push must be retried, never converted into a blind cooldown
# and a request for somebody else to prove the deployment.
tree_i=$(new_agent_worktree agent-i feat/agent-i)
set_responses "$finished" "$finished" 'not-json' "$finished"
STATUS_STUB='' run_merge "$tree_i" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN='' >"$TEMP/transient.log" \
  || wd_die "a transient status failure was not retried: $(cat "$TEMP/transient.log")"
grep -Fq 'temporarily unavailable; retrying autonomously' "$TEMP/transient.log" \
  || wd_die 'a transient status failure was not reported as an autonomous retry'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)

# --- a red target blocks unrelated work but never blocks its own repair ------------------------------
set_responses "$broken"
tree_h=$(new_agent_worktree agent-h feat/agent-h)
STATUS_STUB='' expect_failure 'stacking unrelated work on a red target' 'is RED' \
  run_merge "$tree_h" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN=''
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'unrelated work landed on a red target'
# The repair for that failure must be able to land, or a red master wedges delivery for everyone.
# The target is red when we check it, and its own deployment is green once it lands.
set_responses "$broken" "$broken" "$finished"
MERGE_FLAGS=--fix-red STATUS_STUB='' run_merge "$tree_h" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN='' \
  >"$TEMP/fixred.log" || wd_die "a repair could not land on a red target: $(cat "$TEMP/fixred.log")"
grep -Fq 'proceeding because --fix-red' "$TEMP/fixred.log" \
  || wd_die 'the red override was not reported'
[[ $(git --git-dir="$ORIGIN" rev-parse master) != "$pushed" ]] \
  || wd_die 'the repair did not land'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)

# An unattested master push is the Phase 7 admission failure class: deploy/tests can be green
# while production quarantines the SHA. The merge client must refuse before GitHub master moves.
tree_stage=$(new_agent_worktree agent-stage feat/agent-stage)
STAGE_STATUS_STUB='printf pending' expect_failure 'an unstaged candidate' \
  'without GREEN deploy/stage' \
  run_merge "$tree_stage"
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'an unstaged candidate still pushed to master'
STAGE_STATUS_STUB='printf failure' MERGE_FLAGS=--fix-red \
  expect_failure 'a red-master repair that skipped stage' \
  'without GREEN deploy/stage' \
  run_merge "$tree_stage"
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'a skipped-stage repair still pushed to master'
STAGE_STATUS_STUB='printf failure' MERGE_FLAGS=--hotfix \
  run_merge "$tree_stage" >"$TEMP/hotfix.log" \
  || wd_die "a hotfix path was blocked by deploy/stage: $(cat "$TEMP/hotfix.log")"
grep -Fq 'skipping deploy/stage and promotion/eligible because --hotfix was given' "$TEMP/hotfix.log" \
  || wd_die 'the hotfix override was not reported'
[[ $(git --git-dir="$ORIGIN" rev-parse master) != "$pushed" ]] \
  || wd_die 'the hotfix path did not land'
pushed=$(git --git-dir="$ORIGIN" rev-parse master)
tree_eligible=$(new_agent_worktree agent-eligible feat/agent-eligible)
STAGE_STATUS_STUB='printf success' ELIGIBLE_STATUS_STUB='printf failure' MERGE_FLAGS= \
  expect_failure 'a SHA whose promotion/eligible was revoked' \
  'because promotion/eligible is failure' \
  run_merge "$tree_eligible"
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'a revoked eligible SHA still pushed to master'
grep -Fq 'without GREEN deploy/stage' "$MERGE" \
  || wd_die 'the merge client must name the unattested-master refusal'
grep -Fq -- '--hotfix' "$MERGE" \
  || wd_die 'the merge client must expose the documented hotfix override'
grep -Fq 'promotion/eligible' "$MERGE" \
  || wd_die 'the merge client must read the promotion/eligible mirror'

# No helper and no GITHUB_TOKEN fails closed before the gate or merge. The agent repairs its own
# credential path and reruns; the workflow must never delegate token supply or green proof to a human.
git_quiet -C "$PRIMARY" config credential.helper \
  '!f() { test "$1" = get && printf "\n"; }; f'
tree_g=$(new_agent_worktree agent-g feat/agent-g)
STATUS_STUB='' expect_failure 'a missing reusable credential' \
  'autonomous GitHub deployment/validation access is unavailable' \
  run_merge "$tree_g" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN=''
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$pushed" ]] \
  || wd_die 'a merge proceeded without autonomous deployment/validation access'
git_quiet -C "$PRIMARY" config --unset credential.helper
grep -Fq 'Check deploy/watchdog on GitHub yourself' "$MERGE" \
  && wd_die 'the merge workflow still delegates deployment proof to a human'
grep -Fq 'blind cooldown' "$MERGE" \
  && wd_die 'the merge workflow still silently accepts an unknown deployment'

# --- the guard does not fail open when node is missing ----------------------------------------------
mkdir -p "$TEMP/nonode"
for tool in bash sed tr cat grep; do
  ln -sf "$(command -v "$tool")" "$TEMP/nonode/$tool" 2>/dev/null || true
done
printf '{"tool_input":{"command":"git checkout master"}}' \
  | env PATH="$TEMP/nonode" bash "$GUARD" >/dev/null 2>&1 \
  && wd_die 'the git guard fails open when node is unavailable'

printf 'agent worktree isolation, serialized merge, and git guard tests passed\n'
