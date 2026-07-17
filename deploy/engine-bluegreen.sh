#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

usage() {
  printf '%s\n' \
    'Usage: engine-bluegreen.sh [--target-port 8787|8788] [--timeout SECONDS] [--dry-run]' \
    '' \
    'Health-gated PostgreSQL engine cutover. Start target, let Caddy include it, pre-drain old,' \
    'then stop old only after target readiness is reverified.'
}

DRY_RUN=0
REQUESTED_TARGET_PORT=
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
HEALTH_WINDOW_SECONDS=${CADDY_HEALTH_WINDOW_SECONDS:-6}
PRE_DRAIN_SECONDS=${ENGINE_PRE_DRAIN_SECONDS:-6}
ENGINE_RELEASE_ROOT=${ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
POSTGRES_ENV=${ENGINE_POSTGRES_ENV:-/srv/claude-api/data/engine-postgres.env}
CADDY_CONFIG=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
CONTROL_READY_URL=${ENGINE_CONTROL_READY_URL:-http://127.0.0.1:8790/ready}
LEGACY_UNIT=claude-api.service
CURRENT_RELEASE=
ACTIVE_PORT=
ACTIVE_UNIT=
TARGET_PORT=
TARGET_UNIT=
TARGET_COMMITTED=0
OLD_SIGNALLED=0
CUTOVER_ACTIVE=0
CUTOVER_COMMITTED=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target-port) [[ $# -ge 2 ]] || die "--target-port requires a value"; REQUESTED_TARGET_PORT=$2; shift 2 ;;
    --timeout) [[ $# -ge 2 ]] || die "--timeout requires a value"; READINESS_TIMEOUT=$2; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    --*) die "unknown option: $1" ;;
    *) die "unexpected argument: $1" ;;
  esac
done

validate_port() { [[ $1 == 8787 || $1 == 8788 ]] || die "engine slot port must be 8787 or 8788: $1"; }
other_port() { [[ $1 == 8787 ]] && printf '8788\n' || printf '8787\n'; }
slot_unit() { printf 'claude-api@%s.service\n' "$1"; }
slot_url() { printf 'http://127.0.0.1:%s/ready\n' "$1"; }
unit_active() { systemctl_raw is-active --quiet "$1" >/dev/null 2>&1; }
ready_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 "$(slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 200 ]]
}
draining_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 "$(slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 503 ]]
}
control_ready() {
  local status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 "$CONTROL_READY_URL" 2>/dev/null) || return 1
  [[ $status == 200 ]]
}
unit_for_ready_port() {
  local port=$1 template
  template=$(slot_unit "$port")
  if unit_active "$template" && ready_port "$port"; then printf '%s\n' "$template"; return 0; fi
  if [[ $port == 8787 ]] && unit_active "$LEGACY_UNIT" && ready_port "$port"; then
    printf '%s\n' "$LEGACY_UNIT"; return 0
  fi
  return 1
}
slot_serves_current() {
  local port=$1 unit
  unit=$(slot_unit "$port")
  unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" \
    && ready_port "$port"
}
wait_target() {
  local deadline=$(( $(date +%s) + READINESS_TIMEOUT ))
  if [[ $DRY_RUN == 1 ]]; then
    log "dry-run: would require $TARGET_UNIT on current release and HTTP 200 at $(slot_url "$TARGET_PORT")"
    return 0
  fi
  while (( $(date +%s) < deadline )); do
    if slot_serves_current "$TARGET_PORT"; then return 0; fi
    sleep 1
  done
  journalctl -u "$TARGET_UNIT" -n 60 --no-pager >&2 || true
  return 1
}

