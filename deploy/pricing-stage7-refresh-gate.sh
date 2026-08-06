#!/usr/bin/env bash
set -euo pipefail

ADMISSION_SHA=3f412e33d631f2956a575e40f7f28f8b0b592106
PLAN_DIGEST=sha256:v2:7cea045492a514550f0c7eafa634c834de5f7890c9b7c07c3d8ad78c5f2e9653
TARGET_GENERATION=23
TARGET_PLAN_DIGEST=sha256:v2:261794d485a01a3e5b41be20d1c19cff4ea9d23423dba5b70dcef5b61e19bde7
TARGET_RELEASE_DIGEST=sha256:v2:bab313f8619d856de9073c13d6dabbf9663a59aa3e4200ab25f7758202b5892c
RECOVERY_GENERATION=24
RECOVERY_PLAN_DIGEST=sha256:v2:33e7566d44a969ad34bf3614e6e2e9f8288e6b8ac07f7a2f43bb6843528e3410
RECOVERY_RELEASE_DIGEST=sha256:v2:36c48d7c909882d8a01d47131a5e63ce04d0d5bdebbbefefc8fe04dfdcddd8b1
API_ORIGIN=http://127.0.0.1:8791
ACTOR=gpt-image-2-stage7-inventory-refresh
REASON='align exact OpenKeys pricing lineages after reviewed inventory refresh'
POLL_SECONDS=5
MAX_POLLS=360
STAGE56_PLAN=/var/lib/apitoken/pricing-stage56-inventory-refresh/$ADMISSION_SHA/plan.json
STATE_PARENT=/var/lib/apitoken/pricing-stage7-inventory-refresh
STATE_DIR=$STATE_PARENT/$ADMISSION_SHA
REQUEST_STATE=$STATE_DIR/request.json

[[ $# -eq 1 ]] || { printf 'usage: pricing-stage7-refresh-gate.sh <exact-admission-sha>\n' >&2; exit 2; }
[[ $1 == "$ADMISSION_SHA" ]] \
  || { printf 'pricing Stage 7 inventory refresh is pinned to %s\n' "$ADMISSION_SHA" >&2; exit 1; }
[[ $(id -u) == 0 ]] \
  || { printf 'pricing Stage 7 inventory refresh must run through the fixed root bridge\n' >&2; exit 1; }
command -v curl >/dev/null || { printf 'curl is required\n' >&2; exit 1; }
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 1; }
command -v flock >/dev/null || { printf 'flock is required\n' >&2; exit 1; }

work=$(mktemp -d /run/pricing-stage7-inventory-refresh.XXXXXX)
chmod 0700 "$work"
trap 'rm -rf -- "$work"' EXIT

[[ ! -L $STATE_PARENT && ! -L $STATE_DIR ]] \
  || { printf 'pricing Stage 7 state path must not be a symlink\n' >&2; exit 1; }
install -d -o root -g root -m 0700 -- "$STATE_PARENT" "$STATE_DIR"
[[ $(stat -c '%U:%G:%a' -- "$STATE_PARENT") == root:root:700 \
    && $(stat -c '%U:%G:%a' -- "$STATE_DIR") == root:root:700 ]] \
  || { printf 'pricing Stage 7 state directories are not private root authority\n' >&2; exit 1; }
exec 9>"$STATE_DIR/lock"
chmod 0600 "$STATE_DIR/lock"
flock -n 9 || { printf 'another pricing Stage 7 admission is active\n' >&2; exit 1; }

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

stage7_control_error_diagnostic() {
  local output=$1 http_code=$2 code
  code=$(jq -er --argjson status "$http_code" '
    if type == "object" and
       (keys | sort) == (["code", "message", "statusCode"] | sort) and
       .statusCode == $status and
       (.code | type == "string") and
       (($status == 409 and .message == "pricing shadow rollout conflicts with durable authority") or
        ($status == 503 and .message == "pricing shadow rollout authority is temporarily unavailable"))
    then .code else empty end
  ' "$output" 2>/dev/null) || { printf 'unclassified\n'; return; }
  case "$http_code:$code" in
    409:stage5_run_missing|409:stage5_run_not_prepared|409:engine_inventory_drift|\
    409:release_plan_missing|409:release_plan_not_prepared|409:release_inventory_drift|\
    409:stage5_artifact_invalid|409:target_assignments_missing|409:target_assignments_drift|\
    409:release_policy_invalid|409:openkeys_owner_missing|409:openkeys_multiplier_drift|\
    409:openkeys_lineage_missing|409:openkeys_lock_drift|409:idempotency_conflict|\
    409:shadow_rollout_conflict|503:engine_inventory_unavailable)
      printf '%s\n' "$code"
      ;;
    *)
      printf 'unclassified\n'
      ;;
  esac
}

