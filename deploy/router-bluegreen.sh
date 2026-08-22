#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"
IFS=, read -r ROUTER_PORT_A ROUTER_PORT_B <<<"$(contour_port_pair "$CONTOUR_PORTS_ROUTER_SLOTS")"
ROUTER_LEGACY_PORT=$CONTOUR_PORTS_ROUTER_LEGACY

usage() {
  printf '%s\n' \
    "Usage: router-bluegreen.sh [--target-port $ROUTER_PORT_A|$ROUTER_PORT_B] [--timeout SECONDS] [--dry-run]" \
    '' \
    'Start and exact-release verify an inactive router slot, atomically promote it in Caddy,' \
    'then gracefully drain and retire the previous slot or legacy singleton.'
}

DRY_RUN=0
REQUESTED_TARGET_PORT=
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
ENGINE_RELEASE_ROOT=${ENGINE_RELEASE_ROOT:-$CONTOUR_ROOTS_ENGINE_RELEASE}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-$CONTOUR_LOCKS_DEPLOY}
CADDY_CONFIG=${CADDY_CONFIG:-$CONTOUR_ROOTS_CADDY_CONFIG}
ACTIVE_SNIPPET=${ROUTER_ACTIVE_SNIPPET:-$CONTOUR_ROOTS_ROUTER_ACTIVE}
PROMOTE_HELPER=${ROUTER_PROMOTE_HELPER:-$CONTOUR_ROOTS_CONTROLLER/router-promote.sh}
STABLE_READY_URL=${ROUTER_STABLE_READY_URL:-$CONTOUR_ORIGINS_ROUTER_STABLE/ready}
STABLE_STARTUP_URL=${ROUTER_STABLE_STARTUP_URL:-$CONTOUR_ORIGINS_ROUTER_STABLE/startup}
LEGACY_UNIT=$CONTOUR_UNITS_ROUTER_LEGACY
ACTIVE_PORT=
ACTIVE_UNIT=
TARGET_PORT=
TARGET_UNIT=
TARGET_STARTED=0
HEADROOM_HELPER=${LARGE_PAYLOAD_HEADROOM_HELPER:-$CONTOUR_ROOTS_CONTROLLER/large-payload-headroom.sh}
PAYLOAD_GATE=${LARGE_PAYLOAD_CANDIDATE_GATE:-$CONTOUR_ROOTS_CONTROLLER/large-payload-candidate-gate.sh}
PAYLOAD_EVIDENCE_DIR=${LARGE_PAYLOAD_EVIDENCE_DIR:-$CONTOUR_ROOTS_STATE/large-payload}
PAYLOAD_MEMORY_HIGH_BYTES=${LARGE_PAYLOAD_ROUTER_MEMORY_HIGH_BYTES:-6442450944}
ROUTER_SUCCESS_PROOF=$CONTOUR_ROOTS_STATE/router-proof/success
PROMOTED=0
CUTOVER_ACTIVE=0
PROOF_CANDIDATE=

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

validate_port() { [[ $1 == "$ROUTER_PORT_A" || $1 == "$ROUTER_PORT_B" ]] || die "router slot port must be $ROUTER_PORT_A or $ROUTER_PORT_B: $1"; }
other_port() { [[ $1 == "$ROUTER_PORT_A" ]] && printf '%s\n' "$ROUTER_PORT_B" || printf '%s\n' "$ROUTER_PORT_A"; }
slot_unit() { printf '%s\n' "${CONTOUR_UNITS_ROUTER_TEMPLATE/@.service/@$1.service}"; }
slot_url() { printf 'http://%s:%s/ready\n' "$CONTOUR_NETWORK_LOOPBACK_HOST" "$1"; }
slot_startup_url() { printf 'http://%s:%s/startup\n' "$CONTOUR_NETWORK_LOOPBACK_HOST" "$1"; }
payload_canary_reason() {
  local sha=$1 file=$PAYLOAD_EVIDENCE_DIR/$sha.reason reason
  [[ $sha =~ ^[0-9a-f]{40}$ && -f $file && ! -L $file ]] || return 1
  reason=$(head -n 1 "$file" 2>/dev/null || true)
  reason=${reason%$'\r'}
  [[ $reason == payload-canary:* ]] || return 1
  printf '%s\n' "$reason"
}
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
  case "$ports" in "$ROUTER_LEGACY_PORT"|"$ROUTER_PORT_A"|"$ROUTER_PORT_B") printf '%s\n' "$ports" ;; *) return 1 ;; esac
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
  [[ -z $PROOF_CANDIDATE ]] || rm -f -- "$PROOF_CANDIDATE"
  if [[ $CUTOVER_ACTIVE == 1 ]]; then
    warn "router cutover aborted by $reason; starting availability-first recovery"
    recover || warn 'automatic router recovery was incomplete'
  fi
  exit "$status"
}