recover() {
  local failed=0
  [[ $DRY_RUN == 0 ]] || return 0
  if [[ $TARGET_COMMITTED == 1 ]] && slot_serves_current "$TARGET_PORT"; then
    warn "recovery retains the verified target $TARGET_UNIT"
    if [[ -n $ACTIVE_UNIT && $ACTIVE_UNIT != "$TARGET_UNIT" ]]; then
      systemctl_raw stop "$ACTIVE_UNIT" || failed=1
    fi
    return "$failed"
  fi
  systemctl_raw stop "$TARGET_UNIT" || failed=1
  if [[ -n $ACTIVE_UNIT ]]; then
    if unit_active "$ACTIVE_UNIT" && ready_port "$ACTIVE_PORT"; then
      log "recovery preserved old ready unit $ACTIVE_UNIT"
    elif [[ $OLD_SIGNALLED == 1 ]]; then
      warn "old unit was pre-drained; restarting it through current to restore readiness"
      systemctl_raw restart "$ACTIVE_UNIT" || failed=1
      for _ in $(seq 1 "$READINESS_TIMEOUT"); do ready_port "$ACTIVE_PORT" && return "$failed"; sleep 1; done
      failed=1
    fi
  fi
  return "$failed"
}
abort_cutover() {
  local rc=$1 reason=$2
  trap - ERR EXIT INT TERM
  (( rc != 0 )) || rc=1
  set +e
  if [[ $CUTOVER_ACTIVE == 1 && $CUTOVER_COMMITTED == 0 ]]; then
    warn "engine blue-green cutover aborted by $reason"
    recover || warn "automatic engine recovery was incomplete"
  fi
  exit "$rc"
}
begin_cutover() {
  CUTOVER_ACTIVE=1
  trap 'abort_cutover "$?" ERR' ERR
  trap 'abort_cutover "$?" EXIT' EXIT
  trap 'abort_cutover 130 INT' INT
  trap 'abort_cutover 143 TERM' TERM
}
commit_cutover() { CUTOVER_COMMITTED=1; CUTOVER_ACTIVE=0; trap - ERR EXIT INT TERM; }

validate_timeout "$READINESS_TIMEOUT"
validate_readiness_interval "${READINESS_INTERVAL_SECONDS:-2}"
[[ -z $REQUESTED_TARGET_PORT ]] || validate_port "$REQUESTED_TARGET_PORT"
validate_service_unit "$LEGACY_UNIT"
validate_service_unit "$(slot_unit 8787)"
validate_service_unit "$(slot_unit 8788)"
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "$ENGINE_RELEASE_ROOT" /srv/claude-api engine)

log "preflighting PostgreSQL engine blue-green cutover (dry-run=$DRY_RUN target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
privileged_command test -s "$POSTGRES_ENV" || die "PostgreSQL authority is not active: $POSTGRES_ENV"
privileged_command test -f /etc/systemd/system/claude-api@.service || die "engine slot template is not installed"
privileged_command caddy validate --adapter caddyfile --config "$CADDY_CONFIG" >/dev/null
privileged_command grep -q '127.0.0.1:8788' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the 8788 engine slot"
privileged_command grep -q '127.0.0.1:8790' "$CADDY_CONFIG" \
  || die "Caddy is missing the stable loopback Control API listener on 127.0.0.1:8790"
if [[ $DRY_RUN == 0 ]]; then
  control_ready || die "stable Control API is not ready at $CONTROL_READY_URL"
fi
CURRENT_RELEASE=$(required_current_release_path "$ENGINE_RELEASE_ROOT")
validate_release_marker "$CURRENT_RELEASE" "$(basename -- "$CURRENT_RELEASE")"
[[ -x "$CURRENT_RELEASE/claude-api" ]] || die "current engine binary is missing"

READY_8787=0; READY_8788=0
unit_for_ready_port 8787 >/dev/null && READY_8787=1
unit_for_ready_port 8788 >/dev/null && READY_8788=1

if [[ -n $REQUESTED_TARGET_PORT ]]; then
  TARGET_PORT=$REQUESTED_TARGET_PORT
  OTHER=$(other_port "$TARGET_PORT")
  if unit_for_ready_port "$OTHER" >/dev/null; then
    ACTIVE_PORT=$OTHER; ACTIVE_UNIT=$(unit_for_ready_port "$OTHER")
  elif slot_serves_current "$TARGET_PORT"; then
    log "requested target already exclusively serves current release"
  elif unit_for_ready_port "$TARGET_PORT" >/dev/null; then
    die "requested target is the only ready old slot; choose $OTHER to preserve availability"
  fi
