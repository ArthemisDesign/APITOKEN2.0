#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"
CONTOUR_ROOT=${LIB%/*}
# shellcheck source=deploy/contour-config.sh
source "$CONTOUR_ROOT/contour-config.sh"

STATE_ROOT=$CONTOUR_ROOTS_STATE
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
    systemctl --no-pager --full status "$CONTOUR_UNITS_WATCHDOG_TIMER" \
      "$CONTOUR_UNITS_WATCHDOG_SERVICE" "$CONTOUR_UNITS_CANDIDATE_VALIDATOR_TIMER" \
      "$CONTOUR_UNITS_CANDIDATE_VALIDATOR_SERVICE" || true
    ;;
  run)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "run requires root"
    systemctl start "$CONTOUR_UNITS_WATCHDOG_SERVICE" \
      "$CONTOUR_UNITS_CANDIDATE_VALIDATOR_SERVICE"
    ;;
  retry)
    [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "retry requires root"
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    wd_validate_sha "$2"
    rejected=$(wd_read_sha "$REJECTED") || wd_die "there is no quarantined candidate"
    [[ $rejected == "$2" ]] || wd_die "quarantined candidate is $rejected, not $2"
    rm -f -- "$REJECTED"
    wd_log "cleared quarantine for $2"
    systemctl start "$CONTOUR_UNITS_WATCHDOG_SERVICE"
    ;;
  logs)
    failure_dir=$CONTOUR_ROOTS_STATE/failures
    shopt -s nullglob
    reports=("$failure_dir"/*.summary.md)
    if (( ${#reports[@]} > 0 )); then
      latest=$(ls -t -- "${reports[@]}" | head -n 1)
      if [[ -f $latest && ! -L $latest ]]; then
        printf '--- latest redacted failure report ---\n'
        cat -- "$latest"
        text=${latest%.summary.md}.text
        if [[ -f $text && ! -L $text ]]; then
          printf '\n--- excerpt ---\n'
          cat -- "$text"
        fi
        printf '\n'
      fi
    fi
    journalctl -u "$CONTOUR_UNITS_WATCHDOG_SERVICE" \
      -u "$CONTOUR_UNITS_CANDIDATE_VALIDATOR_SERVICE" -n 250 --no-pager
    ;;
  -h|--help|'')
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
