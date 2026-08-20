#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

usage() {
  printf '%s\n' \
    'Usage: router-bluegreen.sh [--target-port 8800|8801] [--timeout SECONDS] [--dry-run]' \
    '' \
    'Start and exact-release verify an inactive router slot, atomically promote it in Caddy,' \
    'then gracefully drain and retire the previous slot or legacy singleton.'
}

DRY_RUN=0
REQUESTED_TARGET_PORT=
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
ENGINE_RELEASE_ROOT=${ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
CADDY_CONFIG=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
ACTIVE_SNIPPET=${ROUTER_ACTIVE_SNIPPET:-/etc/caddy/router-active.caddy}
PROMOTE_HELPER=${ROUTER_PROMOTE_HELPER:-/usr/local/lib/apitoken-watchdog/controller/router-promote.sh}
STABLE_READY_URL=${ROUTER_STABLE_READY_URL:-http://127.0.0.1:8802/ready}
STABLE_STARTUP_URL=${ROUTER_STABLE_STARTUP_URL:-http://127.0.0.1:8802/startup}
LEGACY_UNIT=claude-router.service
ACTIVE_PORT=
ACTIVE_UNIT=
TARGET_PORT=
TARGET_UNIT=
TARGET_STARTED=0
HEADROOM_HELPER=${LARGE_PAYLOAD_HEADROOM_HELPER:-/usr/local/lib/apitoken-watchdog/controller/large-payload-headroom.sh}
PAYLOAD_GATE=${LARGE_PAYLOAD_CANDIDATE_GATE:-/usr/local/lib/apitoken-watchdog/controller/large-payload-candidate-gate.sh}
PAYLOAD_EVIDENCE_DIR=${LARGE_PAYLOAD_EVIDENCE_DIR:-/var/lib/apitoken/watchdog/large-payload}
PAYLOAD_MEMORY_HIGH_BYTES=${LARGE_PAYLOAD_ROUTER_MEMORY_HIGH_BYTES:-6442450944}
PROMOTED=0
CUTOVER_ACTIVE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target-port) [[ $# -ge 2 ]] || die '--target-port requires a value'; REQUESTED_TARGET_PORT=$2; shift 2 ;;
    --timeout) [[ $# -ge 2 ]] || die '--timeout requires a value'; READINESS_TIMEOUT=$2; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    --*) die "unknown option: $1" ;;
    *) die "unexpected argument: $1" ;;
  esac
done

validate_port() { [[ $1 == 8800 || $1 == 8801 ]] || die "router slot port must be 8800 or 8801: $1"; }
other_port() { [[ $1 == 8800 ]] && printf '8801\n' || printf '8800\n'; }
slot_unit() { printf 'claude-router@%s.service\n' "$1"; }
slot_url() { printf 'http://127.0.0.1:%s/ready\n' "$1"; }
slot_startup_url() { printf 'http://127.0.0.1:%s/startup\n' "$1"; }
unit_active() { systemctl_raw is-active --quiet "$1" >/dev/null 2>&1; }
ready_port() { curl --noproxy '*' --fail --silent --show-error --max-time 2 "$(slot_url "$1")" >/dev/null 2>&1; }
startup_port() { curl --noproxy '*' --fail --silent --show-error --max-time 3 "$(slot_startup_url "$1")" >/dev/null 2>&1; }

active_backend_port() {
  local ports
  [[ -f $ACTIVE_SNIPPET && ! -L $ACTIVE_SNIPPET ]] || return 1
  [[ $(stat -c '%u' -- "$ACTIVE_SNIPPET" 2>/dev/null) == 0 ]] || return 1
  ports=$(sed -n 's/^[[:space:]]*reverse_proxy 127\.0\.0\.1:\([0-9][0-9]*\)[[:space:]]*$/\1/p' \
    "$ACTIVE_SNIPPET") || return 1
  [[ $ports != *$'\n'* ]] || return 1
  case "$ports" in 8798|8800|8801) printf '%s\n' "$ports" ;; *) return 1 ;; esac
}

slot_serves_release() {
  local port=$1 release=$2 unit pid executable
  unit=$(slot_unit "$port")
  unit_active "$unit" || return 1
  pid=$(systemctl_show_value "$unit" MainPID) || return 1
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  executable=$(realpath -- "/proc/$pid/exe" 2>/dev/null) || return 1
  [[ $executable == "$release/claude-router" ]] || return 1
  ready_port "$port" && startup_port "$port"
}

stable_ready() { curl --noproxy '*' --fail --silent --show-error --max-time 3 "$STABLE_READY_URL" >/dev/null 2>&1; }
stable_startup() { curl --noproxy '*' --fail --silent --show-error --max-time 3 "$STABLE_STARTUP_URL" >/dev/null 2>&1; }

wait_target() {
  local deadline=$(( $(date +%s) + READINESS_TIMEOUT ))
  if [[ $DRY_RUN == 1 ]]; then
    log "dry-run: would require $TARGET_UNIT to execute $CURRENT_RELEASE/claude-router and return 200 at $(slot_url "$TARGET_PORT") and $(slot_startup_url "$TARGET_PORT")"
    return 0
  fi
  while (( $(date +%s) < deadline )); do
    slot_serves_release "$TARGET_PORT" "$CURRENT_RELEASE" && return 0
    sleep 1
  done
  journalctl -u "$TARGET_UNIT" -n 60 --no-pager >&2 || true
  return 1
}

stop_unit() {
  local unit=$1
  systemctl_command stop "$unit"
  if [[ $DRY_RUN == 0 ]] && unit_active "$unit"; then
    die "$unit remains active after its bounded graceful drain"
  fi
}

recover() {
  local failed=0
  [[ $DRY_RUN == 0 ]] || return 0
  if [[ $PROMOTED == 0 ]]; then
    if [[ $TARGET_STARTED == 1 ]]; then
      warn "recovery stopping unadmitted router target $TARGET_UNIT"
      systemctl_raw stop "$TARGET_UNIT" || failed=1
      systemctl_raw disable "$TARGET_UNIT" || failed=1
    fi
  elif slot_serves_release "$TARGET_PORT" "$CURRENT_RELEASE" && stable_ready; then
    warn "recovery retains promoted router target $TARGET_UNIT; reverting would interrupt new streams"
    systemctl_raw enable "$TARGET_UNIT" || failed=1
  elif [[ -n $ACTIVE_PORT ]] && ready_port "$ACTIVE_PORT"; then
    warn "promoted target failed before predecessor drain; restoring Caddy to $ACTIVE_PORT"
    privileged_command "$PROMOTE_HELPER" "$ACTIVE_PORT" || failed=1
    systemctl_raw stop "$TARGET_UNIT" || failed=1
    systemctl_raw disable "$TARGET_UNIT" || failed=1
  else
    warn 'CRITICAL: neither promoted router nor predecessor can be verified ready'
    failed=1
  fi
  return "$failed"
}

abort_cutover() {
  local status=$1 reason=$2
  trap - ERR EXIT INT TERM
  (( status != 0 )) || status=1
  set +e
  if [[ $CUTOVER_ACTIVE == 1 ]]; then
    warn "router cutover aborted by $reason; starting availability-first recovery"
    recover || warn 'automatic router recovery was incomplete'
  fi
  exit "$status"
}

begin_cutover() {
  CUTOVER_ACTIVE=1
  trap 'abort_cutover "$?" ERR' ERR
  # EXIT also fires on successful fall-through; abort_cutover coerces status 0 to failure. Recovery
  # belongs on ERR/signals, while commit_cutover clears these traps after final verification.
  trap 'abort_cutover 130 INT' INT
  trap 'abort_cutover 143 TERM' TERM
}

commit_cutover() {
  CUTOVER_ACTIVE=0
  trap - ERR EXIT INT TERM
}

validate_timeout "$READINESS_TIMEOUT"
[[ -z $REQUESTED_TARGET_PORT ]] || validate_port "$REQUESTED_TARGET_PORT"
[[ $CADDY_CONFIG == /etc/caddy/Caddyfile ]] || die 'Caddy config path is fixed at /etc/caddy/Caddyfile'
[[ $ACTIVE_SNIPPET == /etc/caddy/router-active.caddy ]] \
  || die 'router active-backend state is fixed at /etc/caddy/router-active.caddy'
[[ $PROMOTE_HELPER == /usr/local/lib/apitoken-watchdog/controller/router-promote.sh ]] \
  || die 'router promotion helper path is fixed'
[[ $STABLE_READY_URL == http://127.0.0.1:8802/ready ]] \
  || die 'stable router readiness URL is fixed at 127.0.0.1:8802'
[[ $STABLE_STARTUP_URL == http://127.0.0.1:8802/startup ]] \
  || die 'stable router startup URL is fixed at 127.0.0.1:8802'
validate_service_unit "$LEGACY_UNIT"
validate_service_unit "$(slot_unit 8800)"
validate_service_unit "$(slot_unit 8801)"
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "$ENGINE_RELEASE_ROOT" /srv/claude-api engine)

log "preflighting router blue-green cutover (dry-run=$DRY_RUN target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
privileged_command test -f /etc/systemd/system/claude-router@.service \
  || die 'router slot template is not installed'
privileged_command test -x "$PROMOTE_HELPER" || die 'root-owned router promotion helper is missing'
privileged_command caddy validate --adapter caddyfile --config "$CADDY_CONFIG" >/dev/null \
  || die 'live Caddy configuration is invalid'
CURRENT_RELEASE=$(required_current_release_path "$ENGINE_RELEASE_ROOT")
validate_release_marker "$CURRENT_RELEASE" "$(basename -- "$CURRENT_RELEASE")"
[[ -x $CURRENT_RELEASE/claude-router ]] || die 'current router binary is missing'

if [[ $DRY_RUN == 1 ]]; then
  ACTIVE_PORT=${ROUTER_DRY_RUN_ACTIVE_PORT:-8798}
else
  ACTIVE_PORT=$(active_backend_port) || die 'router active-backend state is missing or malformed'
  ready_port "$ACTIVE_PORT" || die "active router backend $ACTIVE_PORT is not ready"
  stable_ready || die "stable router origin is not ready at $STABLE_READY_URL"
fi
case "$ACTIVE_PORT" in
  8798) ACTIVE_UNIT=$LEGACY_UNIT ;;
  8800|8801) ACTIVE_UNIT=$(slot_unit "$ACTIVE_PORT") ;;
  *) die "unsupported active router backend: $ACTIVE_PORT" ;;
esac

if [[ -n $REQUESTED_TARGET_PORT ]]; then
  TARGET_PORT=$REQUESTED_TARGET_PORT
  if [[ $TARGET_PORT == "$ACTIVE_PORT" ]]; then
    if slot_serves_release "$TARGET_PORT" "$CURRENT_RELEASE"; then
      TARGET_PORT=
    else
      die "requested port $TARGET_PORT is the active old slot; target $(other_port "$TARGET_PORT") to preserve availability"
    fi
  fi
else
  case "$ACTIVE_PORT" in
    8798) TARGET_PORT=8800 ;;
    8800|8801)
      if slot_serves_release "$ACTIVE_PORT" "$CURRENT_RELEASE"; then TARGET_PORT=; else TARGET_PORT=$(other_port "$ACTIVE_PORT"); fi
      ;;
  esac
