#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
REJECTED=$STATE_ROOT/rejected.sha
PENDING_INFRA=$STATE_ROOT/pending-infrastructure.sha
APPROVED_INFRA=$STATE_ROOT/infrastructure-approved.sha

usage() {
  printf '%s\n' \
    'Usage:' \
    '  apitoken-watchdog status' \
    '  apitoken-watchdog run' \
    '  apitoken-watchdog retry <full-sha>' \
    '  apitoken-watchdog migrate <full-sha>' \
    '  apitoken-watchdog approve-infrastructure <full-sha>' \
    '  apitoken-watchdog logs'
}

case "${1:-}" in
  status)
    if [[ -r $STATE_ROOT/status ]]; then
      cat "$STATE_ROOT/status"
    else
      printf 'watchdog has no status yet\n'
    fi
    for entry in processed engine backend rejected pending-migration pending-infrastructure; do
      if [[ -r $STATE_ROOT/$entry.sha ]]; then
        printf '%s=%s\n' "$entry" "$(<"$STATE_ROOT/$entry.sha")"
      fi
    done
    systemctl --no-pager --full status apitoken-deploy-watchdog.timer \
      apitoken-deploy-watchdog.service || true
    ;;
  run)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "run requires root"
    systemctl start apitoken-deploy-watchdog.service
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
  migrate)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "migrate requires root"
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh "$2"
    systemctl start apitoken-deploy-watchdog.service
    ;;
  approve-infrastructure)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "infrastructure approval requires root"
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    wd_validate_sha "$2"
    pending=$(wd_read_sha "$PENDING_INFRA") || wd_die "there is no pending infrastructure review"
    [[ $pending == "$2" ]] || wd_die "pending infrastructure candidate is $pending, not $2"
    marker_sha=$(wd_marker_value "$STATE_ROOT/$2.tested" sha) \
      || wd_die "candidate has no valid test-success marker"
    [[ $marker_sha == "$2" ]] || wd_die "test-success marker belongs to another SHA"
    wd_atomic_write "$APPROVED_INFRA" "$2"
    chown root:deploy "$APPROVED_INFRA"
    wd_log "approved reviewed operational changes for $2"
    systemctl start apitoken-deploy-watchdog.service
    ;;
  logs)
    journalctl -u apitoken-deploy-watchdog.service -n 250 --no-pager
    ;;
  -h|--help|'')
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