begin_cutover() {
  CUTOVER_ACTIVE=1
  trap 'abort_cutover "$?" ERR' ERR
  # Bash never runs the ERR trap for an explicit `exit`, so every post-mutation `... || die` path
  # would otherwise skip recovery entirely. Guard EXIT on a nonzero status: success falls through
  # untouched, while an explicit die/exit failure still gets availability-first recovery.
  trap 'rc=$?; (( rc == 0 )) || abort_cutover "$rc" EXIT' EXIT
  trap 'abort_cutover 130 INT' INT
  trap 'abort_cutover 143 TERM' TERM
}

commit_cutover() {
  CUTOVER_ACTIVE=0
  trap - ERR EXIT INT TERM
}

validate_timeout "$READINESS_TIMEOUT"
[[ -z $REQUESTED_TARGET_PORT ]] || validate_port "$REQUESTED_TARGET_PORT"
[[ $CADDY_CONFIG == "$CONTOUR_ROOTS_CADDY_CONFIG" ]] || die "Caddy config path is fixed by contour at $CONTOUR_ROOTS_CADDY_CONFIG"
[[ $ACTIVE_SNIPPET == "$CONTOUR_ROOTS_ROUTER_ACTIVE" ]] \
  || die "router active-backend state is fixed by contour at $CONTOUR_ROOTS_ROUTER_ACTIVE"
[[ $PROMOTE_HELPER == "$CONTOUR_ROOTS_CONTROLLER/router-promote.sh" ]] \
  || die 'router promotion helper path is fixed by contour'
[[ $STABLE_READY_URL == "$CONTOUR_ORIGINS_ROUTER_STABLE/ready" ]] \
  || die "stable router readiness URL is fixed by contour at $CONTOUR_ORIGINS_ROUTER_STABLE"
[[ $STABLE_STARTUP_URL == "$CONTOUR_ORIGINS_ROUTER_STABLE/startup" ]] \
  || die "stable router startup URL is fixed by contour at $CONTOUR_ORIGINS_ROUTER_STABLE"