fi

if [[ -z $TARGET_PORT ]]; then
  TARGET_PORT=$ACTIVE_PORT
  TARGET_UNIT=$ACTIVE_UNIT
  log "$TARGET_UNIT already serves current release; converging steady-state topology"
else
  validate_port "$TARGET_PORT"
  TARGET_UNIT=$(slot_unit "$TARGET_PORT")
  log "cutover decision: $ACTIVE_UNIT on $ACTIVE_PORT -> $TARGET_UNIT on $TARGET_PORT"
fi

begin_cutover
if [[ $TARGET_PORT != "$ACTIVE_PORT" ]]; then
  log "stopping inactive target $TARGET_UNIT before a fresh start"
  systemctl_command stop "$TARGET_UNIT"
  privileged_command "$HEADROOM_HELPER" "/run/claude-router-$TARGET_PORT" claude-router.slice \
    || die "insufficient memory or spool headroom for $TARGET_UNIT"
  log "starting $TARGET_UNIT from releases/current"
  systemctl_command start "$TARGET_UNIT"
  TARGET_STARTED=1
  wait_target || die "$TARGET_UNIT never became ready on the exact current release"
  if [[ -f $CURRENT_RELEASE/.large-payload-canary-v1 ]]; then
    [[ ! -L $CURRENT_RELEASE/.large-payload-canary-v1 ]] \
      && grep -Fxq large-payload-canary-v1 "$CURRENT_RELEASE/.large-payload-canary-v1" \
      || die 'large-payload canary marker is invalid'
    privileged_command "$PAYLOAD_GATE" "$(basename -- "$CURRENT_RELEASE")" \
      "http://127.0.0.1:$TARGET_PORT/v1/chat/completions" "$TARGET_UNIT" \
      "/run/claude-router-$TARGET_PORT" "$PAYLOAD_MEMORY_HIGH_BYTES" "$PAYLOAD_EVIDENCE_DIR" \
      /srv/claude-api/data/large-payload-canary.authorization \
      || die "$TARGET_UNIT failed exact-SHA large-payload candidate evidence"
  fi
  # Make the verified target boot-durable before Caddy can commit traffic to it. Until promotion,
  # the still-enabled predecessor remains the reboot anchor; after promotion, the target is.
  systemctl_command enable "$TARGET_UNIT"

  log "atomically promoting $TARGET_PORT for new Caddy requests"
  privileged_command "$PROMOTE_HELPER" "$TARGET_PORT"
  PROMOTED=1
  if [[ $DRY_RUN == 0 ]]; then
    [[ $(active_backend_port) == "$TARGET_PORT" ]] || die 'Caddy state did not record the promoted router slot'
    slot_serves_release "$TARGET_PORT" "$CURRENT_RELEASE" || die 'promoted router lost exact-release readiness'
    stable_ready || die 'stable router origin lost readiness after promotion'
    stable_startup || die 'stable router origin failed the provider data-path probe after promotion'
  fi
  # Caddy's graceful reload preserves established connections on the old configuration. Only after
  # new requests resolve exclusively to the target do we SIGTERM the old process and wait for its
  # bounded Axum drain, so a release never creates a 502 window or truncates a normal SSE stream.
  log "draining predecessor $ACTIVE_UNIT after Caddy cutover"
  systemctl_command disable "$ACTIVE_UNIT"
  stop_unit "$ACTIVE_UNIT"