request() {
  local method=$1 path=$2 body_file=$3 output=$4 expected_codes=${5:-200 201} http_code diagnostic
  case "$method:$path" in
    GET:/v1/admin/pricing-stage5-v2\?plan_digest=sha256:v2:[0-9a-f]*|\
    GET:/v1/admin/pricing-shadow-rollout-v2|\
    POST:/v1/admin/pricing-shadow-rollout-v2/stage) ;;
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
    diagnostic=$(stage7_control_error_diagnostic "$output" "$http_code")
    printf 'pricing control request returned HTTP %s (%s)\n' "$http_code" "$diagnostic" >&2
    return 1
  }
}

stage5_run_is_valid() {
  local file=$1
  jq -e \
    --arg plan "$PLAN_DIGEST" \
    --arg target_plan "$TARGET_PLAN_DIGEST" \
    --arg target_release "$TARGET_RELEASE_DIGEST" \
    --arg recovery_plan "$RECOVERY_PLAN_DIGEST" \
    --arg recovery_release "$RECOVERY_RELEASE_DIGEST" \
    --arg target "$TARGET_GENERATION" \
    --arg recovery "$RECOVERY_GENERATION" '
    def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
    (keys | sort) == ([
      "blocker_count", "plan_digest", "recovery_generation", "recovery_plan_digest",
      "recovery_release_digest", "run_id", "status", "target_generation",
      "target_plan_digest", "target_release_digest"
    ] | sort) and
    .plan_digest == $plan and .status == "prepared" and (.run_id | uuid) and
    .target_generation == $target and .target_plan_digest == $target_plan and
    .target_release_digest == $target_release and .recovery_generation == $recovery and
    .recovery_plan_digest == $recovery_plan and .recovery_release_digest == $recovery_release and
    .blocker_count == "0"
  ' "$file" >/dev/null
}

stage_response_is_valid() {
  local file=$1
  jq -e '
    def digest: type == "string" and test("^sha256:v2:[0-9a-f]{64}$");
    def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
    (keys | sort) == (["idempotent_replay", "job_count", "rollout_digest", "rollout_id", "status"] | sort) and
    (.rollout_id | uuid) and (.rollout_digest | digest) and
    (.job_count | type == "number" and floor == . and . > 0) and
    (.idempotent_replay | type == "boolean") and .status == "accepted"
  ' "$file" >/dev/null
}

stage7_control_is_valid() {
  local file=$1
  jq -e '
    def digest_v2: type == "string" and test("^sha256:v2:[0-9a-f]{64}$");
    def digest_any: type == "string" and test("^sha256:v[12]:[0-9a-f]{64}$");
    def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
    def timestamp: type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T");
    def count: type == "number" and floor == . and . >= 0;
    def rollout_status: IN("pending", "processing", "confirmed", "blocked", "dead");
    def job_status: IN("pending", "processing", "retry", "confirmed", "blocked", "dead");
    (keys | sort) == (["counts_by_status", "database_observed_at", "jobs", "rollouts"] | sort) and
    (.database_observed_at | timestamp) and
    (.counts_by_status | type == "object" and
      ((keys - ["pending", "processing", "confirmed", "blocked", "dead"]) | length) == 0 and
      all(.[]; count)) and
    (.rollouts | type == "array" and length <= 100 and all(.[];
      (keys | sort) == ([
        "actor_id", "assignment_count", "assignment_manifest_digest", "catalog_generation",
        "completed_at", "created_at", "engine_inventory_digest", "id", "idempotency_key",
        "job_count", "job_counts_by_status", "last_error", "main_catalog_digest",
        "openkeys_catalog_digest", "policy_manifest_digest", "reason", "recovery_digest",
        "recovery_generation", "rollout_digest", "stage5_run_id", "status", "switch_digest",
        "switch_generation", "target_digest", "target_generation", "updated_at"
      ] | sort) and
      (.id | uuid) and (.idempotency_key | uuid) and (.stage5_run_id | uuid) and
      (.rollout_digest | digest_v2) and (.target_digest | digest_v2) and
      (.recovery_digest | digest_v2) and (.engine_inventory_digest | digest_v2) and
      (.assignment_manifest_digest | digest_v2) and (.policy_manifest_digest | digest_v2) and
      (.main_catalog_digest | digest_any) and (.openkeys_catalog_digest | digest_any) and
      (.switch_digest | digest_any) and
      ([.target_generation, .recovery_generation, .catalog_generation, .switch_generation] |
        all(type == "string" and test("^[1-9][0-9]*$"))) and
      ([.assignment_count, .job_count] | all(type == "string" and test("^(0|[1-9][0-9]*)$"))) and
      (.job_counts_by_status | type == "object" and
        ((keys - ["pending", "processing", "retry", "confirmed", "blocked", "dead"]) | length) == 0 and
        all(.[]; count)) and
      (.actor_id | type == "string" and length >= 1 and length <= 200) and
      (.reason | type == "string") and (.status | rollout_status) and
      (.last_error == null or (.last_error | type == "string")) and
      (.completed_at == null or (.completed_at | timestamp)) and
      (.created_at | timestamp) and (.updated_at | timestamp)
    )) and
    (.jobs | type == "array" and length <= 100 and all(.[];
      (keys | sort) == ([
        "account_class", "account_status", "ack_digest", "attempts", "completed_at",
        "confirmed_at", "content_digest", "created_at", "expected_active_digest", "id",
        "last_error", "owner_context", "release_policy_digest", "request_digest", "rollout_id",
        "status", "subject_digest", "updated_at"
      ] | sort) and
      (.id | uuid) and (.rollout_id | uuid) and (.subject_digest | digest_v2) and
      (.account_status | IN("active", "disabled")) and
      (.account_class | IN("b2c", "b2b", "openkeys", "service")) and
      (.owner_context | IN("commerce", "openkeys", "service")) and
      (.release_policy_digest | digest_v2) and (.content_digest | digest_v2) and
      (.expected_active_digest == null or (.expected_active_digest | digest_any)) and
      (.request_digest | digest_v2) and (.status | job_status) and (.attempts | count) and
      (.last_error == null or (.last_error | type == "string")) and
      (.ack_digest == null or (.ack_digest | digest_v2)) and
      (.confirmed_at == null or (.confirmed_at | timestamp)) and
      (.completed_at == null or (.completed_at | timestamp)) and
      (.created_at | timestamp) and (.updated_at | timestamp)
    ))
  ' "$file" >/dev/null
}

