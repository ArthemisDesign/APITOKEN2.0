#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 && ${SUDO_USER:-} == stage-ctl ]] || exit 1
STATE=/var/lib/apitoken-staging/watchdog
ROOT=/usr/local/lib/apitoken-watchdog
lock=/run/lock/apitoken-stage-promotion.lock
exec 8>"$lock"; flock -n 8 || { echo 'stage-promotion: locked' >&2; exit 1; }
cmd=$1; shift
case "$cmd" in
  attest)
    [[ $# -eq 3 && $1 =~ ^[0-9a-f]{40}$ && $2 =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
    sha=$1 actor=$2 reason=$3
    [[ $(cat "$STATE/deployed.sha" 2>/dev/null) == "$sha" ]] || exit 1
    [[ $(cat "$STATE/processed.sha" 2>/dev/null) == "$sha" ]] || exit 1
    [[ -n $reason && ${#reason} -le 512 ]] || exit 2
    STAGE_POLICY_FILE=$ROOT/stage-degradation-policy.json \
      $ROOT/stage-attestation.py --mode promotion --commit "$sha" --actor "$actor" --reason "$reason" \
      --state-root "$STATE" --repo /opt/apitoken-staging/repo >"$STATE/promotion-eligible.json"
    chown root:deploy-stage "$STATE/promotion-eligible.json"; chmod 0640 "$STATE/promotion-eligible.json"
    CONTOUR_CONFIG_FILE=/usr/local/lib/apitoken-watchdog/contour-stage.json \
      CONTOUR_SCHEMA_FILE=/usr/local/lib/apitoken-watchdog/contour-config.schema.json \
      CONTOUR_CONFIG_LOADER=/usr/local/lib/apitoken-watchdog/contour-config.py \
      CONTOUR_GITHUB_CONFIG_OVERRIDE=/etc/apitoken/github-watchdog.env \
      "$ROOT/watchdog-github" commit-status "$sha" success promotion/eligible \
      'Host-owned promotion eligible'
    ;;
  sync)
    [[ $# -eq 1 && $1 =~ ^[0-9a-f]{40}$ ]] || exit 2
    sha=$1
    previous=$(jq -r '.commit_sha // empty' "$STATE/promotion-eligible.json" 2>/dev/null || true)
    rm -f -- "$STATE/promotion-approved.json" "$STATE/promotion-eligible.json" "$STATE/degradation.json"
    if [[ $previous =~ ^[0-9a-f]{40}$ ]]; then
      CONTOUR_CONFIG_FILE=/usr/local/lib/apitoken-watchdog/contour-stage.json \
        CONTOUR_SCHEMA_FILE=/usr/local/lib/apitoken-watchdog/contour-config.schema.json \
        CONTOUR_CONFIG_LOADER=/usr/local/lib/apitoken-watchdog/contour-config.py \
        CONTOUR_GITHUB_CONFIG_OVERRIDE=/etc/apitoken/github-watchdog.env \
        "$ROOT/watchdog-github" commit-status "$previous" failure promotion/eligible \
        'Promotion eligible invalidated by stage-sync' || true
    fi
    printf '%s\n' "$sha" >"$STATE/hotfix-sync.request"
    chown root:deploy-stage "$STATE/hotfix-sync.request"; chmod 0640 "$STATE/hotfix-sync.request"
    echo "stage-sync: invalidated stale approval; stage must validate master $sha"
    ;;
  *) exit 2 ;;
esac
