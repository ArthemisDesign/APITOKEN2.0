#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

usage() {
  printf '%s\n' \
    'Usage: api-bluegreen.sh [options]' \
    '' \
    'Cut the commerce API over between health-gated systemd slots.' \
    '' \
    'Options:' \
    '  --with-worker        Restart the immutable worker after the API cutover' \
    '                       (it runs from /opt/apitoken/releases/current)' \
    '  --target-port PORT   Keep the final API on port 3000 or 3001' \
    '  --timeout SECONDS    Readiness deadline per operation (default: 60)' \
    '  --dry-run            Print mutations without changing service state' \
    '  -h, --help           Show this help'
}

DRY_RUN=0
WITH_WORKER=0
REQUESTED_TARGET_PORT=
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
# Caddy health_interval=2s and health_timeout=2s. Six seconds covers a full
# active-check window plus margin both when admitting and depooling a slot.
CADDY_HEALTH_WINDOW_SECONDS=6
PRE_DRAIN_SECONDS=6
COMMERCE_RELEASE_ROOT=${COMMERCE_RELEASE_ROOT:-/opt/apitoken/releases}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
WORKER_SERVICE=apitoken-worker.service
WORKER_SOURCE_ROOT=$COMMERCE_RELEASE_ROOT/current

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-worker)
      WITH_WORKER=1
      shift
      ;;
    --target-port)
      [[ $# -ge 2 ]] || die "--target-port requires a value"
      REQUESTED_TARGET_PORT=$2
      shift 2
      ;;
    --timeout)
      [[ $# -ge 2 ]] || die "--timeout requires a value"
      READINESS_TIMEOUT=$2
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --*)
      die "unknown option: $1"
      ;;
    *)
      die "unexpected argument: $1"
      ;;
  esac
done

validate_api_port() {
  case "$1" in
    3000|3001) ;;
    *) die "API slot port must be 3000 or 3001: $1" ;;
  esac
}

other_port() {
  case "$1" in
    3000) printf '3001\n' ;;
    3001) printf '3000\n' ;;
    *) return 1 ;;
  esac
}

slot_unit() {
  printf 'apitoken-api@%s.service\n' "$1"
}

slot_url() {
  printf 'http://127.0.0.1:%s/v1/ready\n' "$1"
}

unit_is_active() {
  systemctl_raw is-active --quiet "$1" >/dev/null 2>&1
}

http_returns_status() {
  local url=$1
  local expected_status=$2
  local max_time=${3:-2}
  local status

  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time "$max_time" "$url" 2>/dev/null) || return 1
  [[ "$status" == "$expected_status" ]]
}

http_returns_200() {
  http_returns_status "$1" 200 "${2:-2}"
}

http_returns_503() {
  http_returns_status "$1" 503 "${2:-2}"
}

slot_is_ready() {
  local port=$1
  local max_time=${2:-2}
  local unit
  unit=$(slot_unit "$port")
  unit_is_active "$unit" && http_returns_200 "$(slot_url "$port")" "$max_time"
}

slot_is_draining() {
  local port=$1
  local max_time=${2:-2}
  local unit
  unit=$(slot_unit "$port")
  unit_is_active "$unit" && http_returns_503 "$(slot_url "$port")" "$max_time"
}

slot_serves_current_release() {
  local port=$1
  local expected_release=$2
  local max_time=${3:-2}
  local unit
  unit=$(slot_unit "$port")
  unit_release_binding_ok api "$unit" "$COMMERCE_RELEASE_ROOT" "$expected_release" \
    && http_returns_200 "$(slot_url "$port")" "$max_time"
}