stage56_plan_is_valid() {
  [[ -f $STAGE56_PLAN && ! -L $STAGE56_PLAN \
      && $(stat -c '%U:%G:%a' -- "$STAGE56_PLAN") == root:root:600 ]] || return 1
  jq -e \
    --arg sha "$ADMISSION_SHA" \
    --arg plan "$PLAN_DIGEST" \
    --arg target_plan "$TARGET_PLAN_DIGEST" \
    --arg recovery_plan "$RECOVERY_PLAN_DIGEST" \
    --argjson target "$TARGET_GENERATION" \
    --argjson recovery "$RECOVERY_GENERATION" '
    (keys | sort) == ([
      "admission_sha", "phase", "plan_digest", "recovery_generation", "recovery_plan_digest",
      "target_generation", "target_plan_digest"
    ] | sort) and
    .admission_sha == $sha and .phase == "materialized" and .plan_digest == $plan and
    .target_generation == $target and .target_plan_digest == $target_plan and
    .recovery_generation == $recovery and .recovery_plan_digest == $recovery_plan
  ' "$STAGE56_PLAN" >/dev/null
}

request_state_is_valid() {
  jq -e --arg sha "$ADMISSION_SHA" --arg run_id "$stage5_run_id" '
    def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
    (keys | sort) == (["admission_sha", "idempotency_key", "stage5_run_id"] | sort) and
    .admission_sha == $sha and .stage5_run_id == $run_id and (.idempotency_key | uuid)
  ' "$REQUEST_STATE" >/dev/null
}

stage56_plan_is_valid \
  || { printf 'exact GREEN Stage 5/6 plan fence is unavailable or invalid\n' >&2; exit 1; }
load_admin_key_from_commerce_slot
request GET "/v1/admin/pricing-stage5-v2?plan_digest=$PLAN_DIGEST" /dev/null "$work/stage5-run.json"
stage5_run_is_valid "$work/stage5-run.json" \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid exact terminal Stage 5 run\n' >&2; exit 1; }
stage5_run_id=$(jq -r '.run_id' "$work/stage5-run.json")

if [[ -e $REQUEST_STATE || -L $REQUEST_STATE ]]; then
  [[ -f $REQUEST_STATE && ! -L $REQUEST_STATE \
      && $(stat -c '%U:%G:%a' -- "$REQUEST_STATE") == root:root:600 ]] \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'pricing Stage 7 request fence is not private root authority\n' >&2; exit 1; }
  request_state_is_valid \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'pricing Stage 7 request fence is invalid\n' >&2; exit 1; }
else
  idempotency_key=$(< /proc/sys/kernel/random/uuid)
  [[ $idempotency_key =~ ^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$ ]] \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'kernel did not supply a valid idempotency UUID\n' >&2; exit 1; }
  state_candidate=$(mktemp "$STATE_DIR/.request.json.XXXXXX")
  jq -cn --arg sha "$ADMISSION_SHA" --arg key "$idempotency_key" --arg run_id "$stage5_run_id" \
    '{admission_sha:$sha,idempotency_key:$key,stage5_run_id:$run_id}' >"$state_candidate"
  chmod 0600 "$state_candidate"
  mv -- "$state_candidate" "$REQUEST_STATE"
  request_state_is_valid \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'failed to persist exact pricing Stage 7 request fence\n' >&2; exit 1; }
