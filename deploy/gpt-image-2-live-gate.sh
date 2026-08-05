#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
EXPECTED_IMPLEMENTATION_SHA=3f67d43c0ae541979fee66823d251e2e3eea33e0
GENERATION_BUDGET_NANOUSD=8560000

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-live-gate.sh <exact-engine-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$EXPECTED_IMPLEMENTATION_SHA" ]] \
  || wd_die "GPT Image 2 recovery gate is pinned to $EXPECTED_IMPLEMENTATION_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 recovery gate must run through the fixed root bridge"

root=$STATE_ROOT/gpt-image-2-live/$SHA
prompt=$root/generation-prompt.txt
output=$root/generation.png
checkpoint=$root/generation.json
recovery=$root/.generation.json.openai-image-canary-run
journal=$recovery/journal.json

[[ -d $root && ! -L $root && $(stat -c '%U:%G:%a' -- "$root") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 evidence directory is not private and deploy-owned"
[[ -f $prompt && ! -L $prompt && $(stat -c '%U:%G:%a' -- "$prompt") == deploy:deploy:600 ]] \
  || wd_die "GPT Image 2 prompt evidence is missing or unsafe"
[[ ! -e $output && ! -L $output && ! -e $checkpoint && ! -L $checkpoint ]] \
  || wd_die "GPT Image 2 withdrawal cannot coexist with published generation evidence"
[[ -d $recovery && ! -L $recovery \
    && $(stat -c '%U:%G:%a' -- "$recovery") == deploy:deploy:700 ]] \
  || wd_die "GPT Image 2 non-replayable recovery directory is missing or unsafe"
[[ -f $journal && ! -L $journal \
    && $(stat -c '%U:%G:%a' -- "$journal") == deploy:deploy:600 ]] \
  || wd_die "GPT Image 2 non-replayable journal is missing or unsafe"

mapfile -t recovery_entries < <(find "$recovery" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
[[ ${#recovery_entries[@]} -eq 1 && ${recovery_entries[0]} == journal.json ]] \
  || wd_die "GPT Image 2 recovery contains unexpected artifacts"

jq -e --arg sha "$SHA" --argjson budget "$GENERATION_BUDGET_NANOUSD" '
  .schema_version == 1 and
  .state == "evidence_incomplete" and
  .operation == "generation" and
  .model == "gpt-image-2" and
  (.profile | type == "string" and length > 0) and
  (.image_turn_id | type == "string" and length > 0) and
  .implementation_sha == $sha and
  .authorization_budget_nanousd == $budget
' "$journal" >/dev/null \
  || wd_die "GPT Image 2 non-replayable journal does not match the withdrawn attempt"

printf 'GPT Image 2 generation WITHDRAWN: parsed result lacked required publication evidence; exact attempt is fenced and was not replayed\n'