wait_for_slot() {
  local port=$1
  local expected_release=$2
  local binding_mode=$3
  local timeout=$4
  local interval=${READINESS_INTERVAL_SECONDS:-2}
  local unit url deadline now remaining sleep_for curl_timeout

  validate_api_port "$port"
  validate_timeout "$timeout"
  validate_readiness_interval "$interval"
  unit=$(slot_unit "$port")
  url=$(slot_url "$port")

  if [[ "$DRY_RUN" == "1" ]]; then
    if [[ "$binding_mode" == "current-release" ]]; then
      log "dry-run: would require $unit active, bound to $expected_release through releases/current, and returning HTTP 200 at $url within ${timeout}s"
    else
      log "dry-run: would require $unit active and returning HTTP 200 at $url within ${timeout}s"
    fi
    return 0
  fi

  deadline=$(( $(date +%s) + timeout ))
  while true; do
    now=$(date +%s)
    remaining=$(( deadline - now ))
    (( remaining > 0 )) || break

    curl_timeout=$remaining
    (( curl_timeout > 2 )) && curl_timeout=2
    if [[ "$binding_mode" == "current-release" ]]; then
      if slot_serves_current_release "$port" "$expected_release" "$curl_timeout"; then
        log "$unit is active on release $(basename -- "$expected_release") and returns HTTP 200 at $url"
        return 0
      fi
    elif slot_is_ready "$port" "$curl_timeout"; then
      log "$unit is active and returns HTTP 200 at $url"
      return 0
    fi

    now=$(date +%s)
    remaining=$(( deadline - now ))
    (( remaining > 0 )) || break
    sleep_for=$interval
    (( sleep_for > remaining )) && sleep_for=$remaining
    sleep "$sleep_for"
  done

  warn "$unit did not become active and return HTTP 200 at $url within ${timeout}s"
  return 1
}

require_slot_stopped() {
  local port=$1
  local unit
  unit=$(slot_unit "$port")

  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would require $unit to be stopped"
    return 0
  fi
  ! unit_is_active "$unit" || die "$unit is still active after stop"
  log "$unit is stopped"
}

require_slot_draining() {
  local port=$1
  local unit url
  unit=$(slot_unit "$port")
  url=$(slot_url "$port")

  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would require $unit to remain active while $url returns HTTP 503"
    return 0
  fi
  slot_is_draining "$port" || die "$unit did not remain active with HTTP 503 readiness after pre-drain"
  log "$unit remains active for in-flight requests and returns HTTP 503 at $url"
}

start_slot_fresh() {
  local port=$1
  local unit
  unit=$(slot_unit "$port")

  # The target is the inactive slot. Always stop its unit before starting so a
  # stale process cannot keep the port or continue running the previous release.
  log "stopping inactive target $unit before a fresh start"
  systemctl_command stop "$unit"
  require_slot_stopped "$port"
  log "starting target $unit from releases/current"
  systemctl_command start "$unit"
}

require_worker_active() {
  local worker_pid worker_cwd deadline
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would require $WORKER_SERVICE to be active"
    return 0
  fi
  deadline=$(( $(date +%s) + READINESS_TIMEOUT ))
  while (( $(date +%s) < deadline )); do
    if unit_is_active "$WORKER_SERVICE"; then
      worker_pid=$(systemctl show "$WORKER_SERVICE" -p MainPID --value)
      if [[ $worker_pid =~ ^[1-9][0-9]*$ ]]; then
        worker_cwd=$(readlink -f -- "/proc/$worker_pid/cwd" 2>/dev/null || true)
        if [[ $worker_cwd == "$CURRENT_RELEASE/apps/worker" ]]; then
          log "$WORKER_SERVICE is active from immutable release $CURRENT_RELEASE"
          return 0
        fi
      fi
    fi
    sleep 1
  done
  die "$WORKER_SERVICE did not become active on immutable release $CURRENT_RELEASE within ${READINESS_TIMEOUT}s"
}

require_worker_stopped() {
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would require $WORKER_SERVICE to be stopped before its new process starts"
    return 0
  fi
  ! unit_is_active "$WORKER_SERVICE" || die "$WORKER_SERVICE is still active after stop"
  log "$WORKER_SERVICE is stopped; no old worker process overlaps the new one"
}

ACTIVE_PORT=
TARGET_PORT=
CURRENT_RELEASE=
API_SWITCH_NEEDED=1
NEW_SLOT_COMMITTED=0
OLD_DRAIN_SIGNALLED=0
OLD_SLOT_STOPPED=0
CUTOVER_ACTIVE=0
CUTOVER_COMMITTED=0
WORKER_TOUCHED=0

stop_failed_target_after_old_ready() {
  local old_port=$1
  local failed_target=$2

  [[ "$failed_target" != "$old_port" ]] || return 0
  if systemctl_raw stop "$(slot_unit "$failed_target")"; then
    log "recovery stopped only the failed new slot $(slot_unit "$failed_target"); old port $old_port remains ready"
  else
    warn "recovery could not stop failed new slot $(slot_unit "$failed_target")"
    return 1
  fi
}

