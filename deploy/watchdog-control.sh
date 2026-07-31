#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
REJECTED=$STATE_ROOT/rejected.sha

usage() {
  printf '%s\n' \
    'Usage:' \
    '  apitoken-watchdog status' \
    '  apitoken-watchdog run' \
    '  apitoken-watchdog retry <full-sha>' \
    '  apitoken-watchdog logs'
}

case "${1:-}" in
  status)
    if [[ -r $STATE_ROOT/status ]]; then
      cat "$STATE_ROOT/status"
    else
      printf 'watchdog has no status yet\n'
    fi
    # Independent infrastructure, sales, OpenKeys, and admin baselines must remain visible so an
    # operator can distinguish a pending controller handoff from an application rollout.
    for entry in processed infrastructure engine backend sales openkeys admin rejected pending-migration; do
      if [[ -r $STATE_ROOT/$entry.sha ]]; then
        printf '%s=%s\n' "$entry" "$(<"$STATE_ROOT/$entry.sha")"
      fi
    done
    for slot in 1 2; do
      if [[ -r $STATE_ROOT/candidate-validation-$slot.status ]]; then
        printf 'candidate_slot_%s=%s\n' "$slot" "$(<"$STATE_ROOT/candidate-validation-$slot.status")"
      fi
    done
    systemctl --no-pager --full status apitoken-deploy-watchdog.timer \
      apitoken-deploy-watchdog.service apitoken-candidate-validator.timer \
      apitoken-candidate-validator.service || true
    ;;
  run)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "run requires root"
    systemctl start apitoken-deploy-watchdog.service apitoken-candidate-validator.service
    ;;
  retry)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "retry requires root"
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    wd_validate_sha "$2"
    rejected=$(wd_read_sha "$REJECTED") || wd_die "there is no quarantined candidate"
    [[ $rejected == "$2" ]] || wd_die "quarantined candidate is $rejected, not $2"
    rm -f -- "$REJECTED"
    wd_log "cleared quarantine for $2"
    systemctl start apitoken-deploy-watchdog.service
    ;;
  logs)
    journalctl -u apitoken-deploy-watchdog.service \
      -u apitoken-candidate-validator.service -n 250 --no-pager
    ;;
  -h|--help|'')
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
