#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
ENGINE_DATA_ROOT=/srv/claude-api/data
EXPECTED_IMPLEMENTATION_SHA=3f67d43c0ae541979fee66823d251e2e3eea33e0
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

verify_checkpoint() {
  local checkpoint_sha actual_sha usage_summary input_tokens input_text input_image
  local output_tokens output_image total_tokens
  [[ -f $checkpoint && ! -L $checkpoint && $(stat -c '%a' -- "$checkpoint") == 600 ]] \
    || return 1
  [[ -f $output && ! -L $output && $(stat -c '%a' -- "$output") == 600 ]] || return 1
  jq -e --arg sha "$SHA" '
    .schema_version == 1 and
    .operation == "generation" and
    .model == "gpt-image-2" and
    .width == 1024 and .height == 1024 and
    .provider.background == "opaque" and
    .provider.quality == "low" and
    .provider.size == "1024x1024" and
    ((.provider.output_format == null) or (.provider.output_format == "png")) and
    (.usage | type == "object") and
    (.request_id | type == "string" and length > 0) and
    .implementation_sha == $sha and
    .authorization_budget_nanousd == 8560000 and
    (.output_sha256 | type == "string" and startswith("sha256:"))
  ' "$checkpoint" >/dev/null || return 1
  checkpoint_sha=$(jq -r '.output_sha256 | sub("^sha256:"; "")' "$checkpoint")
  actual_sha=$(sha256sum -- "$output" | awk '{print $1}')
  [[ $checkpoint_sha == "$actual_sha" ]] || return 1
  usage_summary=$(jq -r '[
    .usage.input_tokens // "na",
    .usage.input_tokens_details.text_tokens // "na",
    .usage.input_tokens_details.image_tokens // "na",
    .usage.output_tokens // "na",
    .usage.output_tokens_details.image_tokens // "na",
    .usage.total_tokens // "na"
  ] | @tsv' "$checkpoint")
  IFS=$'\t' read -r input_tokens input_text input_image output_tokens output_image total_tokens \
    <<<"$usage_summary"
  printf 'GPT Image 2 generation GREEN: 1024x1024 opaque/low; usage=input:%s,text:%s,image:%s,output:%s,output_image:%s,total:%s; png=%s\n' \
    "$input_tokens" "$input_text" "$input_image" "$output_tokens" "$output_image" \
    "$total_tokens" "${actual_sha:0:16}"
}

if [[ -e $checkpoint || -L $checkpoint || -e $output || -L $output ]]; then
  verify_checkpoint || wd_die "existing GPT Image 2 generation evidence is incomplete or invalid"
  exit 0
fi
[[ ! -e $recovery && ! -L $recovery ]] \
  || wd_die "prior GPT Image 2 generation attempt is non-replayable; inspect private recovery evidence"

umask 077
install -o deploy -g deploy -m 0600 /dev/null "$prompt"
printf '%s\n' \
  'Create a simple flat illustration of a blue ceramic mug on a plain beige background. No text, logos, people, brands, or copyrighted characters.' \
  >"$prompt"

for environment in config.env server.env engine-postgres.env; do
  path=$ENGINE_DATA_ROOT/$environment
  [[ -f $path && ! -L $path ]] || wd_die "required engine environment is missing: $environment"
  # shellcheck disable=SC1090
  set -a; . "$path"; set +a
done
export SUB_CFG_DIR=$ENGINE_DATA_ROOT
export CLAUDE_API_PROVIDER=openai
export CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0
export CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0

deploy_uid=$(id -u deploy)
deploy_gid=$(id -g deploy)
HOME=/home/deploy timeout --signal=TERM --kill-after=30s 300s \
  setpriv --reuid="$deploy_uid" --regid="$deploy_gid" --init-groups --no-new-privs \
  "$binary" openai-image-canary \
  --prompt-file "$prompt" \
  --output "$output" \
  --checkpoint "$checkpoint" \
  --budget-nanousd "$GENERATION_BUDGET_NANOUSD" \
  --execute

verify_checkpoint || wd_die "GPT Image 2 generation completed without valid exact-SHA evidence"
