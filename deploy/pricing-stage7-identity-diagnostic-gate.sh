#!/usr/bin/env bash
set -euo pipefail

ADMISSION_SHA=3f412e33d631f2956a575e40f7f28f8b0b592106
CONVERGE_ACTOR=gpt-image-2-stage567-converge
CONVERGE_REASON='converge GPT Image 2 pricing admission against current engine inventory'
STATE_PARENT=/var/lib/apitoken/pricing-stage567-converge
API_ORIGIN=http://127.0.0.1:8791
MAX_CYCLES=3

[[ $# -eq 2 && $2 == --inspect ]] \
  || { printf 'usage: pricing-stage7-identity-diagnostic-gate.sh <exact-admission-sha> --inspect\n' >&2; exit 2; }
[[ $1 == "$ADMISSION_SHA" ]] \
  || { printf 'pricing Stage 7 identity diagnostic is pinned to %s\n' "$ADMISSION_SHA" >&2; exit 1; }
[[ $(id -u) == 0 ]] \
  || { printf 'pricing Stage 7 identity diagnostic must run through the fixed root bridge\n' >&2; exit 1; }
command -v curl >/dev/null || { printf 'curl is required\n' >&2; exit 1; }
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 1; }

work=$(mktemp -d /run/pricing-stage7-identity-diagnostic.XXXXXX)
chmod 0700 "$work"
trap 'rm -rf -- "$work"' EXIT

# The diagnostic is read-only: it never takes the convergence lock and never writes cycle state.
[[ ! -L $STATE_PARENT ]] \
  || { printf 'pricing Stage 7 convergence state path must not be a symlink\n' >&2; exit 1; }
[[ -d $STATE_PARENT && $(stat -c '%U:%G:%a' -- "$STATE_PARENT") == root:root:700 ]] \
  || { printf 'pricing Stage 7 convergence state is not private root authority\n' >&2; exit 1; }

cycle=$(mktemp "$work/.cycle.XXXXXX")
: >"$cycle"
for ((c = 1; c <= MAX_CYCLES; c++)); do
  cycle_dir=$STATE_PARENT/cycle-$c
  # Later cycles legitimately do not exist until an earlier one drifts; only an
  # existing-but-untrusted cycle path is a fail-closed condition.
  if [[ ! -e $cycle_dir && ! -L $cycle_dir ]]; then
    continue
  fi
  [[ ! -L $cycle_dir ]] \
    || { printf 'pricing convergence cycle path must not be a symlink\n' >&2; exit 1; }
  [[ -d $cycle_dir && $(stat -c '%U:%G:%a' -- "$cycle_dir") == root:root:700 ]] \
    || { printf 'pricing convergence cycle %s state is not private root authority\n' "$c" >&2; exit 1; }
  stage6_result=$cycle_dir/stage6-result.json
  request_state=$cycle_dir/stage7/$ADMISSION_SHA/request.json
  if [[ -e $stage6_result || -L $stage6_result ]]; then
    [[ -f $stage6_result && ! -L $stage6_result \
        && $(stat -c '%U:%G:%a' -- "$stage6_result") == root:root:600 ]] \
      || { printf 'terminal Stage 6 result in cycle %s is not private root authority\n' "$c" >&2; exit 1; }
    jq -e '
      .state == "green" and
      ([.stage5_plan_digest,.target_plan_digest,.target_release_digest,
        .recovery_plan_digest,.recovery_release_digest] |
        all(type == "string" and test("^sha256:v2:[0-9a-f]{64}$"))) and
      ([.target_generation,.recovery_generation] |
        all(type == "string" and test("^[1-9][0-9]*$")))
    ' "$stage6_result" >/dev/null \
      || { printf 'terminal Stage 6 result in cycle %s has an invalid contract\n' "$c" >&2; exit 1; }
  fi
  if [[ -e $request_state || -L $request_state ]]; then
    [[ -f $request_state && ! -L $request_state \
        && $(stat -c '%U:%G:%a' -- "$request_state") == root:root:600 ]] \
      || { printf 'pricing Stage 7 request fence in cycle %s is not private root authority\n' "$c" >&2; exit 1; }
    jq -e --arg sha "$ADMISSION_SHA" '
      def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
      (keys | sort) == (["admission_sha", "idempotency_key", "stage5_run_id"] | sort) and
      .admission_sha == $sha and (.idempotency_key | uuid) and (.stage5_run_id | uuid)
    ' "$request_state" >/dev/null \
      || { printf 'pricing Stage 7 request fence in cycle %s is invalid\n' "$c" >&2; exit 1; }
  fi
  if [[ -f $stage6_result && -f $request_state ]]; then
    printf '%s\n' "$c" >"$cycle"
  fi
done

cycle=$(< "$cycle")
[[ -n $cycle ]] \
  || { printf 'no converged cycle carries both terminal Stage 6 result and Stage 7 request fence\n' >&2; exit 1; }
cycle_dir=$STATE_PARENT/cycle-$cycle
stage6_result=$cycle_dir/stage6-result.json
request_state=$cycle_dir/stage7/$ADMISSION_SHA/request.json

stage5_plan_digest=$(jq -r '.stage5_plan_digest' "$stage6_result")
target_generation=$(jq -r '.target_generation' "$stage6_result")
target_digest=$(jq -r '.target_release_digest' "$stage6_result")
recovery_generation=$(jq -r '.recovery_generation' "$stage6_result")
recovery_digest=$(jq -r '.recovery_release_digest' "$stage6_result")
request_stage5_run_id=$(jq -r '.stage5_run_id' "$request_state")
request_idempotency_key=$(jq -r '.idempotency_key' "$request_state")

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

control_request() {
  local method=$1 path=$2 output=$3
  case "$method:$path" in
    GET:/v1/admin/pricing-stage5-v2\?plan_digest=sha256:v2:[0-9a-f]*|\
    GET:/v1/admin/pricing-shadow-rollout-v2) ;;
    *) printf 'refusing unexpected pricing control route\n' >&2; return 1 ;;
  esac
  local http_code
  http_code=$({
    printf 'url = "%s%s"\n' "$API_ORIGIN" "$path"
    printf 'request = "%s"\n' "$method"
    printf 'header = "content-type: application/json"\n'
    printf 'header = "x-admin-key: %s"\n' "$COMMERCIAL_ADMIN_KEY"
    printf 'header = "x-admin-actor: %s"\n' "$CONVERGE_ACTOR"
    printf 'output = "%s"\n' "$output"
    printf 'write-out = "%%{http_code}"\n'
    printf 'silent\nshow-error\nmax-time = 30\n'
  } | curl --config -) || {
    printf 'pricing control request failed with HTTP %s\n' "${http_code:-unreachable}" >&2
    return 1
  }
  chmod 0600 "$output"
  [[ $http_code == 200 ]] \
    || { printf 'pricing control request returned HTTP %s\n' "$http_code" >&2; return 1; }
}