fi
idempotency_key=$(jq -r '.idempotency_key' "$REQUEST_STATE")
jq -cn --arg key "$idempotency_key" --arg run_id "$stage5_run_id" --arg reason "$REASON" \
  '{idempotency_key:$key,stage5_run_id:$run_id,reason:$reason}' >"$work/stage-body.json"
request POST /v1/admin/pricing-shadow-rollout-v2/stage "$work/stage-body.json" "$work/staged.json"
stage_response_is_valid "$work/staged.json" \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 7 staging response\n' >&2; exit 1; }
rollout_id=$(jq -r '.rollout_id' "$work/staged.json")
rollout_digest=$(jq -r '.rollout_digest' "$work/staged.json")
staged_job_count=$(jq -r '.job_count' "$work/staged.json")

for ((poll = 1; poll <= MAX_POLLS; poll++)); do
  request GET /v1/admin/pricing-shadow-rollout-v2 /dev/null "$work/status.json"
  stage7_control_is_valid "$work/status.json" \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid Stage 7 status response\n' >&2; exit 1; }
  matching_rollouts=$(jq --arg id "$rollout_id" '[.rollouts[] | select(.id == $id)] | length' "$work/status.json")
  (( matching_rollouts == 1 )) \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'exact Stage 7 rollout is absent from bounded control status\n' >&2; exit 1; }
  jq -e --arg id "$rollout_id" --arg run_id "$stage5_run_id" --arg digest "$rollout_digest" \
    --arg actor "$ACTOR" --arg reason "$REASON" --arg target "$TARGET_GENERATION" \
    --arg target_digest "$TARGET_RELEASE_DIGEST" --arg recovery "$RECOVERY_GENERATION" \
    --arg recovery_digest "$RECOVERY_RELEASE_DIGEST" --arg jobs "$staged_job_count" '
    .rollouts[] | select(.id == $id) |
    .stage5_run_id == $run_id and .rollout_digest == $digest and .actor_id == $actor and
    .reason == $reason and .target_generation == $target and .target_digest == $target_digest and
    .recovery_generation == $recovery and .recovery_digest == $recovery_digest and
    .job_count == $jobs and (.assignment_count | test("^[1-9][0-9]*$"))
  ' "$work/status.json" >/dev/null \
    || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 7 rollout identity drifted\n' >&2; exit 1; }
  status=$(jq -r --arg id "$rollout_id" '.rollouts[] | select(.id == $id) | .status' "$work/status.json")
  if [[ $status == blocked || $status == dead ]]; then
    jq -c --arg id "$rollout_id" '.rollouts[] | select(.id == $id) | {
      state:"blocked",rollout_digest,target_generation,target_digest,recovery_generation,
      recovery_digest,assignment_count,job_count,status,job_counts_by_status,completed_at
    }' "$work/status.json"
    unset COMMERCIAL_ADMIN_KEY
    exit 1
  fi
  if [[ $status == confirmed ]]; then
    jq -e --arg id "$rollout_id" '
      .rollouts[] | select(.id == $id) |
      (.job_count | tonumber) == (.job_counts_by_status.confirmed // 0) and
      (["pending", "processing", "retry", "blocked", "dead"] |
        all(. as $state | (.job_counts_by_status[$state] // 0) == 0)) and
      .completed_at != null
    ' "$work/status.json" >/dev/null \
      || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 7 confirmed without complete job ACKs\n' >&2; exit 1; }
    jq -e --arg id "$rollout_id" '
      [.jobs[] | select(.rollout_id == $id)] |
      all(.[]; .status == "confirmed" and .ack_digest != null and .confirmed_at != null)
    ' "$work/status.json" >/dev/null \
      || { unset COMMERCIAL_ADMIN_KEY; printf 'Stage 7 bounded job evidence lacks an ACK\n' >&2; exit 1; }
    jq -c --arg id "$rollout_id" '.rollouts[] | select(.id == $id) | {
      state:"green",rollout_digest,target_generation,target_digest,recovery_generation,
      recovery_digest,assignment_count,job_count,status,job_counts_by_status,completed_at
    }' "$work/status.json"
    unset COMMERCIAL_ADMIN_KEY
    exit 0
  fi
  sleep "$POLL_SECONDS"
done

unset COMMERCIAL_ADMIN_KEY
printf 'Stage 7 did not confirm within the bounded polling window\n' >&2
exit 1
