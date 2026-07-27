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

[[ -x $MERGE ]] || wd_die 'deploy/agent-merge.sh must be executable'
[[ -x $GUARD ]] || wd_die '.claude/hooks/guard-git.sh must be executable'
bash -n "$MERGE" || wd_die 'deploy/agent-merge.sh does not parse'
bash -n "$GUARD" || wd_die '.claude/hooks/guard-git.sh does not parse'

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
# the two scripts inside the throwaway tree.
install_scripts() {
  mkdir -p -- "$1/deploy" "$1/.claude/hooks"
  cp -- "$MERGE" "$1/deploy/agent-merge.sh"
  cp -- "$GUARD" "$1/.claude/hooks/guard-git.sh"
}
install_scripts "$PRIMARY"
git_quiet -C "$PRIMARY" add deploy .claude
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
      AGENT_MERGE_STATUS_CMD="${STATUS_STUB-printf success}" \
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

# A feature SHA that fails the production host's trusted gate must never reach the merge lock or
# master, even if its local gate passed.
host_gate_log=$TEMP/host-gate.log
GATE_STUB="printf gate >>$host_gate_log" VALIDATION_STATUS_STUB='printf failure' \
  expect_failure 'a red trusted host gate' 'trusted host validation' run_merge "$tree_a"
[[ -s $host_gate_log ]] || wd_die 'the local gate did not run alongside trusted host validation'
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$before" ]] \
  || wd_die 'a host-rejected feature SHA still reached master'
[[ ! -d $TEMP/lock ]] || wd_die 'a failed trusted host gate reached the merge lock'

# --- production is checked before the expensive gate ---------------------------------------------
preflight_gate_log=$TEMP/preflight-gate.log
GATE_STUB="printf gate >>$preflight_gate_log" STATUS_STUB='printf failure' \
  expect_failure 'stacking onto a red master' 'is RED' run_merge "$tree_a"
[[ ! -s $preflight_gate_log ]] || wd_die 'the full gate ran before the existing deployment was checked'
GATE_STUB="printf gate >>$preflight_gate_log" STATUS_STUB='printf pending' \
  expect_failure 'stacking onto a deploying master' 'waiting for deploy/watchdog autonomously' \
  run_merge "$tree_a"
[[ ! -s $preflight_gate_log ]] || wd_die 'the full gate ran while the existing deployment was pending'
[[ $(git --git-dir="$ORIGIN" rev-parse master) == "$before" ]] \
  || wd_die 'a refused merge still pushed to master'

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
# agent-d lands first, so agent-c is stale by the time it reaches the lock.
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
[[ $(wc -l <"$stale_gate_log" | tr -d ' ') == 2 ]] \
  || wd_die 'a rebase that changed the candidate SHA did not force exactly one new gate'
[[ $(wc -l <"$stale_validation_log" | tr -d ' ') == 2 ]] \
  || wd_die 'a rebase that changed the candidate SHA did not request exactly one new host gate'
tail -n 1 "$stale_gate_log" | grep -Fxq "$landed_c" \
  || wd_die 'the rebased SHA was not the last tree tested before push'
tail -n 1 "$stale_validation_log" | grep -Fxq "$landed_c" \
  || wd_die 'the rebased SHA was not the last tree validated by the trusted host'

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
for portable in "$MERGE" "$GUARD" "${BASH_SOURCE[0]}"; do
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
  'git worktree remove ../other-agent' \
  'cargo build && git checkout master'; do
  guard "$blocked" && wd_die "the git guard allowed a destructive command: $blocked"
done
for allowed in \
  'git status --short' \
  'git add crates/forward/src/codex/chat.rs' \
  'git commit -m "forward: sanitize upstream errors"' \
  'git diff --stat origin/master...HEAD' \
  'git push -u origin HEAD' \
  'git worktree add ~/wt/task -b feat/task origin/master' \
  './deploy/agent-merge.sh' \
  'cargo test --locked --workspace' \
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
grep -Fq 'agent-merge.suite.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the merge-path suite'
grep -Fq 'deploy/lib.test.sh' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the production gate does not run the activation-journal suite'
for trusted_gate_contract in \
  'transient_environment: true' \
  'production_environment: false' \
  'candidate-validation' \
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
grep -Fq '.claude/worktrees/' "$ROOT/.gitignore" \
  || wd_die 'agent worktrees under .claude/ would be committed'
for document in CLAUDE.md AGENTS.md BRANCHES.md CONTRIBUTING.md; do
  grep -Fq 'agent-merge.sh' "$ROOT/$document" \
    || wd_die "$document does not point agents at the serialized merge path"
  grep -Fq 'worktree' "$ROOT/$document" \
    || wd_die "$document does not require an isolated worktree"
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
# The merge must wait by itself before running the gate and continue without asking anybody.
set_responses "$partial" "$finished"
STATUS_STUB='' run_merge "$tree_f" PATH="$TEMP/bin:$PATH" GITHUB_TOKEN='' >"$TEMP/credential.log" \
  || wd_die "the credential fallback path failed: $(cat "$TEMP/credential.log")"
grep -Fq 'waiting for deploy/watchdog autonomously' "$TEMP/credential.log" \
  || wd_die 'a partial combined success was treated as a finished deployment'

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
