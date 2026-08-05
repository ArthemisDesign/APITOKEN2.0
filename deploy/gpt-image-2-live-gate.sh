#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
ENGINE_DATA_ROOT=/srv/claude-api/data
EXPECTED_IMPLEMENTATION_SHA=1c48e3769f0fe775e650f60ea3c5839458e5dfe2
GENERATION_IMPLEMENTATION_SHA=df58715abb4f1ac52b6c46b1ea6f830c6e11178f
GENERATION_BUDGET_NANOUSD=22330000
EDIT_BUDGET_NANOUSD=64022330000

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-live-gate.sh <exact-engine-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$EXPECTED_IMPLEMENTATION_SHA" ]] \
  || wd_die "GPT Image 2 edit gate is pinned to $EXPECTED_IMPLEMENTATION_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 edit gate must run through the fixed root bridge"

release=$ENGINE_RELEASE_ROOT/$SHA
current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
[[ $current == "$release" ]] || wd_die "GPT Image 2 gate requires current engine release $SHA"
binary=$release/claude-api
[[ -f $binary && ! -L $binary && -x $binary ]] \
  || wd_die "exact GPT Image 2 canary binary is missing"

evidence_root=$STATE_ROOT/gpt-image-2-live
[[ ! -L $evidence_root ]] || wd_die "GPT Image 2 evidence root must not be a symlink"
generation_root=$evidence_root/$GENERATION_IMPLEMENTATION_SHA
reference=$generation_root/generation.png
reference_checkpoint=$generation_root/generation.json
generation_recovery=$generation_root/.generation.json.openai-image-canary-run
generation_journal=$generation_recovery/journal.json
generation_internal_output=$generation_recovery/result.png
generation_internal_checkpoint=$generation_recovery/checkpoint.json