restart_old_slot_on_current_loudly() {
  local old_port=$1
  local old_unit
  old_unit=$(slot_unit "$old_port")

  warn "CRITICAL: old slot $old_unit is no longer ready, so preserving availability now requires a restart"
  warn "CRITICAL: restarting $old_unit WILL launch releases/current ($(basename -- "$CURRENT_RELEASE")), not the old process's original in-memory release"
  systemctl_raw restart "$old_unit" || return 1
  wait_for_slot "$old_port" "$CURRENT_RELEASE" current-release "$READINESS_TIMEOUT"
}

recover_with_old_slot() {
  local old_port=$1
  local failed_target=$2
  local old_unit
  old_unit=$(slot_unit "$old_port")

  # Before pre-drain, rollback never restarts the old process: it is still the
  # availability anchor even though releases/current already points at the new SHA.
  if slot_is_ready "$old_port"; then
    log "recovery confirmed old slot $old_unit is still running and ready"
    stop_failed_target_after_old_ready "$old_port" "$failed_target" || return 1
    wait_for_slot "$old_port" "$CURRENT_RELEASE" availability "$READINESS_TIMEOUT"
    return
  fi

  # After the new slot was committed and the old slot entered drain/stopped, the
  # safe recovery direction is forward. Never restart old onto the moved symlink
  # while the verified new process can preserve availability.
  if [[ "$OLD_DRAIN_SIGNALLED" == "1" || "$OLD_SLOT_STOPPED" == "1" ]]; then
    warn "old slot $old_unit had already entered the committed drain phase (signalled=$OLD_DRAIN_SIGNALLED stopped=$OLD_SLOT_STOPPED)"
  fi
  if slot_serves_current_release "$failed_target" "$CURRENT_RELEASE"; then
    warn "old slot $old_unit is no longer ready; retaining verified target $(slot_unit "$failed_target") (new-slot-committed=$NEW_SLOT_COMMITTED)"
    if unit_is_active "$old_unit" && [[ "$NEW_SLOT_COMMITTED" == "1" ]]; then
      if systemctl_raw stop "$old_unit"; then
        log "recovery stopped the non-ready old slot after confirming the committed target"
      else
        warn "recovery could not stop the non-ready old slot $old_unit"
        return 1
      fi
    elif unit_is_active "$old_unit"; then
      warn "new slot was not committed before the abort; leaving the non-ready old unit running for operator inspection"
    fi
    return 0
  fi

  # This is the only case where restarting OLD is availability-improving: it has
  # already died/drained and the new slot is not ready. Be explicit that the
  # symlink has moved, then verify the restarted process against current.
  restart_old_slot_on_current_loudly "$old_port" || return 1
  stop_failed_target_after_old_ready "$old_port" "$failed_target"
}

recover_cutover() {
  local recovery_failed=0
  local fallback_port fallback_unit

  if [[ "$DRY_RUN" == "1" ]]; then
    warn "dry-run failure: no service state was changed, so rollback has no mutations to perform"
    return 0
  fi

  if [[ "$API_SWITCH_NEEDED" == "1" ]]; then
    if [[ -n "$ACTIVE_PORT" ]]; then
      if ! recover_with_old_slot "$ACTIVE_PORT" "$TARGET_PORT"; then
        warn "automatic API recovery was incomplete"
        recovery_failed=1
      fi
    else
      # Bootstrap has no old slot. Try the other instance before stopping anything so recovery
      # still prefers at least one ready API even when the requested target cannot start.
      if slot_is_ready "$TARGET_PORT"; then
        warn "bootstrap rollback is leaving target port $TARGET_PORT running because it is the only ready slot"
      else
        fallback_port=$(other_port "$TARGET_PORT")
        fallback_unit=$(slot_unit "$fallback_port")
        warn "bootstrap had no old slot; attempting a fresh recovery start on fallback port $fallback_port"
        systemctl_raw stop "$fallback_unit" || true
        systemctl_raw start "$fallback_unit" || true
        if wait_for_slot "$fallback_port" "$CURRENT_RELEASE" current-release "$READINESS_TIMEOUT"; then
          systemctl_raw stop "$(slot_unit "$TARGET_PORT")" || warn "could not stop failed bootstrap target port $TARGET_PORT"
        else
          warn "fallback port $fallback_port also failed; leaving both units untouched for operator recovery"
          recovery_failed=1
        fi
      fi
    fi
  fi

  if [[ "$WORKER_TOUCHED" == "1" ]] && ! unit_is_active "$WORKER_SERVICE"; then
    if systemctl_raw start "$WORKER_SERVICE" && unit_is_active "$WORKER_SERVICE"; then
      log "recovery restored immutable $WORKER_SERVICE"
    else
      warn "recovery could not restore $WORKER_SERVICE"
      recovery_failed=1
    fi
  fi

  if slot_is_ready 3000 || slot_is_ready 3001; then
    log "recovery verified that at least one API slot is ready"
  else
    warn "CRITICAL: recovery could not establish a ready API slot; immediate operator intervention is required"
    recovery_failed=1
  fi
  return "$recovery_failed"
}

