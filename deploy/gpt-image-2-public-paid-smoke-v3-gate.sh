#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
PRODUCER_SHA=8b68d73a2a6ba6ffae2f24692b283059f15b7c63
EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke-v3
OUTPUT=$EVIDENCE_PARENT/$PRODUCER_SHA

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-public-paid-smoke-v3-gate.sh <exact-producer-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$PRODUCER_SHA" ]] \
  || wd_die "GPT Image 2 public paid smoke v3 is pinned to producer $PRODUCER_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 public paid smoke must run through the fixed root bridge"
command -v jq >/dev/null || wd_die "jq is required for GPT Image 2 public paid evidence"
command -v sha256sum >/dev/null || wd_die "sha256sum is required for GPT Image 2 public paid evidence"

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

png_dimensions() {
  local path=$1 signature width_bytes height_bytes
  signature=$(od -An -tx1 -N8 -- "$path" | tr -d ' \n')
  [[ $signature == 89504e470d0a1a0a ]] || return 1
  read -r -a width_bytes <<<"$(od -An -tu1 -j16 -N4 -- "$path")"
  read -r -a height_bytes <<<"$(od -An -tu1 -j20 -N4 -- "$path")"
  [[ ${#width_bytes[@]} -eq 4 && ${#height_bytes[@]} -eq 4 ]] || return 1
  PNG_WIDTH=$((width_bytes[0] * 16777216 + width_bytes[1] * 65536 + width_bytes[2] * 256 + width_bytes[3]))
  PNG_HEIGHT=$((height_bytes[0] * 16777216 + height_bytes[1] * 65536 + height_bytes[2] * 256 + height_bytes[3]))
  (( PNG_WIDTH >= 1 && PNG_HEIGHT >= 1 ))
}

verify_png() {
  local path=$1 operation=$2 evidence=$3 bytes digest expected_digest expected_width expected_height
  path_is_private_deploy_file "$path" || return 1
  bytes=$(stat -c '%s' -- "$path") || return 1
  [[ $bytes =~ ^[1-9][0-9]*$ ]] || return 1
  (( bytes <= 16 * 1024 * 1024 )) || return 1
  png_dimensions "$path" || return 1
  expected_width=$(jq -r --arg operation "$operation" '.[$operation].width' "$evidence")
  expected_height=$(jq -r --arg operation "$operation" '.[$operation].height' "$evidence")
  [[ $PNG_WIDTH == "$expected_width" && $PNG_HEIGHT == "$expected_height" ]] || return 1
  [[ $(jq -r --arg operation "$operation" '.[$operation].png_bytes' "$evidence") == "$bytes" ]] \
    || return 1
  digest=$(sha256sum -- "$path" | awk '{print $1}') || return 1
  expected_digest=$(jq -r --arg operation "$operation" '.[$operation].png_sha256' "$evidence")
  [[ $expected_digest == "sha256:$digest" ]]
}

verify_success() {
  local evidence=$OUTPUT/evidence.json journal=$OUTPUT/journal.json
  local generation=$OUTPUT/generation.png edit=$OUTPUT/edit.png
  path_is_private_deploy_directory "$EVIDENCE_PARENT" || return 1
  path_is_private_deploy_directory "$OUTPUT" || return 1
  path_is_private_deploy_file "$evidence" || return 1
  path_is_private_deploy_file "$journal" || return 1

  mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
  [[ ${#entries[@]} -eq 4 \
      && ${entries[0]} == edit.png \
      && ${entries[1]} == evidence.json \
      && ${entries[2]} == generation.png \
      && ${entries[3]} == journal.json ]] || return 1

  jq -e --arg sha "$PRODUCER_SHA" '
    def uuid4:
      type == "string" and
      test("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$");
    def operation($name; $references; $image_input_required):
      (keys | sort) == ([
        "height", "png_bytes", "png_sha256", "request_id", "settlement", "usage", "width"
      ] | sort) and
      (.request_id | uuid4) and
      (.width | type == "number" and floor == . and . >= 1 and . <= 3840) and
      (.height | type == "number" and floor == . and . >= 1 and . <= 3840) and
      ((.width * .height) >= 655360 and (.width * .height) <= 8294400) and
      (.width <= 3 * .height and .height <= 3 * .width) and
      (.png_bytes | type == "number" and floor == . and . >= 1 and . <= 16777216) and
      (.png_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$")) and
      (.usage | keys | sort) == ([
        "input_tokens", "input_tokens_details", "output_tokens", "output_tokens_details", "total_tokens"
      ] | sort) and
      (.usage.input_tokens_details | keys | sort) == (["image_tokens", "text_tokens"] | sort) and
      (.usage.output_tokens_details | keys | sort) == ["image_tokens"] and
      ([.usage.input_tokens, .usage.input_tokens_details.text_tokens,
        .usage.input_tokens_details.image_tokens, .usage.output_tokens,
        .usage.output_tokens_details.image_tokens, .usage.total_tokens] |
        all(.[]; type == "number" and floor == . and . >= 0)) and
      (.usage.input_tokens == (.usage.input_tokens_details.text_tokens + .usage.input_tokens_details.image_tokens)) and
      (.usage.output_tokens == .usage.output_tokens_details.image_tokens) and
      (.usage.output_tokens > 0) and
      (.usage.total_tokens == (.usage.input_tokens + .usage.output_tokens)) and
      (if $image_input_required then .usage.input_tokens_details.image_tokens > 0
       else .usage.input_tokens_details.image_tokens == 0 end) and
      (.settlement | keys | sort) == ([
        "account_id", "balance_nano", "cache_read_nano", "cache_read_tokens", "canonical_model_id",
        "charge_nano", "charged_hold_nano", "input_nano", "input_tokens", "key_id",
        "key_reserved_nano", "key_spent_nano", "model", "official_cost", "official_hold_nano",
        "outbox_disposition", "outbox_state", "output_nano", "output_tokens", "priced_ts", "provider",
        "provider_id", "real_nano", "release_billing_mode", "release_generation", "request_id",
        "reservation_actual_nano", "reservation_hold_nano", "reservation_state", "reserved_nano",
        "spent_nano", "tariff_schedule_id"
      ] | sort) and
      (.settlement.request_id == .request_id) and
      (.settlement.request_id | uuid4) and
      (.settlement.account_id | type == "string" and length > 0 and length <= 128) and
      (.settlement.key_id | type == "string" and length > 0 and length <= 128) and
      .settlement.reservation_state == "settled" and
      .settlement.reservation_hold_nano == 0 and
      .settlement.reservation_actual_nano == 0 and
      .settlement.outbox_state == "done" and
      .settlement.outbox_disposition == "settle" and
      (.settlement.release_generation | type == "number" and floor == . and . > 0) and
      .settlement.release_billing_mode == "meter_only" and
      .settlement.provider_id == "openai" and
      .settlement.provider == "openai" and
      .settlement.canonical_model_id == "gpt-image-2-2026-04-21" and
      .settlement.model == "gpt-image-2" and
      .settlement.tariff_schedule_id == "openai/gpt-image-2/2026-04-21/v1" and
      (.settlement.official_hold_nano | type == "number" and floor == . and . > 0) and
      .settlement.charged_hold_nano == 0 and
      .settlement.input_tokens == .usage.input_tokens and
      .settlement.output_tokens == .usage.output_tokens and
      .settlement.cache_read_tokens == 0 and
      .settlement.input_nano == ((.usage.input_tokens_details.text_tokens * 5000) +
        (.usage.input_tokens_details.image_tokens * 8000)) and
      .settlement.output_nano == (.usage.output_tokens * 30000) and
      .settlement.cache_read_nano == 0 and
      .settlement.real_nano == (.settlement.input_nano + .settlement.output_nano) and
      .settlement.real_nano > 0 and
      .settlement.charge_nano == 0 and
      (.settlement.priced_ts | type == "number" and floor == . and . > 0) and
      (.settlement.official_cost | keys | sort) == ([
        "alias_generation", "premium_modifiers", "requested_model_id"
      ] | sort) and
      .settlement.official_cost.alias_generation == 1 and
      .settlement.official_cost.requested_model_id == "gpt-image-2" and
      (.settlement.official_cost.premium_modifiers | keys | sort) == ([
        "background", "kind", "operation", "quality", "reference_count", "size"
      ] | sort) and
      .settlement.official_cost.premium_modifiers.kind == "openai_image_v1" and
      .settlement.official_cost.premium_modifiers.operation == $name and
      .settlement.official_cost.premium_modifiers.background == "opaque" and
      .settlement.official_cost.premium_modifiers.quality == "low" and
      .settlement.official_cost.premium_modifiers.size == "auto" and
      .settlement.official_cost.premium_modifiers.reference_count == $references;
    (keys | sort) == ([
      "account_balance_unchanged", "account_reserved_unchanged", "account_spent_unchanged",
      "discovery_hidden_before_publication", "edit", "generation", "implementation_sha",
      "key_reserved_unchanged", "key_spent_unchanged", "model", "origin", "schema_version", "state"
    ] | sort) and
    .schema_version == 1 and
    .state == "success" and
    .implementation_sha == $sha and
    .origin == "https://openai.api.apitoken.sale" and
    .model == "gpt-image-2" and
    .discovery_hidden_before_publication == true and
    .account_balance_unchanged == true and
    .account_spent_unchanged == true and
    .account_reserved_unchanged == true and
    .key_spent_unchanged == true and
    .key_reserved_unchanged == true and
    (.generation | operation("generation"; 0; false)) and
    (.edit | operation("edit"; 1; true)) and
    .generation.request_id != .edit.request_id and
    .generation.png_sha256 != .edit.png_sha256 and
    .generation.settlement.account_id == .edit.settlement.account_id and
    .generation.settlement.key_id == .edit.settlement.key_id and
    .generation.settlement.balance_nano == .edit.settlement.balance_nano and
    .generation.settlement.spent_nano == .edit.settlement.spent_nano and
    .generation.settlement.reserved_nano == .edit.settlement.reserved_nano and
    .generation.settlement.key_spent_nano == .edit.settlement.key_spent_nano and
    .generation.settlement.key_reserved_nano == .edit.settlement.key_reserved_nano
  ' "$evidence" >/dev/null || return 1

  jq -e --arg sha "$PRODUCER_SHA" \
    --arg generation "$(jq -r '.generation.request_id' "$evidence")" \
    --arg edit "$(jq -r '.edit.request_id' "$evidence")" '
      (keys | sort) == ([
        "edit_dispatched", "edit_request_id", "generation_dispatched", "generation_request_id",
        "implementation_sha", "schema_version", "state"
      ] | sort) and
      .schema_version == 1 and .state == "success" and .implementation_sha == $sha and
      .generation_dispatched == true and .edit_dispatched == true and
      .generation_request_id == $generation and .edit_request_id == $edit
    ' "$journal" >/dev/null || return 1

  verify_png "$generation" generation "$evidence" || return 1
  verify_png "$edit" edit "$evidence" || return 1
  ! cmp -s -- "$generation" "$edit" || return 1
  jq -cn --argjson evidence "$(<"$evidence")" '{
    state: "green",
    generation: {
      width: $evidence.generation.width,
      height: $evidence.generation.height,
      png_sha256: $evidence.generation.png_sha256,
      text_input_tokens: $evidence.generation.usage.input_tokens_details.text_tokens,
      image_input_tokens: $evidence.generation.usage.input_tokens_details.image_tokens,
      image_output_tokens: $evidence.generation.usage.output_tokens_details.image_tokens,
      real_nano: $evidence.generation.settlement.real_nano,
      charge_nano: $evidence.generation.settlement.charge_nano
    },
    edit: {
      width: $evidence.edit.width,
      height: $evidence.edit.height,
      png_sha256: $evidence.edit.png_sha256,
      text_input_tokens: $evidence.edit.usage.input_tokens_details.text_tokens,
      image_input_tokens: $evidence.edit.usage.input_tokens_details.image_tokens,
      image_output_tokens: $evidence.edit.usage.output_tokens_details.image_tokens,
      real_nano: $evidence.edit.settlement.real_nano,
      charge_nano: $evidence.edit.settlement.charge_nano
    }
  }'
}

journal_summary() {
  local journal=$OUTPUT/journal.json
  path_is_private_deploy_file "$journal" || return 1
  jq -er --arg sha "$PRODUCER_SHA" '
    def uuid4_or_null:
      . == null or (type == "string" and
        test("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"));
    . as $journal |
    ((keys | sort) == ([
      "edit_dispatched", "edit_request_id", "generation_dispatched", "generation_request_id",
      "implementation_sha", "schema_version", "state"
    ] | sort) and
    .schema_version == 1 and .implementation_sha == $sha and
    (.state | type == "string" and length >= 1 and length <= 64 and test("^[a-z_]+$")) and
    (.generation_dispatched | type == "boolean") and
    (.edit_dispatched | type == "boolean") and
    (.generation_request_id | uuid4_or_null) and
    (.edit_request_id | uuid4_or_null) and
    (if .edit_dispatched then .generation_dispatched else true end)) as $valid |
    if $valid then
      "gpt-image-paid:\($journal.state):g=\($journal.generation_dispatched):e=\($journal.edit_dispatched)"
    else error("invalid journal contract") end
  ' "$journal"
}

# The producer-SHA output is a permanent paid-dispatch fence. Re-entry may only verify complete
# success; it can never invoke the CLI again after any recorded attempt.
if [[ -e $OUTPUT || -L $OUTPUT ]]; then
  verify_success || {
    journal_summary || true
    wd_die "prior GPT Image 2 public paid smoke is fenced without exact success evidence"
  }
  exit 0
fi
if [[ -e $EVIDENCE_PARENT || -L $EVIDENCE_PARENT ]]; then
  path_is_private_deploy_directory "$EVIDENCE_PARENT" \
    || wd_die "GPT Image 2 public paid evidence parent is not an actual private deploy directory"
else
  install -d -o deploy -g deploy -m 0700 -- "$EVIDENCE_PARENT"
fi
path_is_private_deploy_directory "$EVIDENCE_PARENT" \
  || wd_die "GPT Image 2 public paid evidence parent is not private"

release=$ENGINE_RELEASE_ROOT/$PRODUCER_SHA
current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
[[ $current == "$release" ]] \
  || wd_die "GPT Image 2 public paid smoke requires current producer release $PRODUCER_SHA"
binary=$release/claude-api
[[ -f $binary && ! -L $binary && -x $binary ]] \
  || wd_die "exact GPT Image 2 public paid smoke binary is missing"

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
    export CLAUDE_API_DATABASE_URL
    return 0
  done
  wd_die "no active OpenAI slot can supply the production PostgreSQL authority URL"
}

# The existing service key is selected inside the exact binary from PostgreSQL. No credential is
# inherited, written to disk, placed in argv, or accepted as a controller argument.
load_database_url_from_openai_slot
deploy_uid=$(id -u deploy)
deploy_gid=$(id -g deploy)
if ! timeout --signal=TERM --kill-after=15s 2400s \
    setpriv --reuid="$deploy_uid" --regid="$deploy_gid" --init-groups --no-new-privs \
    env -i HOME=/home/deploy CLAUDE_API_DATABASE_URL="$CLAUDE_API_DATABASE_URL" \
    "$binary" openai-image-public-smoke --output "$OUTPUT" --execute \
    >/dev/null 2>/dev/null; then
  # As with free preflight, a completed verifier-grade result is authoritative if only process
  # teardown crossed the outer deadline. Any partial/unknown paid state remains permanently RED.
  if verify_success; then
    exit 0
  fi
  if path_is_private_deploy_directory "$OUTPUT"; then
    journal_summary || true
  fi
  wd_die "GPT Image 2 public paid smoke failed; replay is forbidden"
fi
verify_success || wd_die "GPT Image 2 public paid smoke returned without exact success evidence"