control_is_valid() {
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

load_admin_key_from_commerce_slot
control_request GET "/v1/admin/pricing-stage5-v2?plan_digest=$stage5_plan_digest" "$work/stage5-run.json"
jq -e '
  def uuid: type == "string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
  (keys | sort) == ([
    "blocker_count", "plan_digest", "recovery_generation", "recovery_plan_digest",
    "recovery_release_digest", "run_id", "status", "target_generation",
    "target_plan_digest", "target_release_digest"
  ] | sort) and .plan_digest == $plan and .status == "prepared" and (.run_id | uuid)
' --arg plan "$stage5_plan_digest" "$work/stage5-run.json" >/dev/null \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid exact terminal Stage 5 run\n' >&2; exit 1; }
stage5_run_id=$(jq -r '.run_id' "$work/stage5-run.json")
control_request GET /v1/admin/pricing-shadow-rollout-v2 "$work/control.json"
control_is_valid "$work/control.json" \
  || { unset COMMERCIAL_ADMIN_KEY; printf 'invalid bounded Stage 7 control response\n' >&2; exit 1; }
unset COMMERCIAL_ADMIN_KEY

jq -c \
  --arg cycle "$cycle" \
  --arg run_id "$stage5_run_id" \
  --arg request_run_id "$request_stage5_run_id" \
  --arg key "$request_idempotency_key" \
  --arg actor "$CONVERGE_ACTOR" \
  --arg reason "$CONVERGE_REASON" \
  --arg target "$target_generation" \
  --arg target_digest "$target_digest" \
  --arg recovery "$recovery_generation" \
  --arg recovery_digest "$recovery_digest" '
  . as $control |
  ($control.rollouts | map(select(.idempotency_key == $key))) as $matched |
  (if ($matched | length) == 0 then []
   else ($control.rollouts | map(select(.rollout_digest == $matched[0].rollout_digest
       and .stage5_run_id == $request_run_id)))
   end) as $same_digest |
  {
    state: "report",
    cycle: $cycle,
    stage5_run_matches_request_fence: ($run_id == $request_run_id),
    stage5_status: "prepared",
    expected: {
      target_generation: $target,
      target_digest: $target_digest,
      recovery_generation: $recovery,
      recovery_digest: $recovery_digest
    },
    candidate: ($matched[0] | if . then {
      present: true,
      status: .status,
      completed_at: (.completed_at != null),
      target_generation: .target_generation,
      target_digest: .target_digest,
      recovery_generation: .recovery_generation,
      recovery_digest: .recovery_digest,
      assignment_count: .assignment_count,
      job_count: .job_count,
      job_counts_by_status: .job_counts_by_status
    } else { present: false } end),
    identity_checks: ($matched[0] | if . then {
      stage5_run_id: (.stage5_run_id == $run_id),
      idempotency_key: (.idempotency_key == $key),
      actor_id: (.actor_id == $actor),
      reason: (.reason == $reason),
      target_generation: (.target_generation == $target),
      target_digest: (.target_digest == $target_digest),
      recovery_generation: (.recovery_generation == $recovery),
      recovery_digest: (.recovery_digest == $recovery_digest),
      assignment_nonzero: (.assignment_count | test("^[1-9][0-9]*$"))
    } else {} end),
    same_digest_replay: {
      rollout_count: ($same_digest | length),
      rollouts: ($same_digest | map({
        status: .status,
        actor_is_converge: (.actor_id == $actor),
        reason_matches: (.reason == $reason),
        idempotency_key_matches: (.idempotency_key == $key),
        confirmed_acks_complete: (([.job_counts_by_status.pending // 0, .job_counts_by_status.processing // 0,
          .job_counts_by_status.retry // 0, .job_counts_by_status.blocked // 0, .job_counts_by_status.dead // 0]
          | add) == 0 and ((.job_counts_by_status.confirmed // 0) | tonumber) == (.job_count | tonumber))
      })),
      all_confirmed: (($same_digest | length) > 0 and ($same_digest | all(.status == "confirmed")))
    },
    rollouts_total: ($control.rollouts | length),
    counts_by_status: $control.counts_by_status,
    drifted_fields: (($matched[0] | if . then
      ($matched[0]) as $candidate |
      (["stage5_run_id", "idempotency_key", "actor_id", "reason", "target_generation",
        "target_digest", "recovery_generation", "recovery_digest", "assignment_nonzero"]
      | map(select((if . == "stage5_run_id" then ($candidate.stage5_run_id == $run_id)
          elif . == "idempotency_key" then ($candidate.idempotency_key == $key)
          elif . == "actor_id" then ($candidate.actor_id == $actor)
          elif . == "reason" then ($candidate.reason == $reason)
          elif . == "target_generation" then ($candidate.target_generation == $target)
          elif . == "target_digest" then ($candidate.target_digest == $target_digest)
          elif . == "recovery_generation" then ($candidate.recovery_generation == $recovery)
          elif . == "recovery_digest" then ($candidate.recovery_digest == $recovery_digest)
          else ($candidate.assignment_count | test("^[1-9][0-9]*$")) end) | not)))
    else [] end))
  }
' "$work/control.json"
