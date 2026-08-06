#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
PRODUCER_SHA=d42fc0e3290c0042a16797626326c250e0f6721c
EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-preflight
OUTPUT=$EVIDENCE_PARENT/$PRODUCER_SHA

[[ $# -eq 1 ]] || wd_die "usage: gpt-image-2-public-preflight-gate.sh <exact-producer-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$PRODUCER_SHA" ]] \
  || wd_die "GPT Image 2 public preflight is pinned to producer $PRODUCER_SHA"
[[ $(id -u) == 0 ]] || wd_die "GPT Image 2 public preflight must run through the fixed root bridge"
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

journal_summary() {
  local journal=$OUTPUT/journal.json
  path_is_private_deploy_file "$journal" || return 1
  jq -er --arg sha "$PRODUCER_SHA" '
    . as $journal |
    ((keys | sort) == ([
      "edit_dispatched", "edit_request_id", "generation_dispatched", "generation_request_id",
      "implementation_sha", "schema_version", "state"
    ] | sort) and
    .schema_version == 1 and .implementation_sha == $sha and
    (.state | type == "string" and length >= 1 and length <= 64 and test("^[a-z_]+$")) and
    .generation_dispatched == false and .edit_dispatched == false and
    .generation_request_id == null and .edit_request_id == null) as $valid |
    if $valid then "gpt-image-preflight:\($journal.state)" else error("invalid journal") end
  ' "$journal"
}

verify_preflight_success() {
  local summary entries=()
  path_is_private_deploy_directory "$EVIDENCE_PARENT" || return 1
  path_is_private_deploy_directory "$OUTPUT" || return 1
  mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
  [[ ${#entries[@]} -eq 1 && ${entries[0]} == journal.json ]] || return 1
  summary=$(journal_summary) || return 1
  [[ $summary == gpt-image-preflight:preflight_success ]]
}

# A SHA-keyed root is a one-shot fence even though this mode cannot dispatch an image.
if [[ -e $OUTPUT || -L $OUTPUT ]]; then
  verify_preflight_success \
    || wd_die "prior GPT Image 2 public preflight is fenced without exact success evidence"
  exit 0
fi
if [[ -e $EVIDENCE_PARENT || -L $EVIDENCE_PARENT ]]; then
  path_is_private_deploy_directory "$EVIDENCE_PARENT" \
    || wd_die "GPT Image 2 public preflight parent is not an actual private deploy directory"
else
  install -d -o deploy -g deploy -m 0700 -- "$EVIDENCE_PARENT"
fi
path_is_private_deploy_directory "$EVIDENCE_PARENT" \
  || wd_die "GPT Image 2 public preflight parent is not private"

release=$ENGINE_RELEASE_ROOT/$PRODUCER_SHA
current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
[[ $current == "$release" ]] \
  || wd_die "GPT Image 2 public preflight requires current producer release $PRODUCER_SHA"
binary=$release/claude-api
[[ -f $binary && ! -L $binary && -x $binary ]] \
  || wd_die "exact GPT Image 2 public preflight binary is missing"

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

# The only inherited application value is the authority DSN. The existing service key is selected
# inside the exact binary from PostgreSQL and is never placed in argv, this controller, or a file.
load_database_url_from_openai_slot
deploy_uid=$(id -u deploy)
deploy_gid=$(id -g deploy)
if ! timeout --signal=TERM --kill-after=10s 60s \
    setpriv --reuid="$deploy_uid" --regid="$deploy_gid" --init-groups --no-new-privs \
    env -i HOME=/home/deploy CLAUDE_API_DATABASE_URL="$CLAUDE_API_DATABASE_URL" \
    "$binary" openai-image-public-smoke --output "$OUTPUT" --preflight-only \
    >/dev/null 2>/dev/null; then
  if path_is_private_deploy_directory "$OUTPUT"; then
    journal_summary || true
  fi
  wd_die "GPT Image 2 public preflight failed without any image dispatch"
fi
verify_preflight_success \
  || wd_die "GPT Image 2 public preflight returned without exact no-dispatch success evidence"
printf 'GPT Image 2 public preflight GREEN for %s\n' "$PRODUCER_SHA"