[[ -d ${ROUTER_SUCCESS_PROOF%/*} && ! -L ${ROUTER_SUCCESS_PROOF%/*} ]] \
  || die 'router success proof directory is missing or unsafe'
[[ $(stat -c '%u:%g:%a' -- "${ROUTER_SUCCESS_PROOF%/*}" 2>/dev/null) \
    == "$(id -u):$(id -g):700" ]] \
  || die 'router success proof directory must be controller-owned mode 0700'
validate_service_unit "$LEGACY_UNIT"
validate_service_unit "$(slot_unit "$ROUTER_PORT_A")"
validate_service_unit "$(slot_unit "$ROUTER_PORT_B")"
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "$ENGINE_RELEASE_ROOT" "${CONTOUR_ROOTS_ENGINE_RELEASE%/releases}" engine)

log "preflighting router blue-green cutover (dry-run=$DRY_RUN target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
privileged_command test -f "$CONTOUR_ROOTS_SYSTEMD_UNITS/$CONTOUR_UNITS_ROUTER_TEMPLATE" \
  || die 'router slot template is not installed'
privileged_command test -x "$PROMOTE_HELPER" || die 'root-owned router promotion helper is missing'
privileged_command caddy validate --adapter caddyfile --config "$CADDY_CONFIG" >/dev/null \
  || die 'live Caddy configuration is invalid'
CURRENT_RELEASE=$(required_current_release_path "$ENGINE_RELEASE_ROOT")
validate_release_marker "$CURRENT_RELEASE" "$(basename -- "$CURRENT_RELEASE")"
[[ -x $CURRENT_RELEASE/claude-router ]] || die 'current router binary is missing'

if [[ $DRY_RUN == 1 ]]; then
  ACTIVE_PORT=${ROUTER_DRY_RUN_ACTIVE_PORT:-$ROUTER_LEGACY_PORT}
else
  ACTIVE_PORT=$(active_backend_port) || die 'router active-backend state is missing or malformed'
  ready_port "$ACTIVE_PORT" || die "active router backend $ACTIVE_PORT is not ready"
  stable_ready || die "stable router origin is not ready at $STABLE_READY_URL"
fi
case "$ACTIVE_PORT" in
  "$ROUTER_LEGACY_PORT") ACTIVE_UNIT=$LEGACY_UNIT ;;
  "$ROUTER_PORT_A"|"$ROUTER_PORT_B") ACTIVE_UNIT=$(slot_unit "$ACTIVE_PORT") ;;
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
    "$ROUTER_LEGACY_PORT") TARGET_PORT=$ROUTER_PORT_A ;;
    "$ROUTER_PORT_A"|"$ROUTER_PORT_B")
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
  privileged_command "$HEADROOM_HELPER" "$CONTOUR_ROOTS_SPOOL/router-$TARGET_PORT" \
    "$CONTOUR_UNITS_ROUTER_SLICE" \
    || die "insufficient memory or spool headroom for $TARGET_UNIT"
  log "starting $TARGET_UNIT from releases/current"
  systemctl_command start "$TARGET_UNIT"
  TARGET_STARTED=1
  wait_target || die "$TARGET_UNIT never became ready on the exact current release"
  if [[ -f $CURRENT_RELEASE/.large-payload-canary-v1 ]]; then
    [[ ! -L $CURRENT_RELEASE/.large-payload-canary-v1 ]] \
      && grep -Fxq large-payload-canary-v1 "$CURRENT_RELEASE/.large-payload-canary-v1" \
      || die 'large-payload canary marker is invalid'
    if ! privileged_command "$PAYLOAD_GATE" "$(basename -- "$CURRENT_RELEASE")" \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$TARGET_PORT/v1/chat/completions" "$TARGET_UNIT" \
      "$CONTOUR_ROOTS_SPOOL/router-$TARGET_PORT" "$PAYLOAD_MEMORY_HIGH_BYTES" "$PAYLOAD_EVIDENCE_DIR" \
      $CONTOUR_ROOTS_DATA/large-payload-canary.authorization; then
      die "$(payload_canary_reason "$(basename -- "$CURRENT_RELEASE")" \
        || printf 'payload-canary: failed exact-SHA candidate evidence')"
    fi
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

for port in "$ROUTER_PORT_A" "$ROUTER_PORT_B"; do
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

if [[ $DRY_RUN == 1 ]]; then
  commit_cutover
  log 'dry-run complete; no router, Caddy, proof, or systemd state changed'
else
  # Publish the exact-release proof before committing the controller traps. The watchdog clears
  # stale state before every invocation and accepts this proof only for the candidate it is
  # deploying. Keep both files in the fixed deploy-owned state directory so the atomic rename
  # cannot cross filesystems.
  umask 077
  PROOF_CANDIDATE=$(mktemp "${ROUTER_SUCCESS_PROOF}.tmp.XXXXXX")
  printf '%s\n' "$(basename -- "$CURRENT_RELEASE")" >"$PROOF_CANDIDATE"
  chmod 0600 "$PROOF_CANDIDATE"
  mv -fT -- "$PROOF_CANDIDATE" "$ROUTER_SUCCESS_PROOF"
  PROOF_CANDIDATE=
  commit_cutover
  log "router blue-green cutover complete; $TARGET_UNIT serves $(basename -- "$CURRENT_RELEASE")"
fi
exit 0
