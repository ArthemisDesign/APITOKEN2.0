#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-report-publish: root required' >&2; exit 1; }
STATE=/var/lib/apitoken-staging/watchdog
ROOT=/usr/local/lib/apitoken-watchdog/stage
sha=$(cat "$STATE/report-pending.sha" 2>/dev/null || true)
[[ -n $sha ]] || exit 0
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 1
[[ $(cat "$STATE/candidate.sha" 2>/dev/null || true) == "$sha" ]] || exit 1
export CONTOUR_CONFIG_FILE=$ROOT/contour-stage.json
export CONTOUR_SCHEMA_FILE=$ROOT/contour-config.schema.json
export CONTOUR_CONFIG_LOADER=$ROOT/contour-config.py
export CONTOUR_GITHUB_CONFIG_OVERRIDE=/etc/apitoken/github-watchdog.env
REPORT=/usr/local/lib/apitoken-watchdog/watchdog-github
"$REPORT" commit-status "$sha" success deploy/stage-tests 'Stage observe-only validation passed'
"$REPORT" commit-status "$sha" success deploy/stage-engine 'Stage mock engine lane is informational'
"$REPORT" commit-status "$sha" success deploy/stage-backend 'Stage mock backend lane is informational'
"$REPORT" commit-status "$sha" success deploy/stage 'Stage observe-only deployment marker published'
"$REPORT" commit-status "$sha" success stage/deployed 'Stage observe-only exact SHA deployed'
deployment_id=$("$REPORT" deployment-create "$sha" staging-environment 'Observe-only staging deployment')
"$REPORT" deployment-status "$deployment_id" success 'Observe-only staging deployment complete' staging-environment ''
printf '%s\n' "$sha" >"$STATE/deployed.sha"
printf '%s\n' "$sha" >"$STATE/processed.sha"
chown deploy-stage:deploy-stage "$STATE/deployed.sha" "$STATE/processed.sha"
rm -f "$STATE/report-pending.sha"
