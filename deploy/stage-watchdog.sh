#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CONFIG=${CONTOUR_CONFIG_FILE:-$SCRIPT_DIR/contour-stage.json}
export CONTOUR_CONFIG_FILE=$CONFIG
# shellcheck source=deploy/contour-config.sh
source "$SCRIPT_DIR/contour-config.sh"
STATE=$CONTOUR_ROOTS_STATE
LOCK=$CONTOUR_LOCKS_WATCHDOG
REPO=$CONTOUR_ROOTS_SOURCE_REPO
SOURCE_MARKER=$STATE/source.sha
mkdir -p "$STATE" "$(dirname -- "$LOCK")"
exec 9>"$LOCK"
flock -n 9 || exit 0
sha=$(cat "$SOURCE_MARKER" 2>/dev/null || true)
[[ -n $sha ]] || exit 0
[[ -d $REPO/.git ]] || { echo 'stage-watchdog: source service did not publish repository' >&2; exit 1; }
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'stage-watchdog: invalid stage SHA' >&2; exit 1; }
processed=$(cat "$STATE/processed.sha" 2>/dev/null || true)
[[ $processed != "$sha" ]] || exit 0
printf '%s\n' "$sha" >"$STATE/candidate.sha"
if ! "$SCRIPT_DIR/stage-watchdog-validate.sh" "$REPO" "$sha"; then
  printf '%s\n' "$sha" >"$STATE/quarantine.dry-run.sha"
  exit 1
fi
printf '%s\n' "$sha" >"$STATE/report-pending.sha"
printf 'stage-watchdog: validated informational SHA %s\n' "$sha"
