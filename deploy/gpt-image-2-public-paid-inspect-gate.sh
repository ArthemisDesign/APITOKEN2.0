#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223
EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke
OUTPUT=$EVIDENCE_PARENT/$PRODUCER_SHA

[[ $# -eq 2 && $2 == --inspect ]] \
  || wd_die "usage: gpt-image-2-public-paid-inspect-gate.sh <exact-producer-sha> --inspect"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$PRODUCER_SHA" ]] \
  || wd_die "GPT Image 2 public paid inspector is pinned to producer $PRODUCER_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 public paid inspector must run through the fixed root bridge"
command -v jq >/dev/null || wd_die "jq is required for GPT Image 2 public paid inspection"
command -v sha256sum >/dev/null || wd_die "sha256sum is required for GPT Image 2 public paid inspection"

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

path_is_private_deploy_directory "$EVIDENCE_PARENT" \
  || wd_die "GPT Image 2 public paid evidence parent is not an actual private deploy directory"
path_is_private_deploy_directory "$OUTPUT" \
  || wd_die "GPT Image 2 public paid evidence root is not an actual private deploy directory"
journal=$OUTPUT/journal.json
generation=$OUTPUT/generation.png
path_is_private_deploy_file "$journal" \
  || wd_die "GPT Image 2 public paid fence has no private journal"

mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
[[ ${#entries[@]} -eq 2 && ${entries[0]} == generation.png && ${entries[1]} == journal.json ]] \
  || wd_die "GPT Image 2 public paid fence has unexpected artifacts"

jq -e --arg sha "$PRODUCER_SHA" '
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

path_is_private_deploy_file "$generation" \
  || wd_die "GPT Image 2 public paid generation PNG is absent or not private"
bytes=$(stat -c '%s' -- "$generation")
[[ $bytes =~ ^[1-9][0-9]*$ ]] && (( bytes <= 16 * 1024 * 1024 )) \
  || wd_die "GPT Image 2 public paid generation PNG is outside the byte bound"
png_dimensions "$generation" || wd_die "GPT Image 2 public paid generation is not a valid PNG"
digest=$(sha256sum -- "$generation" | awk '{print $1}')
[[ $digest =~ ^[0-9a-f]{64}$ ]] || wd_die "GPT Image 2 public paid generation digest is invalid"

jq -cn \
  --arg state generation_received \
  --argjson width "$PNG_WIDTH" \
  --argjson height "$PNG_HEIGHT" \
  --arg png_sha256 "sha256:$digest" \
  --argjson png_bytes "$bytes" \
  '{state: $state, generation_dispatched: true, edit_dispatched: false,
    generation: {width: $width, height: $height, png_sha256: $png_sha256, png_bytes: $png_bytes}}'
