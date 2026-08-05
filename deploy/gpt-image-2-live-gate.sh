#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
ENGINE_DATA_ROOT=/srv/claude-api/data
EXPECTED_IMPLEMENTATION_SHA=8fcd7c3c6f5dc968bedb7260433f2eaff23f8931
GENERATION_BUDGET_NANOUSD=8560000

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-live-gate.sh <exact-engine-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$EXPECTED_IMPLEMENTATION_SHA" ]] \
  || wd_die "GPT Image 2 generation gate is pinned to $EXPECTED_IMPLEMENTATION_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 generation gate must run through the fixed root bridge"

release=$ENGINE_RELEASE_ROOT/$SHA
current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
[[ $current == "$release" ]] || wd_die "GPT Image 2 gate requires current engine release $SHA"
binary=$release/claude-api
[[ -f $binary && ! -L $binary && -x $binary ]] \
  || wd_die "exact GPT Image 2 canary binary is missing"

root=$STATE_ROOT/gpt-image-2-live/$SHA
[[ ! -L $STATE_ROOT/gpt-image-2-live ]] || wd_die "GPT Image 2 evidence root must not be a symlink"
install -d -o deploy -g deploy -m 0700 -- "$STATE_ROOT/gpt-image-2-live" "$root"
[[ -d $root && ! -L $root && $(stat -c '%U:%G:%a' -- "$root") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 evidence directory is not private and deploy-owned"

prompt=$root/generation-prompt.txt
output=$root/generation.png
checkpoint=$root/generation.json
recovery=$root/.generation.json.openai-image-canary-run
journal=$recovery/journal.json
internal_output=$recovery/result.png
internal_checkpoint=$recovery/checkpoint.json

verify_checkpoint() {
  local checkpoint_sha actual_sha png_signature usage_summary
  [[ -f $checkpoint && ! -L $checkpoint \
      && $(stat -c '%U:%G:%a' -- "$checkpoint") == deploy:deploy:600 ]] || return 1
  [[ -f $output && ! -L $output \
      && $(stat -c '%U:%G:%a' -- "$output") == deploy:deploy:600 ]] || return 1
  [[ -d $recovery && ! -L $recovery \
      && $(stat -c '%U:%G:%a' -- "$recovery") == deploy:deploy:700 ]] || return 1
  [[ -f $journal && ! -L $journal \
      && $(stat -c '%U:%G:%a' -- "$journal") == deploy:deploy:600 ]] || return 1
  [[ -f $internal_output && ! -L $internal_output \
      && $(stat -c '%U:%G:%a' -- "$internal_output") == deploy:deploy:600 ]] || return 1
  [[ -f $internal_checkpoint && ! -L $internal_checkpoint \
      && $(stat -c '%U:%G:%a' -- "$internal_checkpoint") == deploy:deploy:600 ]] || return 1

  mapfile -t recovery_entries < <(
    find "$recovery" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort
  )
  [[ ${#recovery_entries[@]} -eq 3 \
      && ${recovery_entries[0]} == checkpoint.json \
      && ${recovery_entries[1]} == journal.json \
      && ${recovery_entries[2]} == result.png ]] || return 1

  jq -e --arg sha "$SHA" --argjson budget "$GENERATION_BUDGET_NANOUSD" '
    .schema_version == 1 and
    .operation == "generation" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    (.image_turn_id | type == "string" and length > 0) and
    .width == 1024 and .height == 1024 and
    .provider.background == "opaque" and
    .provider.quality == "low" and
    .provider.size == "1024x1024" and
    ((.provider.output_format == null) or (.provider.output_format == "png")) and
    (.usage | type == "object" and length > 0) and
    ((.usage | [.. | numbers]) as $values |
      ($values | length) > 0 and
      all($values[]; . >= 0 and floor == .) and
      any($values[]; . > 0)) and
    ((.request_id == null) or (.request_id | type == "string" and length > 0)) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    (.output_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$"))
  ' "$checkpoint" >/dev/null || return 1
  jq -e --arg sha "$SHA" --argjson budget "$GENERATION_BUDGET_NANOUSD" \
    --arg turn "$(jq -r '.image_turn_id' "$checkpoint")" '
    .schema_version == 1 and
    .state == "success" and
    .operation == "generation" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    .image_turn_id == $turn and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    (keys | sort) == ([
      "authorization_budget_nanousd", "image_turn_id", "implementation_sha", "model",
      "operation", "profile", "schema_version", "state"
    ] | sort)
  ' "$journal" >/dev/null || return 1
  cmp -s -- "$checkpoint" "$internal_checkpoint" || return 1
  cmp -s -- "$output" "$internal_output" || return 1
  checkpoint_sha=$(jq -r '.output_sha256 | sub("^sha256:"; "")' "$checkpoint")
  actual_sha=$(sha256sum -- "$output" | awk '{print $1}')
  [[ $checkpoint_sha == "$actual_sha" ]] || return 1
  png_signature=$(od -An -tx1 -N8 -- "$output" | tr -d ' \n')
  [[ $png_signature == 89504e470d0a1a0a ]] || return 1

  usage_summary=$(jq -c '.usage' "$checkpoint")
  printf 'GPT Image 2 generation GREEN: 1024x1024 opaque/low; usage=%s; png=%s; provider_request_id=%s\n' \
    "$usage_summary" "${actual_sha:0:16}" \
    "$(jq -r 'if .request_id == null then "absent" else "present" end' "$checkpoint")"
}

verify_diagnostic_mismatch() {
  local summary
  [[ ! -e $output && ! -L $output && ! -e $checkpoint && ! -L $checkpoint ]] || return 1
  [[ -d $recovery && ! -L $recovery \
      && $(stat -c '%U:%G:%a' -- "$recovery") == deploy:deploy:700 ]] || return 1
  [[ -f $journal && ! -L $journal \
      && $(stat -c '%U:%G:%a' -- "$journal") == deploy:deploy:600 ]] || return 1
  for forbidden in "$internal_output" "$internal_checkpoint"; do
    [[ ! -e $forbidden && ! -L $forbidden ]] || return 1
  done

  mapfile -t recovery_entries < <(
    find "$recovery" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort
  )
  [[ ${#recovery_entries[@]} -eq 1 && ${recovery_entries[0]} == journal.json ]] || return 1

  jq -e --arg sha "$SHA" --argjson budget "$GENERATION_BUDGET_NANOUSD" '
    .schema_version == 1 and
    (.state == "evidence_home_mismatch" or
      .state == "evidence_turn_mismatch" or
      .state == "evidence_controls_mismatch" or
      .state == "evidence_usage_missing") and
    .operation == "generation" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    (.image_turn_id | type == "string" and length > 0) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    (.returned.exact_home | type == "boolean") and
    (.returned.exact_turn | type == "boolean") and
    (.returned.width | type == "number" and . >= 1 and . <= 4096 and floor == .) and
    (.returned.height | type == "number" and . >= 1 and . <= 4096 and floor == .) and
    (.returned.created | type == "number" and . >= 1577836800 and . <= 4102444800 and floor == .) and
    (.returned.provider.background == "auto" or .returned.provider.background == "opaque") and
    (.returned.provider.quality == "auto" or .returned.provider.quality == "low" or
      .returned.provider.quality == "medium" or .returned.provider.quality == "high") and
    (.returned.provider.size | type == "string" and length > 0 and length <= 32) and
    ((.returned.provider.output_format == null) or (.returned.provider.output_format == "png")) and
    ((.returned.usage == null) or
      (.returned.usage | type == "object" and
        ([.. | numbers]) as $values and
        ($values | length) > 0 and all($values[]; . >= 0 and floor == .))) and
    ((.returned.request_id == null) or
      (.returned.request_id | type == "string" and length > 0 and length <= 128)) and
    (.returned.output_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$")) and
    (keys | sort) == ([
      "authorization_budget_nanousd", "image_turn_id", "implementation_sha", "model",
      "operation", "profile", "returned", "schema_version", "state"
    ] | sort) and
    ((.returned | keys | sort) == ([
      "created", "exact_home", "exact_turn", "height", "output_sha256", "provider",
      "request_id", "usage", "width"
    ] | sort) or (.returned | keys | sort) == ([
      "created", "exact_home", "exact_turn", "height", "output_sha256", "provider",
      "request_id", "width"
    ] | sort)) and
    (.returned.provider | keys | sort) == ([
      "background", "output_format", "quality", "size"
    ] | sort)
  ' "$journal" >/dev/null || return 1

  summary=$(jq -c '{state,returned:{exact_home:.returned.exact_home,exact_turn:.returned.exact_turn,width:.returned.width,height:.returned.height,created:.returned.created,provider:.returned.provider,usage:.returned.usage,request_id_present:(.returned.request_id != null),output_sha256:.returned.output_sha256}}' "$journal")
  printf 'GPT Image 2 generation WITHDRAWN: parsed result failed exact evidence; sanitized_diagnostic=%s; no image was persisted or published; exact attempt is fenced\n' "$summary"
}

if [[ -e $checkpoint || -L $checkpoint || -e $output || -L $output ]]; then
  verify_checkpoint || wd_die "existing GPT Image 2 generation evidence is incomplete or invalid"
  exit 0
fi
if [[ -e $recovery || -L $recovery ]]; then
  verify_diagnostic_mismatch || wd_die "prior GPT Image 2 generation attempt is non-replayable and invalid"
  exit 0
fi

umask 077
install -o deploy -g deploy -m 0600 /dev/null "$prompt"
printf '%s\n' \
  'Create a simple flat illustration of a blue ceramic mug on a plain beige background. No text, logos, people, brands, or copyrighted characters.' \
  >"$prompt"

load_openai_runtime_environment() {
  local unit pid executable entry name provider
  local entries=()
  for unit in claude-api-openai@8793.service claude-api-openai@8797.service; do
    [[ $(systemctl show "$unit" -p ActiveState --value 2>/dev/null) == active ]] || continue
    pid=$(systemctl show "$unit" -p MainPID --value 2>/dev/null)
    [[ $pid =~ ^[1-9][0-9]*$ ]] || continue
    executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null) || continue
    [[ $executable == "$binary" ]] || continue

    entries=()
    provider=
    while IFS= read -r -d '' entry; do
      [[ $entry == *=* ]] || wd_die "running OpenAI slot contains an invalid environment entry"
      name=${entry%%=*}
      [[ $name =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] \
        || wd_die "running OpenAI slot contains an invalid environment name"
      case "$name" in
        CLAUDE_API_PROVIDER) provider=${entry#*=}; entries+=("$entry") ;;
        CLAUDE_API_DATABASE_URL|CLAUDE_API_AFFINITY_SECRET|CLAUDE_API_REDIS_URL|\
          CLAUDE_API_AFFINITY_REDIS_URL|CLAUDE_API_CODEX_*) entries+=("$entry") ;;
      esac
    done <"/proc/$pid/environ"
    [[ $provider == openai ]] || continue

    for entry in "${entries[@]}"; do
      export "$entry"
    done
    return 0
  done
  wd_die "no active exact-release OpenAI slot can supply the parsed production environment"
}

# Production EnvironmentFile values are systemd syntax, not shell syntax. Reuse the exact active
# OpenAI slot's already parsed environment instead of evaluating root-only files as Bash. Values stay
# in process environments (never argv or logs), and fixed canary overrides are applied afterwards.
load_openai_runtime_environment
export HOME=/home/deploy
export SUB_CFG_DIR=$ENGINE_DATA_ROOT
export CLAUDE_API_PROVIDER=openai
export CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0
export CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0

deploy_uid=$(id -u deploy)
deploy_gid=$(id -g deploy)
if HOME=/home/deploy timeout --signal=TERM --kill-after=30s 300s \
  setpriv --reuid="$deploy_uid" --regid="$deploy_gid" --init-groups --no-new-privs \
  "$binary" openai-image-canary \
  --prompt-file "$prompt" \
  --output "$output" \
  --checkpoint "$checkpoint" \
  --budget-nanousd "$GENERATION_BUDGET_NANOUSD" \
  --execute; then
  verify_checkpoint || wd_die "GPT Image 2 generation completed without valid exact-SHA evidence"
else
  verify_diagnostic_mismatch \
    || wd_die "GPT Image 2 generation failed without a valid sanitized terminal diagnostic"
fi