cutover_abort() {
  local status=$1
  local reason=$2

  trap - ERR EXIT INT TERM
  (( status != 0 )) || status=1
  set +e

  if [[ "$CUTOVER_ACTIVE" == "1" && "$CUTOVER_COMMITTED" != "1" ]]; then
    warn "blue-green cutover aborted by $reason; starting availability-first rollback"
    recover_cutover || warn "automatic rollback was incomplete"
  fi
  exit "$status"
}

begin_cutover() {
  CUTOVER_ACTIVE=1
  CUTOVER_COMMITTED=0
  trap 'cutover_abort "$?" ERR' ERR
  trap 'cutover_abort "$?" EXIT' EXIT
  trap 'cutover_abort 130 INT' INT
  trap 'cutover_abort 143 TERM' TERM
}

commit_cutover() {
  CUTOVER_COMMITTED=1
  CUTOVER_ACTIVE=0
  trap - ERR EXIT INT TERM
}

validate_timeout "$READINESS_TIMEOUT"
validate_readiness_interval "${READINESS_INTERVAL_SECONDS:-2}"
if [[ -n "$REQUESTED_TARGET_PORT" ]]; then
  validate_api_port "$REQUESTED_TARGET_PORT"
fi
validate_service_unit "$(slot_unit 3000)"
validate_service_unit "$(slot_unit 3001)"
validate_service_unit "$WORKER_SERVICE"
COMMERCE_RELEASE_ROOT=$(canonicalize_release_root "$COMMERCE_RELEASE_ROOT" /opt/apitoken commerce)

log "preflighting blue-green API cutover (dry-run=$DRY_RUN with-worker=$WITH_WORKER target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
CURRENT_RELEASE=$(required_current_release_path "$COMMERCE_RELEASE_ROOT")
validate_release_marker "$CURRENT_RELEASE" "$(basename -- "$CURRENT_RELEASE")"
[[ -r "$CURRENT_RELEASE/apps/api/dist/main.js" ]] || die "commerce API artifact is missing: $CURRENT_RELEASE/apps/api/dist/main.js"
log "current points to validated release $(basename -- "$CURRENT_RELEASE")"

READY_3000=0
READY_3001=0
slot_is_ready 3000 && READY_3000=1
slot_is_ready 3001 && READY_3001=1

if [[ -n "$REQUESTED_TARGET_PORT" ]]; then
  TARGET_PORT=$REQUESTED_TARGET_PORT
  OTHER_PORT=$(other_port "$TARGET_PORT")
  if [[ "$TARGET_PORT" == "3000" ]]; then
    TARGET_READY=$READY_3000
    OTHER_READY=$READY_3001
  else
    TARGET_READY=$READY_3001
    OTHER_READY=$READY_3000
  fi

  if [[ "$OTHER_READY" == "1" ]]; then
    ACTIVE_PORT=$OTHER_PORT
  elif [[ "$TARGET_READY" == "1" ]]; then
    if slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
      API_SWITCH_NEEDED=0
    else
      die "requested target port $TARGET_PORT is the only ready slot but is not bound to current; target the other port to preserve availability"
    fi
  else
    ACTIVE_PORT=
  fi
else
  case "$READY_3000:$READY_3001" in
    1:0)
      ACTIVE_PORT=3000
      TARGET_PORT=3001
      ;;
    0:1)
      ACTIVE_PORT=3001
      TARGET_PORT=3000
      ;;
    0:0)
      ACTIVE_PORT=
      TARGET_PORT=3000
      ;;
    1:1)
      ACTIVE_PORT=3000
      TARGET_PORT=3001
      log "both slots are already healthy; defaulting to retain 3001 (use --target-port to choose explicitly)"
      ;;
  esac
fi

