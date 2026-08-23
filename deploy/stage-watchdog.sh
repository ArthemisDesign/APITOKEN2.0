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
REMOTE_URL=https://github.com/3xcalibur-tech/Claude_API.git
REPORTER=$CONTOUR_GITHUB_REPORTING_HELPER
mkdir -p "$STATE" "$(dirname -- "$LOCK")"
exec 9>"$LOCK"
flock -n 9 || exit 0
if [[ ! -d $REPO/.git ]]; then
  git clone --filter=blob:none --no-checkout "$REMOTE_URL" "$REPO"
fi
git -C "$REPO" fetch --quiet --prune origin "$CONTOUR_GIT_BRANCH"
sha=$(git -C "$REPO" rev-parse "origin/$CONTOUR_GIT_BRANCH^{commit}")
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'stage-watchdog: invalid stage SHA' >&2; exit 1; }
processed=$(cat "$STATE/processed.sha" 2>/dev/null || true)
[[ $processed != "$sha" ]] || exit 0
printf '%s\n' "$sha" >"$STATE/candidate.sha"
if ! "$SCRIPT_DIR/stage-watchdog-validate.sh" "$REPO" "$sha"; then
  printf '%s\n' "$sha" >"$STATE/quarantine.dry-run.sha"
  sudo -n "$REPORTER" commit-status "$sha" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" \
    'Stage observe-only validation failed' || true
  sudo -n "$REPORTER" commit-status "$sha" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" \
    'Stage observe-only cycle failed' || true
  exit 1
fi
sudo -n "$REPORTER" commit-status "$sha" success "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" \
  'Stage observe-only validation passed'
sudo -n "$REPORTER" commit-status "$sha" success "$CONTOUR_GITHUB_STATUS_CONTEXT_ENGINE" \
  'Stage mock engine lane is informational'
sudo -n "$REPORTER" commit-status "$sha" success "$CONTOUR_GITHUB_STATUS_CONTEXT_BACKEND" \
  'Stage mock backend lane is informational'
sudo -n "$REPORTER" commit-status "$sha" success "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" \
  'Stage observe-only deployment marker published'
sudo -n "$REPORTER" commit-status "$sha" success "$CONTOUR_GITHUB_STATUS_CONTEXT_DEPLOYED" \
  'Stage observe-only exact SHA deployed'
deployment_id=$(sudo -n "$REPORTER" deployment-create "$sha" \
  "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_STAGE" 'Observe-only staging deployment')
sudo -n "$REPORTER" deployment-status "$deployment_id" success \
  'Observe-only staging deployment complete' "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_STAGE" ''
printf '%s\n' "$sha" >"$STATE/deployed.sha"
printf '%s\n' "$sha" >"$STATE/processed.sha"
printf 'stage-watchdog: deployed informational SHA %s\n' "$sha"
