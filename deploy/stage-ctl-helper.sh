#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 && ${SUDO_USER:-} == stage-ctl ]] || { echo 'stage-ctl: caller rejected' >&2; exit 1; }
[[ $# -eq 1 ]] || exit 2
case "$1" in
  emergency-stop)
    systemctl stop staging.slice
    loginctl terminate-user deploy-stage >/dev/null 2>&1 || true
    ;;
  reseed)
    /usr/local/lib/apitoken-watchdog/stage-seed.sh reseed
    ;;
  attest|sync)
    echo "stage-ctl: $1 is phase-disabled" >&2
    exit 2
    ;;
  *) exit 2 ;;
esac
