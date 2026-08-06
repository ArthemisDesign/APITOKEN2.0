#!/usr/bin/env bash
set -euo pipefail

ADMISSION_SHA=3f412e33d631f2956a575e40f7f28f8b0b592106
API_ORIGIN=http://127.0.0.1:8791
ACTOR=gpt-image-2-stage56-inventory-refresh
REASON='refresh GPT Image 2 pricing admission after reviewed engine inventory drift'
POLL_SECONDS=5
MAX_POLLS=360
STATE_PARENT=/var/lib/apitoken/pricing-stage56-inventory-refresh
STATE_DIR=$STATE_PARENT/$ADMISSION_SHA
PLAN_STATE=$STATE_DIR/plan.json

[[ $# -eq 1 ]] || { printf 'usage: pricing-stage56-refresh-gate.sh <exact-admission-sha>\n' >&2; exit 2; }
[[ $1 == "$ADMISSION_SHA" ]] \
  || { printf 'pricing Stage 5/6 inventory refresh is pinned to %s\n' "$ADMISSION_SHA" >&2; exit 1; }
[[ $(id -u) == 0 ]] \
  || { printf 'pricing Stage 5/6 inventory refresh must run through the fixed root bridge\n' >&2; exit 1; }
command -v curl >/dev/null || { printf 'curl is required\n' >&2; exit 1; }
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 1; }

work=$(mktemp -d /run/pricing-stage56-inventory-refresh.XXXXXX)
chmod 0700 "$work"
trap 'rm -rf -- "$work"' EXIT

[[ ! -L $STATE_PARENT && ! -L $STATE_DIR ]] \
  || { printf 'pricing Stage 5/6 refresh state path must not be a symlink\n' >&2; exit 1; }
install -d -o root -g root -m 0700 -- "$STATE_PARENT" "$STATE_DIR"
[[ $(stat -c '%U:%G:%a' -- "$STATE_PARENT") == root:root:700 \
    && $(stat -c '%U:%G:%a' -- "$STATE_DIR") == root:root:700 ]] \
  || { printf 'pricing Stage 5/6 refresh state directories are not private root authority\n' >&2; exit 1; }
exec 9>"$STATE_DIR/lock"
chmod 0600 "$STATE_DIR/lock"
flock -n 9 || { printf 'another pricing Stage 5/6 inventory refresh is active\n' >&2; exit 1; }

load_admin_key_from_commerce_slot() {
  local unit pid entry name key cwd current
  current=$(readlink -f -- /opt/apitoken/releases/current)
  [[ $current == /opt/apitoken/releases/* ]] \
    || { printf 'active commerce release is not canonical\n' >&2; return 1; }
  for unit in apitoken-api@3000.service apitoken-api@3001.service; do
    [[ $(systemctl show "$unit" -p ActiveState --value 2>/dev/null) == active ]] || continue
    pid=$(systemctl show "$unit" -p MainPID --value 2>/dev/null)
    [[ $pid =~ ^[1-9][0-9]*$ ]] || continue
    cwd=$(readlink -f -- "/proc/$pid/cwd")
    [[ $cwd == "$current/apps/api" ]] || continue
    key=
    while IFS= read -r -d '' entry; do
      [[ $entry == *=* ]] || { printf 'active commerce slot has an invalid environment entry\n' >&2; return 1; }
      name=${entry%%=*}
      if [[ $name == COMMERCIAL_ADMIN_KEY ]]; then
        [[ -z $key ]] || { printf 'active commerce slot has duplicate admin credential state\n' >&2; return 1; }
        key=${entry#*=}
      fi
    done <"/proc/$pid/environ"
    [[ ${#key} -ge 32 ]] \
      || { printf 'active commerce slot lacks a valid admin credential\n' >&2; return 1; }
    COMMERCIAL_ADMIN_KEY=$key
    return 0
  done
  printf 'no active exact-release commerce slot can supply the admin credential\n' >&2
  return 1
}

pricing_control_error_diagnostic() {
  local output=$1 http_code=$2 code
  code=$(jq -er --argjson status "$http_code" '
    select(
      type == "object" and
      ((keys - ["code", "error", "message", "statusCode"]) | length) == 0 and
      .statusCode == $status and
      (.message | type == "string" and length >= 1 and length <= 300) and
      (.code | type == "string" and test("^[a-z][a-z0-9_]{0,99}$")) and
      ((.error // "") | type == "string")
    ) | .code
  ' "$output" 2>/dev/null) || { printf 'unclassified\n'; return; }
  printf '%s\n' "$code"
}

request() {
  local method=$1 path=$2 body_file=$3 output=$4 expected_codes=${5:-200 201} http_code diagnostic
  case "$method:$path" in
    POST:/v1/admin/pricing-stage5-v2/dry-run|\
    POST:/v1/admin/pricing-stage5-v2/materialize|\
    GET:/v1/admin/pricing-stage6-v2\?plan_digest=sha256:v2:[0-9a-f]*|\
    POST:/v1/admin/pricing-stage6-v2/stage) ;;
    *) printf 'refusing unexpected pricing control route\n' >&2; return 1 ;;
  esac
  http_code=$({
    printf 'url = "%s%s"\n' "$API_ORIGIN" "$path"
    printf 'request = "%s"\n' "$method"
    printf 'header = "content-type: application/json"\n'
    printf 'header = "x-admin-key: %s"\n' "$COMMERCIAL_ADMIN_KEY"
    printf 'header = "x-admin-actor: %s"\n' "$ACTOR"
    printf 'output = "%s"\n' "$output"
    printf 'write-out = "%%{http_code}"\n'
    printf 'silent\nshow-error\nmax-time = 30\n'
    [[ $method == GET ]] || printf 'data = "@%s"\n' "$body_file"
  } | curl --config -) || {
    unset COMMERCIAL_ADMIN_KEY
    printf 'pricing control request failed with HTTP %s\n' "${http_code:-unreachable}" >&2
    return 1
  }
  chmod 0600 "$output"
  [[ " $expected_codes " == *" $http_code "* ]] || {
    unset COMMERCIAL_ADMIN_KEY
    diagnostic=$(pricing_control_error_diagnostic "$output" "$http_code")
    printf 'pricing control request returned HTTP %s (%s)\n' "$http_code" "$diagnostic" >&2
    return 1
  }
  REQUEST_HTTP_CODE=$http_code
}

stage5_result_is_valid() {
  local expected_mode=$1 expected_status=$2 file=$3
  jq -e --arg mode "$expected_mode" --arg status "$expected_status" '
    def digest: type == "string" and test("^sha256:v2:[0-9a-f]{64}$");
    def uuid_or_null: . == null or (type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"));
    (keys | sort) == ([
      "blocker_count", "blockers", "commerce_inventory_digest", "engine_prepared",
      "engine_scan_first_digest", "engine_scan_second_digest", "funding_plan_digest", "mode",
      "openkeys_scan_first_digest", "openkeys_scan_second_digest", "plan_digest",
      "recovery_generation", "recovery_plan_digest", "run_id", "service_inventory_digest",
      "status", "target_generation", "target_plan_digest", "writes_committed"
    ] | sort) and
    .mode == $mode and .status == $status and
    (.plan_digest | digest) and (.commerce_inventory_digest | digest) and
    (.engine_scan_first_digest | digest) and (.engine_scan_second_digest | digest) and
    (.openkeys_scan_first_digest | digest) and (.openkeys_scan_second_digest | digest) and
    (.service_inventory_digest | digest) and (.funding_plan_digest | digest) and
    (.target_plan_digest | digest) and (.recovery_plan_digest | digest) and
    (.run_id | uuid_or_null) and
    (.target_generation | type == "number" and floor == . and . > 0) and
    (.recovery_generation | type == "number" and floor == . and . > 0) and
    .recovery_generation > .target_generation and
    (.blocker_count | type == "number" and floor == . and . >= 0) and
    (.blockers | type) == "array" and (.blockers | length) == .blocker_count and
    ([.writes_committed, .engine_prepared] | all(type == "boolean")) and
    (.blockers | all(
      (keys | sort) == (["blocker_code", "blocker_context", "blocker_digest", "detail", "subject_id"] | sort) and
      (.blocker_code | type == "string" and length >= 1 and length <= 200) and
      (.blocker_context | IN("commerce", "engine", "openkeys", "service", "funding", "release")) and
      (.blocker_digest | digest) and
      (.detail | type == "string" and length >= 1 and length <= 2000) and
      (.subject_id | type == "string" and length >= 1 and length <= 500)
    ))
  ' "$file" >/dev/null
}

stage6_result_is_valid() {
  local expected_digest=$1 file=$2
  jq -e --arg digest "$expected_digest" '
    def sha: type == "string" and test("^sha256:v2:[0-9a-f]{64}$");
    def nullable_sha: . == null or sha;
    def nullable_uuid: . == null or (type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"));
    [
      (((keys - ["staged_job_id"]) | sort) == ([
        "blocker_accounts", "job_attempts", "job_id", "job_last_error", "job_result_digest",
        "job_status", "pending_accounts", "processing_accounts", "ready_accounts",
        "recovery_funding_manifest_digest", "recovery_generation", "recovery_plan_digest",
        "recovery_release_digest", "recovery_status", "retry_accounts", "stage5_plan_digest",
        "stage5_status", "target_funding_manifest_digest", "target_generation", "target_plan_digest",
        "target_release_digest", "target_status"
      ] | sort)),
      (.stage5_plan_digest == $digest),
      (.stage5_status | IN("blocked", "planned", "materializing", "prepared", "failed")),
      (.target_generation | type == "string" and test("^[1-9][0-9]*$")),
      (.recovery_generation | type == "string" and test("^[1-9][0-9]*$")),
      (.target_plan_digest | sha), (.recovery_plan_digest | sha),
      (.target_release_digest | nullable_sha), (.recovery_release_digest | nullable_sha),
      (.target_status | IN("planned", "materializing", "prepared", "active", "superseded", "failed")),
      (.recovery_status | IN("planned", "materializing", "prepared", "active", "superseded", "failed")),
      (.job_id | nullable_uuid),
      (.job_status == null or (.job_status | IN("pending", "processing", "retry", "confirmed", "dead"))),
      (.job_attempts == null or (.job_attempts | type == "number" and floor == . and . >= 0)),
      (.job_last_error == null or (.job_last_error | type == "string")),
      (.job_result_digest | nullable_sha),
      ([.pending_accounts, .processing_accounts, .retry_accounts, .ready_accounts, .blocker_accounts] |
        all(type == "number" and floor == . and . >= 0)),
      (.target_funding_manifest_digest | nullable_sha),
      (.recovery_funding_manifest_digest | nullable_sha),
      (if has("staged_job_id") then (.staged_job_id | nullable_uuid) else true end)
    ] | all
  ' "$file" >/dev/null
}

plan_state_is_valid() {
  jq -e --arg sha "$ADMISSION_SHA" '
    def digest: type == "string" and test("^sha256:v2:[0-9a-f]{64}$");
    (keys | sort) == ([
      "admission_sha", "phase", "plan_digest", "recovery_generation", "recovery_plan_digest",
      "target_generation", "target_plan_digest"
    ] | sort) and
    .admission_sha == $sha and (.phase | IN("planned", "materialized")) and
    (.plan_digest | digest) and
    (.target_plan_digest | digest) and (.recovery_plan_digest | digest) and
    (.target_generation | type == "number" and floor == . and . > 0) and
    (.recovery_generation | type == "number" and floor == . and . > 0) and
    .recovery_generation > .target_generation
  ' "$PLAN_STATE" >/dev/null
}

stage6_identity_matches_plan() {
  local file=$1
  jq -e --slurpfile plan "$PLAN_STATE" '
    .stage5_plan_digest == $plan[0].plan_digest and
    .target_generation == ($plan[0].target_generation | tostring) and
    .target_plan_digest == $plan[0].target_plan_digest and
    .recovery_generation == ($plan[0].recovery_generation | tostring) and
    .recovery_plan_digest == $plan[0].recovery_plan_digest
  ' "$file" >/dev/null
}

load_admin_key_from_commerce_slot
plan_phase=absent
if [[ -e $PLAN_STATE || -L $PLAN_STATE ]]; then
  [[ -f $PLAN_STATE && ! -L $PLAN_STATE \
      && $(stat -c '%U:%G:%a' -- "$PLAN_STATE") == root:root:600 ]] \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'pricing Stage 5/6 plan fence is not private root authority\n' >&2; exit 1; }
  plan_state_is_valid \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'pricing Stage 5/6 plan fence is invalid\n' >&2; exit 1; }
  plan_phase=$(jq -r '.phase' "$PLAN_STATE")
fi
if [[ $plan_phase != materialized ]]; then
  printf '{}\n' >"$work/dry-run-body.json"
  request POST /v1/admin/pricing-stage5-v2/dry-run "$work/dry-run-body.json" "$work/dry-run.json"
  stage5_result_is_valid dry_run dry_run "$work/dry-run.json" \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 5 dry-run response\n' >&2; exit 1; }
  blocker_count=$(jq -r '.blocker_count' "$work/dry-run.json")
  if (( blocker_count != 0 )); then
    jq -c '{state:"blocked",plan_digest,target_generation,recovery_generation,blocker_count,
      blockers:[.blockers[]|{blocker_code,blocker_context,blocker_digest}]}' "$work/dry-run.json"
    unset COMMERCIAL_ADMIN_KEY
    exit 1
  fi
  jq -e '
    .writes_committed == false and .engine_prepared == false and .run_id == null and
    .engine_scan_first_digest == .engine_scan_second_digest and
    .openkeys_scan_first_digest == .openkeys_scan_second_digest
  ' "$work/dry-run.json" >/dev/null \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 5 dry-run is not stable and side-effect free\n' >&2; exit 1; }
  plan_candidate=$(mktemp "$STATE_DIR/.plan.json.XXXXXX")
  jq -c --arg sha "$ADMISSION_SHA" '{
    admission_sha:$sha,phase:"planned",plan_digest,target_generation,target_plan_digest,
    recovery_generation,recovery_plan_digest
  }' "$work/dry-run.json" >"$plan_candidate"
  chmod 0600 "$plan_candidate"
  mv -- "$plan_candidate" "$PLAN_STATE"
  plan_state_is_valid \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'failed to persist exact pricing Stage 5/6 plan fence\n' >&2; exit 1; }
fi

plan_digest=$(jq -r '.plan_digest' "$PLAN_STATE")
query="/v1/admin/pricing-stage6-v2?plan_digest=$plan_digest"
if [[ $(jq -r '.phase' "$PLAN_STATE") == planned ]]; then
  jq -cn --arg plan_digest "$plan_digest" --arg reason "$REASON" \
    '{plan_digest:$plan_digest,reason:$reason}' >"$work/materialize-body.json"
  request POST /v1/admin/pricing-stage5-v2/materialize "$work/materialize-body.json" "$work/materialize.json"
  stage5_result_is_valid apply materializing "$work/materialize.json" \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 5 materialize response\n' >&2; exit 1; }
  jq -e --slurpfile plan "$PLAN_STATE" '
    .plan_digest == $plan[0].plan_digest and
    .target_generation == $plan[0].target_generation and
    .target_plan_digest == $plan[0].target_plan_digest and
    .recovery_generation == $plan[0].recovery_generation and
    .recovery_plan_digest == $plan[0].recovery_plan_digest and
    .writes_committed == true and .engine_prepared == true and .run_id != null and
    .blocker_count == 0 and (.blockers | length) == 0 and
    .engine_scan_first_digest == .engine_scan_second_digest and
    .openkeys_scan_first_digest == .openkeys_scan_second_digest
  ' "$work/materialize.json" >/dev/null \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 5 materialization is not fully ACKed\n' >&2; exit 1; }
  plan_candidate=$(mktemp "$STATE_DIR/.plan.json.XXXXXX")
  jq -c '.phase = "materialized"' "$PLAN_STATE" >"$plan_candidate"
  chmod 0600 "$plan_candidate"
  mv -- "$plan_candidate" "$PLAN_STATE"
  plan_state_is_valid \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'failed to advance pricing Stage 5/6 plan fence\n' >&2; exit 1; }
fi

request GET "$query" /dev/null "$work/stage6-before.json"
stage6_result_is_valid "$plan_digest" "$work/stage6-before.json" \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 6 pre-stage response (contract)\n' >&2; exit 1; }
stage6_identity_matches_plan "$work/stage6-before.json" \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 6 pre-stage response (plan_identity)\n' >&2; exit 1; }

if jq -e '.job_status == "dead" or .stage5_status == "failed" or .stage5_status == "blocked" or
    .target_status == "failed" or .recovery_status == "failed" or .blocker_accounts != 0' \
    "$work/stage6-before.json" >/dev/null; then
  jq -c '{state:"blocked",stage5_plan_digest,stage5_status,target_generation,target_status,
    recovery_generation,recovery_status,job_status,job_attempts,pending_accounts,
    processing_accounts,retry_accounts,ready_accounts,blocker_accounts}' "$work/stage6-before.json"
  unset COMMERCIAL_ADMIN_KEY
  exit 1
fi

if [[ $(jq -r '.job_status' "$work/stage6-before.json") == null ]]; then
  jq -e '
    .stage5_status == "materializing" and
    (.target_status | IN("planned", "materializing")) and
    (.recovery_status | IN("planned", "materializing")) and .blocker_accounts == 0
  ' "$work/stage6-before.json" >/dev/null \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 6 is not in a stageable state\n' >&2; exit 1; }
  jq -cn --arg plan_digest "$plan_digest" --arg reason "$REASON" \
    '{plan_digest:$plan_digest,reason:$reason}' >"$work/stage6-body.json"
  request POST /v1/admin/pricing-stage6-v2/stage "$work/stage6-body.json" "$work/stage6-staged.json"
  stage6_result_is_valid "$plan_digest" "$work/stage6-staged.json" \
    && stage6_identity_matches_plan "$work/stage6-staged.json" \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 6 staged response\n' >&2; exit 1; }
fi

for ((poll = 1; poll <= MAX_POLLS; poll++)); do
  request GET "$query" /dev/null "$work/stage6-status.json"
  stage6_result_is_valid "$plan_digest" "$work/stage6-status.json" \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 6 status response\n' >&2; exit 1; }
  if jq -e '.job_status == "dead" or .stage5_status == "failed" or
      .target_status == "failed" or .recovery_status == "failed" or .blocker_accounts != 0' \
      "$work/stage6-status.json" >/dev/null; then
    jq -c '{state:"blocked",stage5_plan_digest,stage5_status,target_generation,target_status,
      recovery_generation,recovery_status,job_status,job_attempts,pending_accounts,
      processing_accounts,retry_accounts,ready_accounts,blocker_accounts}' "$work/stage6-status.json"
    unset COMMERCIAL_ADMIN_KEY
    exit 1
  fi
  if jq -e '
      .stage5_status == "prepared" and .target_status == "prepared" and
      .recovery_status == "prepared" and .job_status == "confirmed" and
      .pending_accounts == 0 and .processing_accounts == 0 and .retry_accounts == 0 and
      .blocker_accounts == 0 and .ready_accounts > 0 and
      .target_release_digest != null and .recovery_release_digest != null and
      .job_result_digest != null and .target_funding_manifest_digest != null and
      .target_funding_manifest_digest == .recovery_funding_manifest_digest
    ' "$work/stage6-status.json" >/dev/null; then
    jq -c '{state:"green",stage5_plan_digest,stage5_status,target_generation,target_plan_digest,
      target_release_digest,target_status,recovery_generation,recovery_plan_digest,
      recovery_release_digest,recovery_status,job_status,job_attempts,job_result_digest,
      ready_accounts,target_funding_manifest_digest,recovery_funding_manifest_digest}' \
      "$work/stage6-status.json"
    unset COMMERCIAL_ADMIN_KEY
    exit 0
  fi
  sleep "$POLL_SECONDS"
done

unset COMMERCIAL_ADMIN_KEY
printf 'Stage 6 did not confirm within the bounded polling window\n' >&2
exit 1
