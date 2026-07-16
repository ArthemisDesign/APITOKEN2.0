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
    '  --with-worker        Stop then start apitoken-worker.service after the API cutover' \
    '  --target-port PORT   Keep the final API on port 3000 or 3001' \
    '  --timeout SECONDS    Readiness deadline per operation (default: 60)' \
    '  --dry-run            Print mutations without changing service state' \
    '  -h, --help           Show this help'
}

DRY_RUN=0
WITH_WORKER=0
REQUESTED_TARGET_PORT=
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
CADDY_GRACE_SECONDS=5
COMMERCE_RELEASE_ROOT=${COMMERCE_RELEASE_ROOT:-/opt/apitoken/releases}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
WORKER_SERVICE=apitoken-worker.service

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

http_returns_200() {
  local url=$1
  local max_time=${2:-2}
  local status

  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time "$max_time" "$url" 2>/dev/null) || return 1
  [[ "$status" == "200" ]]
}

slot_is_ready() {
  local port=$1
  local max_time=${2:-2}
  local unit
  unit=$(slot_unit "$port")
  unit_is_active "$unit" && http_returns_200 "$(slot_url "$port")" "$max_time"
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

require_worker_active() {
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would require $WORKER_SERVICE to be active"
    return 0
  fi
  unit_is_active "$WORKER_SERVICE" || die "$WORKER_SERVICE is not active after start"
  log "$WORKER_SERVICE is active"
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
CUTOVER_ACTIVE=0
CUTOVER_COMMITTED=0
WORKER_TOUCHED=0

recover_slot_without_downtime() {
  local recovery_port=$1
  local failed_target=$2

  if ! slot_is_ready "$recovery_port"; then
    warn "recovery starting $(slot_unit "$recovery_port") before touching the target slot"
    systemctl_raw start "$(slot_unit "$recovery_port")" || true
    if ! wait_for_slot "$recovery_port" "$CURRENT_RELEASE" availability "$READINESS_TIMEOUT"; then
      warn "recovery start did not restore port $recovery_port; attempting a restart"
      systemctl_raw restart "$(slot_unit "$recovery_port")" || true
      wait_for_slot "$recovery_port" "$CURRENT_RELEASE" availability "$READINESS_TIMEOUT" || return 1
    fi
  fi

  if [[ "$failed_target" != "$recovery_port" ]]; then
    if systemctl_raw stop "$(slot_unit "$failed_target")"; then
      log "recovery stopped failed target $(slot_unit "$failed_target") after port $recovery_port was ready"
    else
      warn "recovery could not stop failed target $(slot_unit "$failed_target")"
    fi
  fi
  return 0
}

recover_cutover() {
  local recovery_failed=0
  local fallback_port

  if [[ "$DRY_RUN" == "1" ]]; then
    warn "dry-run failure: no service state was changed, so rollback has no mutations to perform"
    return 0
  fi

  if [[ "$API_SWITCH_NEEDED" == "1" ]]; then
    if [[ -n "$ACTIVE_PORT" ]]; then
      if ! recover_slot_without_downtime "$ACTIVE_PORT" "$TARGET_PORT"; then
        warn "old slot $ACTIVE_PORT could not be restored; leaving target slot $TARGET_PORT untouched"
        recovery_failed=1
      fi
    else
      # Bootstrap has no old slot. Try the other instance before stopping anything so recovery
      # still prefers at least one ready API even when the requested target cannot start.
      if slot_is_ready "$TARGET_PORT"; then
        warn "bootstrap rollback is leaving target port $TARGET_PORT running because it is the only ready slot"
      else
        fallback_port=$(other_port "$TARGET_PORT")
        warn "bootstrap had no old slot; attempting recovery on fallback port $fallback_port"
        systemctl_raw start "$(slot_unit "$fallback_port")" || true
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
      log "recovery restored $WORKER_SERVICE"
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
    if unit_is_active "$(slot_unit "$TARGET_PORT")"; then
      log "stopping active but unsuitable target $(slot_unit "$TARGET_PORT") before a clean start"
      systemctl_command stop "$(slot_unit "$TARGET_PORT")"
      require_slot_stopped "$TARGET_PORT"
    fi
    log "starting target $(slot_unit "$TARGET_PORT")"
    systemctl_command start "$(slot_unit "$TARGET_PORT")"
    if ! wait_for_slot "$TARGET_PORT" "$CURRENT_RELEASE" current-release "$READINESS_TIMEOUT"; then
      die "target port $TARGET_PORT never became ready on the current release"
    fi
  fi

  log "waiting ${CADDY_GRACE_SECONDS}s for Caddy to health-include target port $TARGET_PORT"
  run sleep "$CADDY_GRACE_SECONDS"

  if [[ "$DRY_RUN" != "1" ]] && ! slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
    die "target port $TARGET_PORT lost readiness before the old slot could be drained"
  fi

  if [[ -n "$ACTIVE_PORT" ]]; then
    if slot_is_ready "$ACTIVE_PORT"; then
      log "both API slots are healthy after the Caddy inclusion grace"
    else
      warn "old port $ACTIVE_PORT is no longer ready; target port $TARGET_PORT remains the verified serving slot"
    fi
    log "draining and stopping old $(slot_unit "$ACTIVE_PORT")"
    systemctl_command stop "$(slot_unit "$ACTIVE_PORT")"
    require_slot_stopped "$ACTIVE_PORT"
  else
    log "bootstrap target port $TARGET_PORT is ready; there is no old slot to drain"
  fi
fi

if [[ "$WITH_WORKER" == "1" ]]; then
  WORKER_TOUCHED=1
  log "stopping old $WORKER_SERVICE before starting its new process (worker overlap is forbidden)"
  systemctl_command stop "$WORKER_SERVICE"
  require_worker_stopped
  log "starting new $WORKER_SERVICE"
  systemctl_command start "$WORKER_SERVICE"
  require_worker_active
fi

if [[ "$DRY_RUN" != "1" ]] && ! slot_serves_current_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
  die "target port $TARGET_PORT is not ready on the current release at final verification"
fi

commit_cutover
if [[ "$DRY_RUN" == "1" ]]; then
  log "dry-run complete; no API or worker service state changed"
else
  log "blue-green API cutover complete; port $TARGET_PORT serves release $(basename -- "$CURRENT_RELEASE")"
fi