else
  case "$READY_8787:$READY_8788" in
    1:0) ACTIVE_PORT=8787; ACTIVE_UNIT=$(unit_for_ready_port 8787); TARGET_PORT=8788 ;;
    0:1) ACTIVE_PORT=8788; ACTIVE_UNIT=$(unit_for_ready_port 8788); TARGET_PORT=8787 ;;
    0:0) TARGET_PORT=8787 ;;
    1:1)
      if slot_serves_current 8788; then TARGET_PORT=8788; ACTIVE_PORT=8787; ACTIVE_UNIT=$(unit_for_ready_port 8787)
      else TARGET_PORT=8787; ACTIVE_PORT=8788; ACTIVE_UNIT=$(unit_for_ready_port 8788); fi
      ;;
  esac
fi
TARGET_UNIT=$(slot_unit "$TARGET_PORT")
log "cutover decision: ${ACTIVE_UNIT:-no ready old unit} -> $TARGET_UNIT"

begin_cutover
if ! slot_serves_current "$TARGET_PORT"; then
  systemctl_command stop "$TARGET_UNIT"
  systemctl_command start "$TARGET_UNIT"
fi
wait_target || die "$TARGET_UNIT did not become ready on current release"
log "waiting ${HEALTH_WINDOW_SECONDS}s for Caddy to health-include $TARGET_PORT"
run sleep "$HEALTH_WINDOW_SECONDS"
if [[ $DRY_RUN == 0 ]]; then
  slot_serves_current "$TARGET_PORT" || die "target lost readiness during Caddy inclusion"
  control_ready || die "stable Control API lost readiness while admitting the target"
fi
TARGET_COMMITTED=1

if [[ -n $ACTIVE_UNIT && $ACTIVE_UNIT != "$TARGET_UNIT" ]]; then
  log "pre-draining $ACTIVE_UNIT with SIGUSR1"
  systemctl_command kill -s SIGUSR1 "$ACTIVE_UNIT"
  OLD_SIGNALLED=1
  run sleep "$PRE_DRAIN_SECONDS"
  if [[ $DRY_RUN == 0 ]]; then draining_port "$ACTIVE_PORT" || die "old engine did not flip readiness to 503"; fi
  if [[ $DRY_RUN == 0 ]]; then
    slot_serves_current "$TARGET_PORT" || die "target lost readiness during old-engine pre-drain"
    control_ready || die "stable Control API lost readiness during old-engine pre-drain"
  fi
  systemctl_command stop "$ACTIVE_UNIT"
  systemctl_command disable "$ACTIVE_UNIT"
fi
systemctl_command enable "$TARGET_UNIT"
if [[ $DRY_RUN == 0 ]]; then
  OTHER_PORT=$(other_port "$TARGET_PORT")
  OTHER_UNIT=$(slot_unit "$OTHER_PORT")
  slot_serves_current "$TARGET_PORT" || die "target failed final exact-release verification"
  control_ready || die "stable Control API failed final readiness verification"
  unit_active "$OTHER_UNIT" && die "inactive engine slot remains active after cutover: $OTHER_UNIT"
  ready_port "$OTHER_PORT" && die "inactive engine slot remains ready after cutover: $OTHER_UNIT"
  # Repair boot-time drift as well as live drift. Disabling an already inactive unit is safe and
  # prevents an out-of-band enable from resurrecting a second writer after the next host reboot.
  systemctl_command disable "$OTHER_UNIT"
  systemctl_raw is-enabled --quiet "$TARGET_UNIT" \
    || die "target engine slot is not enabled after cutover: $TARGET_UNIT"
  if systemctl_raw is-enabled --quiet "$OTHER_UNIT"; then
    die "inactive engine slot remains enabled after cutover: $OTHER_UNIT"
  fi
  unit_active "$LEGACY_UNIT" && die "legacy engine unit remains active after cutover"
  if systemctl_raw is-enabled --quiet "$LEGACY_UNIT"; then
    systemctl_command disable "$LEGACY_UNIT"
  fi
fi

commit_cutover
if [[ $DRY_RUN == 1 ]]; then
  log "dry-run complete; no engine or Caddy state changed"
else
  log "engine blue-green cutover complete; $TARGET_UNIT serves $(basename -- "$CURRENT_RELEASE")"
fi