else
  PROMOTED=1
  systemctl_command enable "$TARGET_UNIT"
fi

for port in 8800 8801; do
  unit=$(slot_unit "$port")
  [[ $port == "$TARGET_PORT" ]] && continue
  systemctl_command disable "$unit"
  stop_unit "$unit"
done
if [[ $TARGET_UNIT != "$LEGACY_UNIT" ]]; then
  systemctl_command disable "$LEGACY_UNIT"
  stop_unit "$LEGACY_UNIT"
fi

if [[ $DRY_RUN == 0 ]]; then
  slot_serves_release "$TARGET_PORT" "$CURRENT_RELEASE" \
    || die 'router target failed final exact-binary verification'
  stable_ready || die 'stable router origin failed final readiness verification'
  stable_startup || die 'stable router origin failed final provider data-path verification'
  [[ $(active_backend_port) == "$TARGET_PORT" ]] || die 'router active-backend state drifted after cutover'
  systemctl_raw is-enabled --quiet "$TARGET_UNIT" \
    || die "router target is not enabled after cutover: $TARGET_UNIT"
fi

commit_cutover
if [[ $DRY_RUN == 1 ]]; then
  log 'dry-run complete; no router, Caddy, or systemd state changed'
else
  log "router blue-green cutover complete; $TARGET_UNIT serves $(basename -- "$CURRENT_RELEASE")"
fi
# The controller contract is explicit: reaching this point means every final verification passed.
exit 0
