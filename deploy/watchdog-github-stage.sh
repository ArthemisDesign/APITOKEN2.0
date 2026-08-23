#!/usr/bin/env bash
set -euo pipefail
ROOT=/usr/local/lib/apitoken-watchdog/stage
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'watchdog-github-stage: root required' >&2; exit 1; }
[[ ${SUDO_USER:-} == deploy-stage ]] || { echo 'watchdog-github-stage: caller rejected' >&2; exit 1; }
case "${1:-}" in
  commit-status)
    [[ $# -ge 5 && $# -le 6 ]] || exit 2
    sha=$2
    case "$4" in deploy/stage|deploy/stage-tests|deploy/stage-engine|deploy/stage-backend|stage/deployed|stage/direct-push-dry-run) ;;
      *) echo 'watchdog-github-stage: production context rejected' >&2; exit 2 ;;
    esac
    [[ $3 =~ ^(error|failure|pending|success)$ && ${#5} -le 140 ]] || exit 2
    ;;
  deployment-create)
    [[ $# -eq 4 && $3 == staging-* ]] || exit 2
    sha=$2
    ;;
  deployment-status)
    [[ $# -ge 6 && $# -le 7 && $5 == staging-* ]] || exit 2
    sha=$(cat /var/lib/apitoken-staging/watchdog/candidate.sha 2>/dev/null || true)
    ;;
  *) echo 'watchdog-github-stage: command rejected' >&2; exit 2 ;;
esac
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 2
marker=$(cat /var/lib/apitoken-staging/watchdog/candidate.sha 2>/dev/null || true)
[[ $marker == "$sha" ]] || { echo 'watchdog-github-stage: SHA is not current marker' >&2; exit 1; }
export CONTOUR_CONFIG_FILE=$ROOT/contour-stage.json
# Phase 3 reuses the single root-owned GitHub credential. The stage contour keeps a separate
# logical config path, but no second token file is created.
export CONTOUR_GITHUB_CONFIG_OVERRIDE=/etc/apitoken/github-watchdog.env
export CONTOUR_SCHEMA_FILE=$ROOT/contour-config.schema.json
export CONTOUR_CONFIG_LOADER=$ROOT/contour-config.py
exec /usr/local/lib/apitoken-watchdog/watchdog-github "$@"