root=$evidence_root/$SHA
install -d -o deploy -g deploy -m 0700 -- "$evidence_root" "$root"
[[ -d $root && ! -L $root && $(stat -c '%U:%G:%a' -- "$root") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 edit evidence directory is not private and deploy-owned"

prompt=$root/edit-prompt.txt
output=$root/edit.png
checkpoint=$root/edit.json
recovery=$root/.edit.json.openai-image-canary-run
journal=$recovery/journal.json
internal_output=$recovery/result.png
internal_checkpoint=$recovery/checkpoint.json

verify_generation_reference() {
  local checkpoint_sha actual_sha png_signature
  [[ -d $generation_root && ! -L $generation_root \
      && $(stat -c '%U:%G:%a' -- "$generation_root") == deploy:deploy:700 ]] || return 1
  [[ -f $reference && ! -L $reference \
      && $(stat -c '%U:%G:%a' -- "$reference") == deploy:deploy:600 ]] || return 1
  [[ -f $reference_checkpoint && ! -L $reference_checkpoint \
      && $(stat -c '%U:%G:%a' -- "$reference_checkpoint") == deploy:deploy:600 ]] || return 1
  [[ -d $generation_recovery && ! -L $generation_recovery \
      && $(stat -c '%U:%G:%a' -- "$generation_recovery") == deploy:deploy:700 ]] || return 1
  [[ -f $generation_journal && ! -L $generation_journal \
      && $(stat -c '%U:%G:%a' -- "$generation_journal") == deploy:deploy:600 ]] || return 1
  [[ -f $generation_internal_output && ! -L $generation_internal_output \
      && $(stat -c '%U:%G:%a' -- "$generation_internal_output") == deploy:deploy:600 ]] || return 1
  [[ -f $generation_internal_checkpoint && ! -L $generation_internal_checkpoint \
      && $(stat -c '%U:%G:%a' -- "$generation_internal_checkpoint") == deploy:deploy:600 ]] || return 1

  mapfile -t generation_entries < <(
    find "$generation_recovery" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort
  )
  [[ ${#generation_entries[@]} -eq 3 \
      && ${generation_entries[0]} == checkpoint.json \
      && ${generation_entries[1]} == journal.json \
      && ${generation_entries[2]} == result.png ]] || return 1

  jq -e --arg sha "$GENERATION_IMPLEMENTATION_SHA" \
    --argjson budget "$GENERATION_BUDGET_NANOUSD" '
    .schema_version == 1 and
    .operation == "generation" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    (.image_turn_id | type == "string" and length > 0) and
    (.width | type == "number" and . >= 1 and . <= 3840 and floor == .) and
    (.height | type == "number" and . >= 1 and . <= 3840 and floor == .) and
    ((.width * .height) >= 655360 and (.width * .height) <= 8294400) and
    ((.width <= 3 * .height) and (.height <= 3 * .width)) and
    .provider.background == "opaque" and
    .provider.quality == "low" and
    (.provider.size == "auto" or .provider.size == "\(.width)x\(.height)") and
    ((.provider.output_format == null) or (.provider.output_format == "png")) and
    (.usage | type == "object" and length > 0) and
    ((.usage | [.. | numbers]) as $values |
      ($values | length) > 0 and
      all($values[]; . >= 0 and floor == .) and
      any($values[]; . > 0)) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    (.output_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$"))
  ' "$reference_checkpoint" >/dev/null || return 1
  jq -e --arg sha "$GENERATION_IMPLEMENTATION_SHA" \
    --argjson budget "$GENERATION_BUDGET_NANOUSD" \
    --arg turn "$(jq -r '.image_turn_id' "$reference_checkpoint")" '
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
  ' "$generation_journal" >/dev/null || return 1
  cmp -s -- "$reference_checkpoint" "$generation_internal_checkpoint" || return 1
  cmp -s -- "$reference" "$generation_internal_output" || return 1
  checkpoint_sha=$(jq -r '.output_sha256 | sub("^sha256:"; "")' "$reference_checkpoint")
  actual_sha=$(sha256sum -- "$reference" | awk '{print $1}')
  [[ $checkpoint_sha == "$actual_sha" ]] || return 1
  png_signature=$(od -An -tx1 -N8 -- "$reference" | tr -d ' \n')
  [[ $png_signature == 89504e470d0a1a0a ]] || return 1
}

verify_checkpoint() {
  local checkpoint_sha actual_sha png_signature image_input_tokens image_output_tokens
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

  jq -e --arg sha "$SHA" --argjson budget "$EDIT_BUDGET_NANOUSD" '
    .schema_version == 1 and
    .operation == "edit" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    (.image_turn_id | type == "string" and length > 0) and
    (.width | type == "number" and . >= 1 and . <= 3840 and floor == .) and
    (.height | type == "number" and . >= 1 and . <= 3840 and floor == .) and
    ((.width * .height) >= 655360 and (.width * .height) <= 8294400) and
    ((.width <= 3 * .height) and (.height <= 3 * .width)) and
    .provider.background == "opaque" and
    .provider.quality == "low" and
    (.provider.size == "auto" or .provider.size == "\(.width)x\(.height)") and
    ((.provider.output_format == null) or (.provider.output_format == "png")) and
    (.usage | type == "object") and
    (.usage.input_tokens | type == "number" and . > 0 and floor == .) and
    (.usage.input_tokens_details | type == "object") and
    (.usage.input_tokens_details.image_tokens | type == "number" and . > 0 and floor == .) and
    (.usage.input_tokens >= .usage.input_tokens_details.image_tokens) and
    (.usage.output_tokens | type == "number" and . > 0 and floor == .) and
    (.usage.output_tokens_details | type == "object") and
    (.usage.output_tokens_details.image_tokens | type == "number" and . > 0 and floor == .) and
    (.usage.output_tokens >= .usage.output_tokens_details.image_tokens) and
    (.usage.total_tokens | type == "number" and floor == .) and
    (.usage.total_tokens == (.usage.input_tokens + .usage.output_tokens)) and
    ((.usage | [.. | numbers]) as $values | all($values[]; . >= 0 and floor == .)) and
    ((.request_id == null) or (.request_id | type == "string" and length > 0 and length <= 128)) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    (.output_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$"))
  ' "$checkpoint" >/dev/null || return 1
  jq -e --arg sha "$SHA" --argjson budget "$EDIT_BUDGET_NANOUSD" \
    --arg turn "$(jq -r '.image_turn_id' "$checkpoint")" '
    .schema_version == 1 and
    .state == "success" and
    .operation == "edit" and
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
  ! cmp -s -- "$output" "$reference" || return 1
  checkpoint_sha=$(jq -r '.output_sha256 | sub("^sha256:"; "")' "$checkpoint")
  actual_sha=$(sha256sum -- "$output" | awk '{print $1}')
  [[ $checkpoint_sha == "$actual_sha" ]] || return 1
  png_signature=$(od -An -tx1 -N8 -- "$output" | tr -d ' \n')
  [[ $png_signature == 89504e470d0a1a0a ]] || return 1

  image_input_tokens=$(jq -r '.usage.input_tokens_details.image_tokens' "$checkpoint")
  image_output_tokens=$(jq -r '.usage.output_tokens_details.image_tokens' "$checkpoint")
  printf 'GPT Image 2 edit GREEN: %sx%s opaque/low/auto; image_input_tokens=%s; image_output_tokens=%s; png=%s; provider_request_id=%s\n' \
    "$(jq -r '.width' "$checkpoint")" "$(jq -r '.height' "$checkpoint")" \
    "$image_input_tokens" "$image_output_tokens" "${actual_sha:0:16}" \
    "$(jq -r 'if .request_id == null then "absent" else "present" end' "$checkpoint")"
}

verify_terminal_withdrawal() {
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

  jq -e --arg sha "$SHA" --argjson budget "$EDIT_BUDGET_NANOUSD" '
    .schema_version == 1 and
    .operation == "edit" and
    .model == "gpt-image-2" and
    (.profile | type == "string" and length > 0) and
    (.image_turn_id | type == "string" and length > 0) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == $budget and
    if (.state == "rejected" or .state == "outcome_unknown") then
      (has("returned") | not) and
      (keys | sort) == ([
        "authorization_budget_nanousd", "image_turn_id", "implementation_sha", "model",
        "operation", "profile", "schema_version", "state"
      ] | sort)
    else
      (.state == "evidence_home_mismatch" or
        .state == "evidence_turn_mismatch" or
        .state == "evidence_controls_mismatch" or
        .state == "evidence_usage_missing") and
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
          (([.. | numbers]) as $values |
            ($values | length) > 0 and all($values[]; . >= 0 and floor == .)))) and
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
    end
  ' "$journal" >/dev/null || return 1

  summary=$(jq -c 'if has("returned") then {state,returned:{exact_home:.returned.exact_home,exact_turn:.returned.exact_turn,width:.returned.width,height:.returned.height,created:.returned.created,provider:.returned.provider,usage:.returned.usage,request_id_present:(.returned.request_id != null),output_sha256:.returned.output_sha256}} else {state} end' "$journal")
  printf 'GPT Image 2 edit WITHDRAWN: sanitized_diagnostic=%s; no image was persisted or published; exact attempt is fenced\n' "$summary"
}

verify_generation_reference \
  || wd_die "owned GPT Image 2 generation reference or its immutable provenance is invalid"
if [[ -e $checkpoint || -L $checkpoint || -e $output || -L $output ]]; then
  verify_checkpoint || wd_die "existing GPT Image 2 edit evidence is incomplete or invalid"
  exit 0
fi
if [[ -e $recovery || -L $recovery ]]; then
  verify_terminal_withdrawal || wd_die "prior GPT Image 2 edit attempt is non-replayable and invalid"
  exit 0
fi

umask 077
install -o deploy -g deploy -m 0600 /dev/null "$prompt"
printf '%s\n' \
  'Edit the supplied image so the ceramic mug is bright red instead of blue. Keep the plain beige background and simple flat illustration style. Add no text, logos, people, brands, or copyrighted characters.' \
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
  --reference "$reference" \
  --output "$output" \
  --checkpoint "$checkpoint" \
  --budget-nanousd "$EDIT_BUDGET_NANOUSD" \
  --execute; then
  verify_checkpoint || wd_die "GPT Image 2 edit completed without valid exact-SHA evidence"
else
  verify_terminal_withdrawal \
    || wd_die "GPT Image 2 edit failed without a valid sanitized terminal diagnostic"
fi