if [[ "$API_SWITCH_NEEDED" == "0" ]]; then
  log "target port $TARGET_PORT already serves current release $(basename -- "$CURRENT_RELEASE"); API cutover is already complete"
elif [[ -n "$ACTIVE_PORT" ]]; then
  log "cutover decision: active $ACTIVE_PORT -> target $TARGET_PORT"
else
  log "cutover decision: no ready API slot detected; bootstrap target is $TARGET_PORT"
fi

begin_cutover

if [[ "$API_SWITCH_NEEDED" == "1" ]]; then
  if slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
    log "target $(slot_unit "$TARGET_PORT") already serves the current release; reusing it"
  else
    start_slot_fresh "$TARGET_PORT"
  fi
  if ! wait_for_slot "$TARGET_PORT" "$CURRENT_RELEASE" current-release "$READINESS_TIMEOUT"; then
    die "target port $TARGET_PORT never became ready on the current release"
  fi

  log "waiting ${CADDY_HEALTH_WINDOW_SECONDS}s for Caddy to health-include target port $TARGET_PORT"
  run sleep "$CADDY_HEALTH_WINDOW_SECONDS"

  if [[ "$DRY_RUN" != "1" ]] && ! slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
    die "target port $TARGET_PORT lost readiness before Caddy inclusion could be committed"
  fi
  NEW_SLOT_COMMITTED=1
  log "new slot $(slot_unit "$TARGET_PORT") is ready on releases/current and has passed the Caddy inclusion window"

  if [[ -n "$ACTIVE_PORT" ]]; then
    if slot_is_ready "$ACTIVE_PORT"; then
      log "old slot $(slot_unit "$ACTIVE_PORT") is still ready; committing the new slot before pre-drain"
    else
      warn "old port $ACTIVE_PORT died before pre-drain; committed target port $TARGET_PORT remains the verified serving slot"
    fi

    log "pre-draining old $(slot_unit "$ACTIVE_PORT") with SIGUSR1 before its listener is stopped"
    systemctl_command kill -s SIGUSR1 "$(slot_unit "$ACTIVE_PORT")"
    OLD_DRAIN_SIGNALLED=1
    log "waiting ${PRE_DRAIN_SECONDS}s for Caddy to observe HTTP 503 and depool old port $ACTIVE_PORT"
    run sleep "$PRE_DRAIN_SECONDS"
    require_slot_draining "$ACTIVE_PORT"

    if [[ "$DRY_RUN" != "1" ]] && ! slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
      die "target port $TARGET_PORT lost readiness during old-slot pre-drain"
    fi
    [[ "$NEW_SLOT_COMMITTED" == "1" ]] || die "refusing to stop old slot before the new slot is committed"
    log "new slot is still ready and old slot is depoolable; stopping old $(slot_unit "$ACTIVE_PORT") for bounded application drain"
    systemctl_command stop "$(slot_unit "$ACTIVE_PORT")"
    require_slot_stopped "$ACTIVE_PORT"
    OLD_SLOT_STOPPED=1
  else
    log "bootstrap target port $TARGET_PORT is ready; there is no old slot to pre-drain"
  fi
fi

if [[ "$WITH_WORKER" == "1" ]]; then
  WORKER_TOUCHED=1
  log "stopping immutable $WORKER_SERVICE before restart (worker overlap is forbidden)"
  systemctl_command stop "$WORKER_SERVICE"
  require_worker_stopped
  log "starting $WORKER_SERVICE from $WORKER_SOURCE_ROOT"
  systemctl_command start "$WORKER_SERVICE"
  require_worker_active
fi

if [[ "$DRY_RUN" != "1" ]] && ! slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
  die "target port $TARGET_PORT is not ready on the current release at final verification"
fi

# Boot persistence must follow the verified serving slot. Otherwise a reboot can
# resurrect the stopped instance while leaving the selected one disabled.
systemctl_command enable "$(slot_unit "$TARGET_PORT")"
if [[ -n "$ACTIVE_PORT" && "$ACTIVE_PORT" != "$TARGET_PORT" ]]; then
  systemctl_command disable "$(slot_unit "$ACTIVE_PORT")"
fi

commit_cutover
if [[ "$DRY_RUN" == "1" ]]; then
  log "dry-run complete; no API or worker service state changed"
else
  log "blue-green API cutover complete; port $TARGET_PORT serves release $(basename -- "$CURRENT_RELEASE")"
fi
