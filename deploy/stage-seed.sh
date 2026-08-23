#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-seed: root required' >&2; exit 1; }
[[ ${SUDO_USER:-} == stage-ctl ]] || { echo 'stage-seed: caller rejected' >&2; exit 1; }
[[ ${1:-} == reseed && $# -eq 1 ]] || exit 2
marker=/var/lib/apitoken-staging/watchdog/reseed.requested
printf '%s\n' "$(date -u +%FT%TZ)" >"$marker"
chown deploy-stage:deploy-stage "$marker"; chmod 0600 "$marker"
printf 'stage-seed: reseed request recorded; Phase 4 seed worker remains mock-only\n'
