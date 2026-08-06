#!/usr/bin/env bash
set -euo pipefail

ADMISSION_SHA=3f412e33d631f2956a575e40f7f28f8b0b592106
MAX_CYCLES=3
DRIFT_EXIT=75
BLOCKED_EXIT=76
STATE_PARENT=/var/lib/apitoken/pricing-stage567-converge-v2
STAGE56_HELPER=/usr/local/lib/apitoken-watchdog/controller/pricing-stage56-refresh-gate.sh
STAGE7_HELPER=/usr/local/lib/apitoken-watchdog/controller/pricing-stage7-refresh-gate.sh

[[ $# -eq 1 ]] || { printf 'usage: pricing-stage567-converge-v2-gate.sh <exact-admission-sha>\n' >&2; exit 2; }
[[ $1 == "$ADMISSION_SHA" ]] \
  || { printf 'pricing Stage 5-7 convergence is pinned to %s\n' "$ADMISSION_SHA" >&2; exit 1; }
[[ $(id -u) == 0 ]] \
  || { printf 'pricing Stage 5-7 convergence must run through the fixed root bridge\n' >&2; exit 1; }
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 1; }
command -v flock >/dev/null || { printf 'flock is required\n' >&2; exit 1; }

fixed_helper_is_trusted() {
  local helper=$1 parent=${1%/*}
  [[ -d $parent && ! -L $parent \
      && $(stat -c '%U:%G:%a' -- "$parent") == root:root:755 \
      && -f $helper && ! -L $helper \
      && $(stat -c '%U:%G:%a' -- "$helper") == root:root:755 ]]
}

stage6_result_is_valid() {
  local file=$1
  jq -s -e '
    length == 1 and .[0].state == "green" and
    ([.[0].stage5_plan_digest,.[0].target_plan_digest,.[0].target_release_digest,
      .[0].recovery_plan_digest,.[0].recovery_release_digest,
      .[0].job_result_digest,.[0].target_funding_manifest_digest,
      .[0].recovery_funding_manifest_digest] |
      all(type == "string" and test("^sha256:v2:[0-9a-f]{64}$"))) and
    ([.[0].target_generation,.[0].recovery_generation] |
      all(type == "string" and test("^[1-9][0-9]*$"))) and
    (.[0].target_generation | tonumber) < (.[0].recovery_generation | tonumber) and
    .[0].stage5_status == "prepared" and .[0].target_status == "prepared" and
    .[0].recovery_status == "prepared" and .[0].job_status == "confirmed" and
    (.[0].job_attempts | type == "number" and floor == . and . > 0) and
    (.[0].ready_accounts | type == "number" and floor == . and . > 0) and
    .[0].target_funding_manifest_digest == .[0].recovery_funding_manifest_digest
  ' "$file" >/dev/null
}

fixed_helper_is_trusted "$STAGE56_HELPER" \
  || { printf 'fixed Stage 5/6 helper is unavailable or untrusted\n' >&2; exit 1; }
fixed_helper_is_trusted "$STAGE7_HELPER" \
  || { printf 'fixed Stage 7 helper is unavailable or untrusted\n' >&2; exit 1; }
[[ ! -L $STATE_PARENT ]] \
  || { printf 'pricing Stage 5-7 convergence state path must not be a symlink\n' >&2; exit 1; }
install -d -o root -g root -m 0700 -- "$STATE_PARENT"
[[ $(stat -c '%U:%G:%a' -- "$STATE_PARENT") == root:root:700 ]] \
  || { printf 'pricing Stage 5-7 convergence state is not private root authority\n' >&2; exit 1; }
exec 9>"$STATE_PARENT/lock"
chmod 0600 "$STATE_PARENT/lock"
flock -n 9 || { printf 'another pricing Stage 5-7 convergence is active\n' >&2; exit 1; }

for ((cycle = 1; cycle <= MAX_CYCLES; cycle++)); do
  cycle_dir=$STATE_PARENT/cycle-$cycle
  stage6_result=$cycle_dir/stage6-result.json
  drift_state=$cycle_dir/inventory-drift.json
  [[ ! -L $cycle_dir ]] \
    || { printf 'pricing convergence cycle path must not be a symlink\n' >&2; exit 1; }
  install -d -o root -g root -m 0700 -- "$cycle_dir"
  [[ $(stat -c '%U:%G:%a' -- "$cycle_dir") == root:root:700 ]] \
    || { printf 'pricing convergence cycle state is not private root authority\n' >&2; exit 1; }

  if [[ -e $drift_state || -L $drift_state ]]; then
    [[ -f $drift_state && ! -L $drift_state \
        && $(stat -c '%U:%G:%a' -- "$drift_state") == root:root:600 ]] \
      || { printf 'pricing convergence drift fence is not private root authority\n' >&2; exit 1; }
    jq -e --argjson cycle "$cycle" \
      '(keys | sort) == (["cycle","state"] | sort) and .cycle == $cycle and
       (.state == "engine_inventory_drift" or .state == "rollout_blocked")' \
      "$drift_state" >/dev/null \
      || { printf 'pricing convergence drift fence is invalid\n' >&2; exit 1; }
    continue
  fi

  if [[ -e $stage6_result || -L $stage6_result ]]; then
    [[ -f $stage6_result && ! -L $stage6_result \
        && $(stat -c '%U:%G:%a' -- "$stage6_result") == root:root:600 ]] \
      || { printf 'terminal Stage 6 result is not private root authority\n' >&2; exit 1; }
    stage6_result_is_valid "$stage6_result" \
      || { printf 'persisted terminal Stage 6 result is invalid\n' >&2; exit 1; }
  else
    candidate=$(mktemp "$cycle_dir/.stage6-result.json.XXXXXX")
    chmod 0600 "$candidate"
    if ! "$STAGE56_HELPER" "$ADMISSION_SHA" --converge-cycle "$cycle" "$cycle_dir" >"$candidate"; then
      rm -f -- "$candidate"
      printf 'pricing Stage 5/6 convergence cycle %s failed\n' "$cycle" >&2
      exit 1
    fi
    stage6_result_is_valid "$candidate" \
      || { rm -f -- "$candidate"; printf 'terminal Stage 6 result is invalid\n' >&2; exit 1; }
    mv -- "$candidate" "$stage6_result"
  fi

  set +e
  "$STAGE7_HELPER" "$ADMISSION_SHA" --converge-cycle "$cycle" "$stage6_result"
  rc=$?
  set -e
  if (( rc == 0 )); then
    exit 0
  fi
  if (( rc != DRIFT_EXIT && rc != BLOCKED_EXIT )); then
    printf 'pricing Stage 7 convergence cycle %s failed with a non-recoverable blocker\n' "$cycle" >&2
    exit 1
  fi

  drift_candidate=$(mktemp "$cycle_dir/.inventory-drift.json.XXXXXX")
  if (( rc == BLOCKED_EXIT )); then
    jq -cn --argjson cycle "$cycle" '{cycle:$cycle,state:"rollout_blocked"}' >"$drift_candidate"
  else
    jq -cn --argjson cycle "$cycle" '{cycle:$cycle,state:"engine_inventory_drift"}' >"$drift_candidate"
  fi
  chmod 0600 "$drift_candidate"
  mv -- "$drift_candidate" "$drift_state"
  jq -c . "$drift_state"
done

printf 'pricing Stage 5-7 did not converge after %s fresh inventory cycles\n' "$MAX_CYCLES" >&2
exit 1
