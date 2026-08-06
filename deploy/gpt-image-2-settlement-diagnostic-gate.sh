#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
DIAGNOSTIC_PRODUCER_SHA=ab3b4e557f7b870b93f62a88a53e87e46b49fb4c
FENCED_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223
EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke
OUTPUT=$EVIDENCE_PARENT/$FENCED_PRODUCER_SHA

[[ $# -eq 2 && $2 == --inspect ]] \
  || wd_die "usage: gpt-image-2-settlement-diagnostic-gate.sh <exact-diagnostic-producer-sha> --inspect"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$DIAGNOSTIC_PRODUCER_SHA" ]] \
  || wd_die "GPT Image 2 settlement diagnostic is pinned to producer $DIAGNOSTIC_PRODUCER_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 settlement diagnostic must run through the fixed root bridge"
command -v jq >/dev/null || wd_die "jq is required for GPT Image 2 settlement diagnostic"
command -v setpriv >/dev/null || wd_die "setpriv is required for GPT Image 2 settlement diagnostic"
command -v timeout >/dev/null || wd_die "timeout is required for GPT Image 2 settlement diagnostic"

path_is_private_deploy_directory() {
  local path=$1
  [[ -d $path && ! -L $path \
      && $(stat -c '%U:%G:%a' -- "$path" 2>/dev/null) == deploy:deploy:700 ]]
}

path_is_private_deploy_file() {
  local path=$1
  [[ -f $path && ! -L $path \
      && $(stat -c '%U:%G:%a' -- "$path" 2>/dev/null) == deploy:deploy:600 ]]
}

path_is_private_deploy_directory "$EVIDENCE_PARENT" \
  || wd_die "GPT Image 2 public paid evidence parent is not an actual private deploy directory"
path_is_private_deploy_directory "$OUTPUT" \
  || wd_die "GPT Image 2 public paid evidence root is not an actual private deploy directory"
journal=$OUTPUT/journal.json
generation=$OUTPUT/generation.png
path_is_private_deploy_file "$journal" \
  || wd_die "GPT Image 2 public paid fence has no private journal"
path_is_private_deploy_file "$generation" \
  || wd_die "GPT Image 2 public paid fence has no private generation"
mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
[[ ${#entries[@]} -eq 2 && ${entries[0]} == generation.png && ${entries[1]} == journal.json ]] \
  || wd_die "GPT Image 2 public paid fence has unexpected artifacts"

jq -e --arg sha "$FENCED_PRODUCER_SHA" '
  def uuid4:
    type == "string" and
    test("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
  (keys | sort) == ([
    "edit_dispatched", "edit_request_id", "generation_dispatched", "generation_request_id",
    "implementation_sha", "schema_version", "state"
  ] | sort) and
  .schema_version == 1 and .implementation_sha == $sha and
  .state == "generation_received" and
  .generation_dispatched == true and .edit_dispatched == false and
  (.generation_request_id | uuid4) and .edit_request_id == null
' "$journal" >/dev/null || wd_die "GPT Image 2 public paid fence is not the exact generation-only withdrawal"
request_id=$(jq -jr '.generation_request_id' "$journal")

release=$ENGINE_RELEASE_ROOT/$DIAGNOSTIC_PRODUCER_SHA
current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
[[ $current == "$release" ]] \
  || wd_die "GPT Image 2 settlement diagnostic requires current producer release $DIAGNOSTIC_PRODUCER_SHA"
binary=$release/claude-api
[[ -f $binary && ! -L $binary && -x $binary ]] \
  || wd_die "exact GPT Image 2 settlement diagnostic binary is missing"

load_database_url_from_openai_slot() {
  local unit pid entry name provider database_url
  for unit in claude-api-openai@8793.service claude-api-openai@8797.service; do
    [[ $(systemctl show "$unit" -p ActiveState --value 2>/dev/null) == active ]] || continue
    pid=$(systemctl show "$unit" -p MainPID --value 2>/dev/null)
    [[ $pid =~ ^[1-9][0-9]*$ ]] || continue
    provider=
    database_url=
    while IFS= read -r -d '' entry; do
      [[ $entry == *=* ]] || wd_die "running OpenAI slot contains an invalid environment entry"
      name=${entry%%=*}
      case "$name" in
        CLAUDE_API_PROVIDER) provider=${entry#*=} ;;
        CLAUDE_API_DATABASE_URL) database_url=${entry#*=} ;;
      esac
    done <"/proc/$pid/environ"
    [[ $provider == openai ]] || continue
    [[ -n $database_url ]] || wd_die "active OpenAI slot lacks the PostgreSQL authority URL"
    CLAUDE_API_DATABASE_URL=$database_url
    return 0
  done
  wd_die "no active OpenAI slot can supply the production PostgreSQL authority URL"
}

load_database_url_from_openai_slot
deploy_uid=$(id -u deploy)
deploy_gid=$(id -g deploy)
if ! diagnostic=$(printf '%s\n' "$request_id" | \
    timeout --signal=TERM --kill-after=5s 30s \
    setpriv --reuid="$deploy_uid" --regid="$deploy_gid" --init-groups --no-new-privs \
    env -i HOME=/home/deploy CLAUDE_API_DATABASE_URL="$CLAUDE_API_DATABASE_URL" \
    "$binary" openai-image-settlement-diagnostic 2>/dev/null); then
  unset request_id CLAUDE_API_DATABASE_URL
  wd_die "GPT Image 2 settlement diagnostic binary failed"
fi
unset request_id CLAUDE_API_DATABASE_URL

jq -e '
  def integer_between($low; $high):
    type == "number" and floor == . and . >= $low and . <= $high;
  def nullable_integer($low; $high):
    . == null or integer_between($low; $high);
  (keys | sort) == ([
    "account_present", "cache_read_nano", "cache_read_tokens", "charge_nano",
    "charged_hold_nano", "input_nano", "input_tokens", "key_present", "official_hold_nano",
    "outbox_attempts", "outbox_disposition", "outbox_has_error", "outbox_present",
    "outbox_state", "output_nano", "output_tokens", "priced_ts", "real_nano",
    "release_billing_mode", "release_generation", "reservation_actual_nano",
    "reservation_hold_nano", "reservation_present", "reservation_state", "schema_version",
    "snapshot_canonical_model", "snapshot_generation_controls", "snapshot_openai",
    "snapshot_present", "snapshot_requested_model", "snapshot_tariff", "status",
    "usage_model", "usage_openai", "usage_present"
  ] | sort) and
  .schema_version == 1 and
  ([.reservation_present, .snapshot_present, .snapshot_openai, .snapshot_canonical_model,
    .snapshot_tariff, .snapshot_requested_model, .snapshot_generation_controls,
    .outbox_present, .outbox_has_error, .usage_present, .usage_openai, .usage_model,
    .account_present, .key_present] | all(type == "boolean")) and
  (.reservation_state == null or
    (.reservation_state | IN("reserved", "delivering", "settlement_pending", "settled", "canceled"))) and
  (.reservation_hold_nano | nullable_integer(0; 1000000000000)) and
  (.reservation_actual_nano | nullable_integer(0; 1000000000000)) and
  (.release_generation | nullable_integer(1; 1000000000)) and
  (.release_billing_mode == null or
    (.release_billing_mode | IN("shadow", "strict", "meter_only"))) and
  (.official_hold_nano | nullable_integer(0; 1000000000000)) and
  (.charged_hold_nano | nullable_integer(0; 1000000000000)) and
  (.outbox_state == null or (.outbox_state | IN("pending", "processing", "done", "failed"))) and
  (.outbox_disposition == null or
    (.outbox_disposition | IN("settle", "cancel", "reconcile_full_hold"))) and
  (.outbox_attempts | nullable_integer(0; 1000000)) and
  (.input_tokens | nullable_integer(0; 100000000)) and
  (.output_tokens | nullable_integer(0; 100000000)) and
  (.cache_read_tokens | nullable_integer(0; 100000000)) and
  (.real_nano | nullable_integer(0; 1000000000000)) and
  (.charge_nano | nullable_integer(0; 1000000000000)) and
  (.input_nano | nullable_integer(0; 1000000000000)) and
  (.output_nano | nullable_integer(0; 1000000000000)) and
  (.cache_read_nano | nullable_integer(0; 1000000000000)) and
  (.priced_ts | nullable_integer(0; 9999999999)) and
  (.reservation_present == (.reservation_state != null)) and
  (if .reservation_present then .reservation_hold_nano != null
   else .reservation_hold_nano == null and .reservation_actual_nano == null end) and
  (.snapshot_present == (.release_generation != null)) and
  (if .snapshot_present then
     .release_billing_mode != null and .official_hold_nano != null and .charged_hold_nano != null
   else
     .release_billing_mode == null and .official_hold_nano == null and .charged_hold_nano == null and
     ([.snapshot_openai, .snapshot_canonical_model, .snapshot_tariff,
       .snapshot_requested_model, .snapshot_generation_controls] | all(. == false))
   end) and
  (.outbox_present == (.outbox_state != null)) and
  (if .outbox_present then .outbox_disposition != null and .outbox_attempts != null
   else .outbox_disposition == null and .outbox_attempts == null and .outbox_has_error == false end) and
  (if .usage_present then
     ([.input_tokens, .output_tokens, .cache_read_tokens, .real_nano, .charge_nano,
       .input_nano, .output_nano, .cache_read_nano, .priced_ts] | all(. != null))
   else
     ([.input_tokens, .output_tokens, .cache_read_tokens, .real_nano, .charge_nano,
       .input_nano, .output_nano, .cache_read_nano, .priced_ts] | all(. == null)) and
     .usage_openai == false and .usage_model == false
   end) and
  (if .reservation_present then true else .account_present == false and .key_present == false end) and
  (if .reservation_present | not then "reservation_missing"
   elif .snapshot_present | not then "snapshot_missing"
   elif .outbox_present | not then "outbox_missing"
   elif .outbox_state == "failed" then "outbox_failed"
   elif .outbox_state != "done" then "outbox_pending"
   elif .usage_present | not then "usage_missing"
   elif .reservation_state != "settled" then "reservation_nonterminal"
   elif (.account_present and .key_present) | not then "principal_missing"
   else "terminal_evidence_present" end) == .status
' <<<"$diagnostic" >/dev/null || wd_die "GPT Image 2 settlement diagnostic returned invalid evidence"

printf '%s\n' "$diagnostic"
