#!/usr/bin/env bash
# Behaviour of the serial stage client: freeze, --fix-red only on a red master, no --hotfix.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
STAGE=$ROOT/deploy/agent-merge-stage.sh
TEMP=$(mktemp -d)
TEMP=$(cd -- "$TEMP" && pwd -P)
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf 'agent-merge-stage.test: %s\n' "$*" >&2; exit 1; }
[[ -x $STAGE ]] || fail 'deploy/agent-merge-stage.sh must be executable'
bash -n "$STAGE" || fail 'deploy/agent-merge-stage.sh does not parse'
grep -Fq -- '--hotfix is a master-only override' "$STAGE" || fail 'stage client must refuse --hotfix'
grep -Fq 'requires origin/master deploy/watchdog to be RED' "$STAGE" || fail 'stage client must gate --fix-red on a red master'

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export HOME=$TEMP/home
mkdir -p "$HOME"

git_test() {
  git -c user.name='Stage Client Tests' -c user.email=stage-client-tests@example.invalid \
    -c commit.gpgsign=false "$@"
}

ORIGIN=$TEMP/origin.git
git_test init --quiet --bare -b master "$ORIGIN"
REPO=$TEMP/repo
git_test init --quiet -b master "$REPO"
git_test -C "$REPO" remote add origin "$ORIGIN"
printf 'base\n' >"$REPO/file.txt"
git_test -C "$REPO" add file.txt
git_test -C "$REPO" commit --quiet -m base
git_test -C "$REPO" push --quiet origin HEAD:master
printf 'stage-only\n' >"$REPO/file.txt"
git_test -C "$REPO" commit --quiet -am 'unpromoted stage'
git_test -C "$REPO" push --quiet origin HEAD:stage
git_test -C "$REPO" reset --quiet --hard origin/master
printf 'candidate\n' >"$REPO/file.txt"
git_test -C "$REPO" commit --quiet -am candidate
git_test -C "$REPO" checkout --quiet -b feat/stage-client
git_test -C "$REPO" push --quiet -u origin HEAD

mkdir -p "$REPO/deploy"
cp -- "$STAGE" "$REPO/deploy/agent-merge-stage.sh"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$REPO/deploy/agent-merge.sh"
chmod +x "$REPO/deploy/agent-merge-stage.sh" "$REPO/deploy/agent-merge.sh"

run_stage() {
  local output status=0
  output=$(
    cd "$REPO" && env \
      AGENT_MERGE_STAGE_LOCK="$TEMP/stage.lock.d" \
      AGENT_MERGE_STAGE_STATUS_CMD="$AGENT_MERGE_STAGE_STATUS_CMD" \
      bash "$REPO/deploy/agent-merge-stage.sh" "$@" 2>&1
  ) || status=$?
  printf '%s\n' "$output"
  return "$status"
}

rm -rf "$TEMP/stage.lock.d"
AGENT_MERGE_STAGE_STATUS_CMD='printf success'
output=$(run_stage) && fail "a frozen stage still moved without --fix-red: $output"
[[ $output == *'stage is frozen at unpromoted SHA'* ]] || fail "freeze refusal missing: $output"

rm -rf "$TEMP/stage.lock.d"
output=$(run_stage --fix-red) && fail "a green master still replaced frozen stage: $output"
[[ $output == *'requires origin/master deploy/watchdog to be RED'* ]] || fail "green-master --fix-red refusal missing: $output"

rm -rf "$TEMP/stage.lock.d"
output=$(run_stage --hotfix) && fail "--hotfix still moved stage: $output"
[[ $output == *'--hotfix is a master-only override'* ]] || fail "--hotfix refusal missing: $output"

rm -rf "$TEMP/stage.lock.d"
mkdir -p "$TEMP/stage.lock.d"
printf '999999\n' >"$TEMP/stage.lock.d/pid"
output=$(run_stage) && fail "a frozen stage still moved after a stale lock: $output"
[[ $output == *'breaking a stale stage lock left by dead pid 999999'* ]] || fail "stale-pid break missing: $output"
[[ $output == *'stage is frozen at unpromoted SHA'* ]] || fail "freeze refusal after stale-pid break missing: $output"

rm -rf "$TEMP/stage.lock.d"
AGENT_MERGE_STAGE_STATUS_CMD='if [[ $2 == deploy/watchdog ]]; then printf failure; else printf success; fi'
output=$(run_stage --fix-red) || fail "a red master could not replace frozen stage: $output"
[[ $output == *'replacing frozen unpromoted SHA'* ]] || fail "red-master --fix-red warning missing: $output"
[[ $output == *'deploy/stage is GREEN'* ]] || fail "stage wait did not complete: $output"
[[ $(git --git-dir="$ORIGIN" rev-parse refs/heads/stage) == "$(git -C "$REPO" rev-parse HEAD)" ]] \
  || fail 'red-master --fix-red did not move origin/stage to HEAD'

printf 'agent-merge-stage.test: ok\n'
