#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
EXPECTED_IMPLEMENTATION_SHA=012fccc471142fc51a46563da3a87564d674b39f
GENERATION_BUDGET_NANOUSD=8560000

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-live-gate.sh <exact-engine-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$EXPECTED_IMPLEMENTATION_SHA" ]] \
  || wd_die "GPT Image 2 generation withdrawal is pinned to $EXPECTED_IMPLEMENTATION_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 generation withdrawal must run through the fixed root bridge"

root=$STATE_ROOT/gpt-image-2-live/$SHA
recovery=$root/.generation.json.openai-image-canary-run
journal=$recovery/journal.json
output=$root/generation.png
checkpoint=$root/generation.json
internal_output=$recovery/result.png
internal_checkpoint=$recovery/checkpoint.json

[[ -d $root && ! -L $root && $(stat -c '%U:%G:%a' -- "$root") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 withdrawal evidence root is missing or unsafe"
[[ -d $recovery && ! -L $recovery \
    && $(stat -c '%U:%G:%a' -- "$recovery") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 terminal recovery directory is missing or unsafe"
[[ -f $journal && ! -L $journal \
    && $(stat -c '%U:%G:%a' -- "$journal") == deploy:deploy:600 ]] \
  || wd_die "GPT Image 2 terminal journal is missing or unsafe"

mapfile -t recovery_entries < <(
  find "$recovery" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort
)
[[ ${#recovery_entries[@]} -eq 1 && ${recovery_entries[0]} == journal.json ]] \
  || wd_die "GPT Image 2 withdrawn attempt contains unexpected recovery artifacts"
for forbidden in "$output" "$checkpoint" "$internal_output" "$internal_checkpoint"; do
  [[ ! -e $forbidden && ! -L $forbidden ]] \
    || wd_die "GPT Image 2 withdrawn attempt unexpectedly published an artifact"
done

jq -e --arg sha "$SHA" --argjson budget "$GENERATION_BUDGET_NANOUSD" '
  .schema_version == 1 and
  .state == "evidence_controls_mismatch" and
  .operation == "generation" and
  .model == "gpt-image-2" and
  (.profile | type == "string" and length > 0) and
  (.image_turn_id | type == "string" and length > 0) and
  .implementation_sha == $sha and
  .authorization_budget_nanousd == $budget and
  (keys | sort) == ([
    "authorization_budget_nanousd",
    "image_turn_id",
    "implementation_sha",
    "model",
    "operation",
    "profile",
    "schema_version",
    "state"
  ] | sort)
' "$journal" >/dev/null \
  || wd_die "GPT Image 2 terminal controls-mismatch journal is invalid"

printf 'GPT Image 2 generation WITHDRAWN: provider returned mismatched controls; exact paid attempt is fenced, no artifact was published, and no request was replayed\n'
