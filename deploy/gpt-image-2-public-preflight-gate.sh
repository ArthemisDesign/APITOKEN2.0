#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
PRODUCER_SHA=d42fc0e3290c0042a16797626326c250e0f6721c
EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-preflight
OUTPUT=$EVIDENCE_PARENT/$PRODUCER_SHA

[[ $# -eq 2 && $2 == --inspect ]] \
  || wd_die "usage: gpt-image-2-public-preflight-gate.sh <exact-producer-sha> --inspect"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$PRODUCER_SHA" ]] \
  || wd_die "GPT Image 2 public preflight inspector is pinned to producer $PRODUCER_SHA"
[[ $(id -u) == 0 ]] \
  || wd_die "GPT Image 2 public preflight inspector must run through the fixed root bridge"
command -v jq >/dev/null || wd_die "jq is required for GPT Image 2 public preflight evidence"

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
  || wd_die "GPT Image 2 public preflight parent is not an actual private deploy directory"
path_is_private_deploy_directory "$OUTPUT" \
  || wd_die "GPT Image 2 public preflight fence is not an actual private deploy directory"
mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
[[ ${#entries[@]} -eq 1 && ${entries[0]} == journal.json ]] \
  || wd_die "GPT Image 2 public preflight fence contains unexpected artifacts"
journal=$OUTPUT/journal.json
path_is_private_deploy_file "$journal" \
  || wd_die "GPT Image 2 public preflight fence has no private journal"
summary=$(jq -er --arg sha "$PRODUCER_SHA" '
  . as $journal |
  ((keys | sort) == ([
    "edit_dispatched", "edit_request_id", "generation_dispatched", "generation_request_id",
    "implementation_sha", "schema_version", "state"
  ] | sort) and
  .schema_version == 1 and .implementation_sha == $sha and
  (.state | type == "string" and length >= 1 and length <= 64 and test("^[a-z_]+$")) and
  .generation_dispatched == false and .edit_dispatched == false and
  .generation_request_id == null and .edit_request_id == null) as $valid |
  if $valid then "gpt-image-preflight:\($journal.state):g=false:e=false"
  else error("invalid journal") end
' "$journal") || wd_die "GPT Image 2 public preflight journal is malformed"
printf '%s\n' "$summary"
