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
    'Health-gated provider cutover. Admit Anthropic, OpenAI and Gemini targets before draining' \
    'their old slots; fixed-provider rosters remain sealed and shared across each handoff.'
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
OPENAI_LEGACY_UNIT=claude-api-openai.service
GEMINI_LEGACY_UNIT=claude-api-gemini.service
PROVIDER_CAPABILITY_MARKER=.provider-runtime-v1
GEMINI_CAPABILITY_MARKER=.gemini-provider-v1
GEMINI_BLUEGREEN_MARKER=.gemini-bluegreen-v1
OPENAI_CAPABILITY_MARKER=.openai-bluegreen-v1
OPENAI_STABLE_READY_URL=${OPENAI_STABLE_READY_URL:-http://127.0.0.1:8792/ready}
ENGINE_MIGRATION_HELPER=/usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh
GEMINI_STABLE_READY_URL=${GEMINI_STABLE_READY_URL:-http://127.0.0.1:8794/ready}
CURRENT_RELEASE=
PREVIOUS_RELEASE=
ACTIVE_PORT=
ACTIVE_UNIT=
TARGET_PORT=
TARGET_UNIT=
TARGET_COMMITTED=0
OLD_SIGNALLED=0
CUTOVER_ACTIVE=0
CUTOVER_COMMITTED=0
OPENAI_ACTIVE_PORT=
OPENAI_ACTIVE_UNIT=
OPENAI_TARGET_PORT=
OPENAI_TARGET_UNIT=
OPENAI_TARGET_STARTED=0
OPENAI_TARGET_PREEXISTING=0
OPENAI_OLD_SIGNALLED=0
OPENAI_OLD_STOPPED=0
OPENAI_TRANSITION_STARTED=0
OPENAI_LEGACY_TARGET=0
OPENAI_SHARED_SUPPORTED=0
OPENAI_PREVIOUS_SHARED_SUPPORTED=0
OPENAI_COMMITTED=0
GEMINI_SUPPORTED=0
GEMINI_SHARED_SUPPORTED=0
GEMINI_PREVIOUS_SHARED_SUPPORTED=0
GEMINI_ACTIVE_PORT=
GEMINI_ACTIVE_UNIT=
GEMINI_TARGET_PORT=
GEMINI_TARGET_UNIT=
GEMINI_TARGET_STARTED=0
GEMINI_TARGET_PREEXISTING=0
GEMINI_OLD_SIGNALLED=0
GEMINI_OLD_STOPPED=0
GEMINI_LEGACY_TARGET=0
GEMINI_COMMITTED=0

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
slot_unit() { printf 'claude-api-anthropic@%s.service\n' "$1"; }
legacy_slot_unit() { printf 'claude-api@%s.service\n' "$1"; }
slot_url() { printf 'http://127.0.0.1:%s/ready\n' "$1"; }
openai_other_port() { [[ $1 == 8793 ]] && printf '8797\n' || printf '8793\n'; }
openai_slot_unit() { printf 'claude-api-openai@%s.service\n' "$1"; }
openai_slot_url() { printf 'http://127.0.0.1:%s/ready\n' "$1"; }
gemini_other_port() { [[ $1 == 8795 ]] && printf '8799\n' || printf '8795\n'; }
gemini_slot_unit() { printf 'claude-api-gemini@%s.service\n' "$1"; }
gemini_slot_url() { printf 'http://127.0.0.1:%s/ready\n' "$1"; }
unit_active() { systemctl_raw is-active --quiet "$1" >/dev/null 2>&1; }
unit_stopped() {
  local unit=$1 state pid control_group cgroup_file cgroup_pid=''
  state=$(systemctl_show_value "$unit" ActiveState) || return 1
  pid=$(systemctl_show_value "$unit" MainPID) || return 1
  control_group=$(systemctl_show_value "$unit" ControlGroup) || return 1
  [[ $state == inactive || $state == failed ]] || return 1
  [[ $pid == 0 ]] || return 1
  cgroup_file="/sys/fs/cgroup$control_group/cgroup.procs"
  if [[ -n $control_group && -e $cgroup_file ]]; then
    [[ -r $cgroup_file ]] || return 1
    IFS= read -r cgroup_pid <"$cgroup_file" || true
  fi
  [[ -z $cgroup_pid ]]
}
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
  template=$(legacy_slot_unit "$port")
  if unit_active "$template" && ready_port "$port"; then printf '%s\n' "$template"; return 0; fi
  if [[ $port == 8787 ]] && unit_active "$LEGACY_UNIT" && ready_port "$port"; then
    printf '%s\n' "$LEGACY_UNIT"; return 0
  fi
  return 1
}
slot_serves_current() {
  local port=$1 unit
  unit=$(slot_unit "$port")
  unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" anthropic \
    && ready_port "$port"
}
openai_ready_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$(openai_slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 200 ]]
}
openai_draining_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$(openai_slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 503 ]]
}
openai_slot_retired() {
  local unit=$1 port=$2 state
  systemctl_raw is-enabled --quiet "$unit" >/dev/null 2>&1 && return 1
  openai_ready_port "$port" && return 1
  state=$(systemctl_show_value "$unit" ActiveState) || return 1
  case "$state" in
    deactivating) return 0 ;;
    inactive|failed) unit_stopped "$unit" ;;
    *) return 1 ;;
  esac
}
wait_openai_retirement_ack() {
  local unit=$1 port=$2 attempt
  if [[ $DRY_RUN == 1 ]]; then return 0; fi
  # `systemctl --no-block stop` returns after queuing the job. Wait only for systemd to acknowledge
  # the state transition, never for the potentially ten-minute settlement drain itself.
  for attempt in $(seq 1 20); do
    openai_slot_retired "$unit" "$port" && return 0
    sleep 0.1
  done
  return 1
}
openai_slot_serves_current() {
  local port=$1 unit
  unit=$(openai_slot_unit "$port")
  unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" openai \
    && openai_ready_port "$port"
}
openai_slot_serves_release() {
  local port=$1 release=$2 unit pid executable
  unit=$(openai_slot_unit "$port")
  unit_active "$unit" || return 1
  pid=$(systemctl_show_value "$unit" MainPID) || return 1
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  executable=$(realpath -- "/proc/$pid/exe" 2>/dev/null) || return 1
  [[ $executable == "$release/claude-api" ]] || return 1
  process_environment_has "$pid" 'CLAUDE_API_PROVIDER=openai' || return 1
  openai_ready_port "$port"
}
openai_legacy_ready() {
  unit_active "$OPENAI_LEGACY_UNIT" && openai_ready_port 8793
}
openai_legacy_serves_current() {
  unit_release_binding_ok engine "$OPENAI_LEGACY_UNIT" "$ENGINE_RELEASE_ROOT" \
    "$CURRENT_RELEASE" openai && openai_ready_port 8793
}
openai_target_serves_current() {
  if [[ $OPENAI_LEGACY_TARGET == 1 ]]; then
    openai_legacy_serves_current
  else
    openai_slot_serves_current "$OPENAI_TARGET_PORT"
  fi
}
openai_unit_for_ready_port() {
  local port=$1 unit
  unit=$(openai_slot_unit "$port")
  if unit_active "$unit" && openai_ready_port "$port"; then printf '%s\n' "$unit"; return 0; fi
  if [[ $port == 8793 ]] && openai_legacy_ready; then
    printf '%s\n' "$OPENAI_LEGACY_UNIT"
    return 0
  fi
  return 1
}
stable_openai_ready() {
  curl --noproxy '*' --fail --silent --show-error --max-time 2 \
    "$OPENAI_STABLE_READY_URL" >/dev/null 2>&1
}
gemini_ready_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$(gemini_slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 200 ]]
}
gemini_draining_port() {
  local port=$1 status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$(gemini_slot_url "$port")" 2>/dev/null) || return 1
  [[ $status == 503 ]]
}
gemini_slot_retired() {
  local unit=$1 port=$2 state
  systemctl_raw is-enabled --quiet "$unit" >/dev/null 2>&1 && return 1
  gemini_ready_port "$port" && return 1
  state=$(systemctl_show_value "$unit" ActiveState) || return 1
  case "$state" in
    deactivating) return 0 ;;
    inactive|failed) unit_stopped "$unit" ;;
    *) return 1 ;;
  esac
}
wait_gemini_retirement_ack() {
  local unit=$1 port=$2 attempt
  if [[ $DRY_RUN == 1 ]]; then return 0; fi
  for attempt in $(seq 1 20); do
    gemini_slot_retired "$unit" "$port" && return 0
    sleep 0.1
  done
  return 1
}
gemini_slot_serves_current() {
  local port=$1 unit
  unit=$(gemini_slot_unit "$port")
  unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" gemini \
    && gemini_ready_port "$port"
}
gemini_slot_serves_release() {
  local port=$1 release=$2 unit pid executable
  unit=$(gemini_slot_unit "$port")
  unit_active "$unit" || return 1
  pid=$(systemctl_show_value "$unit" MainPID) || return 1
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  executable=$(realpath -- "/proc/$pid/exe" 2>/dev/null) || return 1
  [[ $executable == "$release/claude-api" ]] || return 1
  process_environment_has "$pid" 'CLAUDE_API_PROVIDER=gemini' || return 1
  gemini_ready_port "$port"
}
gemini_legacy_ready() {
  unit_active "$GEMINI_LEGACY_UNIT" && gemini_ready_port 8795
}
gemini_legacy_serves_current() {
  unit_release_binding_ok engine "$GEMINI_LEGACY_UNIT" "$ENGINE_RELEASE_ROOT" \
    "$CURRENT_RELEASE" gemini && gemini_ready_port 8795
}
gemini_target_serves_current() {
  if [[ $GEMINI_LEGACY_TARGET == 1 ]]; then
    gemini_legacy_serves_current
  else
    gemini_slot_serves_current "$GEMINI_TARGET_PORT"
  fi
}
gemini_unit_for_ready_port() {
  local port=$1 unit
  unit=$(gemini_slot_unit "$port")
  if unit_active "$unit" && gemini_ready_port "$port"; then printf '%s\n' "$unit"; return 0; fi
  if [[ $port == 8795 ]] && gemini_legacy_ready; then
    printf '%s\n' "$GEMINI_LEGACY_UNIT"
    return 0
  fi
  return 1
}
stable_gemini_ready() {
  curl --noproxy '*' --fail --silent --show-error --max-time 2 \
    "$GEMINI_STABLE_READY_URL" >/dev/null 2>&1
}
gemini_provider_envelope() {
  local status body_file
  body_file=$(mktemp)
  status=$(curl --noproxy '*' -sS -o "$body_file" -w '%{http_code}' --max-time 3 \
    -H 'content-type: application/json' -d '{}' \
    'http://127.0.0.1:8794/v1beta/models/gemini-provider-probe:generateContent' 2>/dev/null || true)
  # Fixed Gemini mode stays up when its provider-only kill switch is disabled, mirroring the OpenAI
  # singleton. Enabled mode rejects the unauthenticated probe at the client-key stage, faithfully
  # mirroring Google with a 400 API_KEY_INVALID (checked before any project/profile, so it holds for
  # an empty pre-onboarding roster too); disabled mode proves the same native router with its closed
  # 404. The watchdog's Prometheus-aware final verifier distinguishes the two.
  { [[ $status == 400 ]] && grep -Fq 'API_KEY_INVALID' "$body_file"; } \
    || { [[ $status == 404 ]] && grep -Fq 'NOT_FOUND' "$body_file"; }
  local result=$?
  rm -f -- "$body_file"
  return "$result"
}
post_admission_die() {
  printf '[deploy] ERROR: %s\n' "$*" >&2
  # Watchdog distinguishes this from an admission failure and rolls the selected cohort back.
  exit 2
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

wait_openai_target() {
  local deadline=$(( $(date +%s) + READINESS_TIMEOUT ))
  if [[ $DRY_RUN == 1 ]]; then
    log "dry-run: would require $OPENAI_TARGET_UNIT on current release and HTTP 200 at $(openai_slot_url "$OPENAI_TARGET_PORT")"
    return 0
  fi
  while (( $(date +%s) < deadline )); do
    if openai_target_serves_current; then return 0; fi
    sleep 1
  done
  journalctl -u "$OPENAI_TARGET_UNIT" -n 60 --no-pager >&2 || true
  return 1
}

wait_gemini_target() {
  local deadline=$(( $(date +%s) + READINESS_TIMEOUT ))
  if [[ $DRY_RUN == 1 ]]; then
    log "dry-run: would require $GEMINI_TARGET_UNIT on current release and HTTP 200 at $(gemini_slot_url "$GEMINI_TARGET_PORT")"
    return 0
  fi
  while (( $(date +%s) < deadline )); do
    if gemini_target_serves_current; then return 0; fi
    sleep 1
  done
  journalctl -u "$GEMINI_TARGET_UNIT" -n 60 --no-pager >&2 || true
  return 1
}

recover() {
  local failed=0 old_recovered=0
  [[ $DRY_RUN == 0 ]] || return 0
  if [[ $OPENAI_COMMITTED == 0 ]]; then
    if [[ $OPENAI_LEGACY_TARGET == 1 && $OPENAI_TARGET_STARTED == 1 ]]; then
      if [[ $OPENAI_OLD_SIGNALLED == 1 ]]; then
        warn 'recovery is completing the one-way legacy handoff after the shared slot was drained'
        if ! openai_legacy_serves_current; then
          systemctl_raw restart "$OPENAI_LEGACY_UNIT" || failed=1
          for _ in $(seq 1 "$READINESS_TIMEOUT"); do
            openai_legacy_serves_current && break
            sleep 1
          done
        fi
        if openai_legacy_serves_current; then
          if [[ -n $OPENAI_ACTIVE_UNIT ]]; then
            systemctl_raw stop "$OPENAI_ACTIVE_UNIT" || failed=1
            systemctl_raw disable "$OPENAI_ACTIVE_UNIT" || failed=1
          fi
          if [[ -z $OPENAI_ACTIVE_UNIT ]] || unit_stopped "$OPENAI_ACTIVE_UNIT"; then
            systemctl_raw enable "$OPENAI_LEGACY_UNIT" || failed=1
            systemctl_raw disable "$(openai_slot_unit 8793)" || failed=1
            systemctl_raw disable "$(openai_slot_unit 8797)" || failed=1
          else
            failed=1
            warn 'shared OpenAI slot did not stop; both disjoint generations were retained'
          fi
        else
          failed=1
          warn 'legacy OpenAI target did not recover; preserving the disjoint shared generation'
        fi
      else
        warn 'recovery is releasing the unadmitted legacy target back to the shared generation'
        systemctl_raw stop "$OPENAI_LEGACY_UNIT" || failed=1
      fi
    elif [[ $OPENAI_TARGET_STARTED == 1 && $OPENAI_OLD_STOPPED == 1 ]] \
        && openai_target_serves_current; then
      # The old owner is already gone, so rolling backward here would manufacture an outage. Make
      # the admitted target boot-durable and finish the daemon handoff; an interrupted first
      # transition remains retryable by the reconciler timer while its authenticated seed serves.
      warn "recovery commits verified OpenAI target $OPENAI_TARGET_UNIT after old-owner shutdown"
      systemctl_raw enable "$OPENAI_TARGET_UNIT" || failed=1
      if [[ -n $OPENAI_ACTIVE_UNIT && $OPENAI_ACTIVE_UNIT != "$OPENAI_TARGET_UNIT" ]]; then
        systemctl_raw disable "$OPENAI_ACTIVE_UNIT" || failed=1
      fi
      systemctl_raw disable "$OPENAI_LEGACY_UNIT" || failed=1
    else
      if [[ $OPENAI_TARGET_STARTED == 1 && -n $OPENAI_ACTIVE_UNIT \
          && $OPENAI_ACTIVE_UNIT != "$OPENAI_TARGET_UNIT" \
          && $OPENAI_OLD_SIGNALLED == 1 ]]; then
        warn "recovery restoring the previous OpenAI runtime before releasing the target"
        systemctl_raw restart "$OPENAI_ACTIVE_UNIT" || failed=1
        for _ in $(seq 1 "$READINESS_TIMEOUT"); do
          openai_ready_port "$OPENAI_ACTIVE_PORT" && break
          sleep 1
        done
        if openai_ready_port "$OPENAI_ACTIVE_PORT"; then
          old_recovered=1
        else
          failed=1
          warn "previous OpenAI runtime is not ready; retaining the target to preserve availability"
        fi
      else
        old_recovered=1
      fi
      if [[ $old_recovered == 1 ]]; then
        if [[ $OPENAI_TARGET_STARTED == 1 ]]; then
          if [[ $OPENAI_TARGET_PREEXISTING == 1 ]]; then
            warn "recovery retains the pre-existing current OpenAI target $OPENAI_TARGET_UNIT"
          else
            warn "recovery stopping the unverified OpenAI target $OPENAI_TARGET_UNIT"
            systemctl_raw stop "$OPENAI_TARGET_UNIT" || failed=1
            systemctl_raw disable "$OPENAI_TARGET_UNIT" || failed=1
          fi
        fi
      fi
    fi
  fi
  if [[ $GEMINI_COMMITTED == 0 ]]; then
    if [[ $GEMINI_LEGACY_TARGET == 1 && $GEMINI_TARGET_STARTED == 1 ]]; then
      if [[ $GEMINI_OLD_SIGNALLED == 1 ]]; then
        warn 'recovery is completing the one-way legacy Gemini handoff after the slot was drained'
        if ! gemini_legacy_serves_current; then
          systemctl_raw restart "$GEMINI_LEGACY_UNIT" || failed=1
          for _ in $(seq 1 "$READINESS_TIMEOUT"); do
            gemini_legacy_serves_current && break
            sleep 1
          done
        fi
        if gemini_legacy_serves_current; then
          if [[ -n $GEMINI_ACTIVE_UNIT ]]; then
            systemctl_raw stop "$GEMINI_ACTIVE_UNIT" || failed=1
            systemctl_raw disable "$GEMINI_ACTIVE_UNIT" || failed=1
          fi
          if [[ -z $GEMINI_ACTIVE_UNIT ]] || unit_stopped "$GEMINI_ACTIVE_UNIT"; then
            systemctl_raw enable "$GEMINI_LEGACY_UNIT" || failed=1
            systemctl_raw disable "$(gemini_slot_unit 8795)" || failed=1
            systemctl_raw disable "$(gemini_slot_unit 8799)" || failed=1
          else
            failed=1
            warn 'Gemini slot did not stop; both generations were retained'
          fi
        else
          failed=1
          warn 'legacy Gemini target did not recover; preserving the slot generation'
        fi
      else
        warn 'recovery is releasing the unadmitted legacy Gemini target back to the slot generation'
        systemctl_raw stop "$GEMINI_LEGACY_UNIT" || failed=1
      fi
    elif [[ $GEMINI_TARGET_STARTED == 1 && $GEMINI_OLD_STOPPED == 1 ]] \
        && gemini_target_serves_current; then
      warn "recovery commits verified Gemini target $GEMINI_TARGET_UNIT after old-slot drain"
      systemctl_raw enable "$GEMINI_TARGET_UNIT" || failed=1
      if [[ -n $GEMINI_ACTIVE_UNIT && $GEMINI_ACTIVE_UNIT != "$GEMINI_TARGET_UNIT" ]]; then
        systemctl_raw disable "$GEMINI_ACTIVE_UNIT" || failed=1
      fi
      systemctl_raw disable "$GEMINI_LEGACY_UNIT" || failed=1
    else
      local gemini_old_recovered=1
      if [[ $GEMINI_TARGET_STARTED == 1 && -n $GEMINI_ACTIVE_UNIT \
          && $GEMINI_ACTIVE_UNIT != "$GEMINI_TARGET_UNIT" \
          && $GEMINI_OLD_SIGNALLED == 1 ]]; then
        gemini_old_recovered=0
        warn "recovery restoring the previous Gemini runtime before releasing the target"
        systemctl_raw restart "$GEMINI_ACTIVE_UNIT" || failed=1
        for _ in $(seq 1 "$READINESS_TIMEOUT"); do
          gemini_ready_port "$GEMINI_ACTIVE_PORT" && break
          sleep 1
        done
        if gemini_ready_port "$GEMINI_ACTIVE_PORT"; then
          gemini_old_recovered=1
        else
          failed=1
          warn "previous Gemini runtime is not ready; retaining the target to preserve availability"
        fi
      fi
      if [[ $gemini_old_recovered == 1 && $GEMINI_TARGET_STARTED == 1 ]]; then
        if [[ $GEMINI_TARGET_PREEXISTING == 1 ]]; then
          warn "recovery retains the pre-existing current Gemini target $GEMINI_TARGET_UNIT"
        elif [[ $GEMINI_OLD_SIGNALLED == 1 \
            && $GEMINI_ACTIVE_UNIT == "$GEMINI_LEGACY_UNIT" ]] \
            && gemini_target_serves_current; then
          # The outer release rollback needs this alternate-port process after `current` moves back:
          # the restarted singleton still executes the failed candidate and cannot itself become the
          # old-release target without a bind gap. Keep the verified slot live but boot-disabled;
          # Caddy's ordered policy sends new work to the recovered singleton until rollback drains it.
          warn "recovery retains verified Gemini target $GEMINI_TARGET_UNIT as the release-rollback anchor"
          systemctl_raw disable "$GEMINI_TARGET_UNIT" || failed=1
        else
          warn "recovery stopping the unverified Gemini target $GEMINI_TARGET_UNIT"
          systemctl_raw stop "$GEMINI_TARGET_UNIT" || failed=1
          systemctl_raw disable "$GEMINI_TARGET_UNIT" || failed=1
        fi
      fi
    fi
  fi
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
validate_service_unit "$OPENAI_LEGACY_UNIT"
validate_service_unit "$(openai_slot_unit 8793)"
validate_service_unit "$(openai_slot_unit 8797)"
validate_service_unit "$GEMINI_LEGACY_UNIT"
validate_service_unit "$(gemini_slot_unit 8795)"
validate_service_unit "$(gemini_slot_unit 8799)"
validate_service_unit "$(slot_unit 8787)"
validate_service_unit "$(slot_unit 8788)"
validate_service_unit "$(legacy_slot_unit 8787)"
validate_service_unit "$(legacy_slot_unit 8788)"
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "$ENGINE_RELEASE_ROOT" /srv/claude-api engine)

log "preflighting PostgreSQL engine blue-green cutover (dry-run=$DRY_RUN target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
privileged_command test -s "$POSTGRES_ENV" || die "PostgreSQL authority is not active: $POSTGRES_ENV"
privileged_command test -x "$ENGINE_MIGRATION_HELPER" \
  || die "engine schema migration helper is missing or not executable"
privileged_command test -f /etc/systemd/system/claude-api-anthropic@.service \
  || die "fixed Anthropic slot template is not installed"
privileged_command test -f /etc/systemd/system/claude-api@.service \
  || die "combined bridge slot template is not installed"
privileged_command test -f "/etc/systemd/system/$OPENAI_LEGACY_UNIT" \
  || die "legacy OpenAI provider unit is not installed"
privileged_command test -f /etc/systemd/system/claude-api-openai@.service \
  || die "OpenAI slot template is not installed"
privileged_command test -f "/etc/systemd/system/$GEMINI_LEGACY_UNIT" \
  || die "legacy Gemini provider unit is not installed"
privileged_command test -f /etc/systemd/system/claude-api-gemini@.service \
  || die "Gemini slot template is not installed"
privileged_command caddy validate --adapter caddyfile --config "$CADDY_CONFIG" >/dev/null
privileged_command grep -q '127.0.0.1:8788' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the 8788 engine slot"
privileged_command grep -q '127.0.0.1:8790' "$CADDY_CONFIG" \
  || die "Caddy is missing the stable loopback Control API listener on 127.0.0.1:8790"
privileged_command grep -q '127.0.0.1:8792' "$CADDY_CONFIG" \
  || die "Caddy is missing the stable OpenAI listener on 127.0.0.1:8792"
privileged_command grep -q '127.0.0.1:8793' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the OpenAI slot on 127.0.0.1:8793"
privileged_command grep -q '127.0.0.1:8797' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the OpenAI slot on 127.0.0.1:8797"
if [[ $DRY_RUN == 0 ]]; then
  control_ready || die "stable Control API is not ready at $CONTROL_READY_URL"
fi
CURRENT_RELEASE=$(required_current_release_path "$ENGINE_RELEASE_ROOT")
validate_release_marker "$CURRENT_RELEASE" "$(basename -- "$CURRENT_RELEASE")"
[[ -x "$CURRENT_RELEASE/claude-api" ]] || die "current engine binary is missing"
[[ -f "$CURRENT_RELEASE/$PROVIDER_CAPABILITY_MARKER" \
    && ! -L "$CURRENT_RELEASE/$PROVIDER_CAPABILITY_MARKER" ]] \
  || die "current engine release lacks the fixed-provider rollback capability"
[[ $(<"$CURRENT_RELEASE/$PROVIDER_CAPABILITY_MARKER") == provider-runtime-v1 ]] \
  || die "current engine release has an invalid fixed-provider capability marker"
if [[ -e "$CURRENT_RELEASE/$OPENAI_CAPABILITY_MARKER" \
    || -L "$CURRENT_RELEASE/$OPENAI_CAPABILITY_MARKER" ]]; then
  [[ -f "$CURRENT_RELEASE/$OPENAI_CAPABILITY_MARKER" \
      && ! -L "$CURRENT_RELEASE/$OPENAI_CAPABILITY_MARKER" \
      && $(<"$CURRENT_RELEASE/$OPENAI_CAPABILITY_MARKER") == openai-bluegreen-v1 ]] \
    || die "current engine release has an invalid OpenAI blue-green capability marker"
  OPENAI_SHARED_SUPPORTED=1
fi
if [[ -f "$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER" \
    && ! -L "$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER" \
    && $(<"$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER") == gemini-provider-v1 ]]; then
  GEMINI_SUPPORTED=1
  if [[ -e "$CURRENT_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
      || -L "$CURRENT_RELEASE/$GEMINI_BLUEGREEN_MARKER" ]]; then
    [[ -f "$CURRENT_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
        && ! -L "$CURRENT_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
        && $(<"$CURRENT_RELEASE/$GEMINI_BLUEGREEN_MARKER") == gemini-bluegreen-v1 ]] \
      || die "current engine release has an invalid Gemini blue-green capability marker"
    GEMINI_SHARED_SUPPORTED=1
  fi
  privileged_command grep -q '127.0.0.1:8794' "$CADDY_CONFIG" \
    || die "Caddy is missing the stable Gemini listener on 127.0.0.1:8794"
  privileged_command grep -q '127.0.0.1:8795' "$CADDY_CONFIG" \
    || die "Caddy is not configured with the Gemini slot on 127.0.0.1:8795"
  privileged_command grep -q '127.0.0.1:8799' "$CADDY_CONFIG" \
    || die "Caddy is not configured with the Gemini slot on 127.0.0.1:8799"
fi
PREVIOUS_RELEASE=$(release_path_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/previous") \
  || die "previous engine release is required for provider cutover rollback"
validate_release_marker "$PREVIOUS_RELEASE" "$(basename -- "$PREVIOUS_RELEASE")"
[[ -f "$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER" \
    && ! -L "$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER" ]] \
  || die "previous engine release lacks the fixed-provider rollback capability"
[[ $(<"$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER") == provider-runtime-v1 ]] \
  || die "previous engine release has an invalid fixed-provider capability marker"
if [[ -e "$PREVIOUS_RELEASE/$OPENAI_CAPABILITY_MARKER" \
    || -L "$PREVIOUS_RELEASE/$OPENAI_CAPABILITY_MARKER" ]]; then
  [[ -f "$PREVIOUS_RELEASE/$OPENAI_CAPABILITY_MARKER" \
      && ! -L "$PREVIOUS_RELEASE/$OPENAI_CAPABILITY_MARKER" \
      && $(<"$PREVIOUS_RELEASE/$OPENAI_CAPABILITY_MARKER") == openai-bluegreen-v1 ]] \
    || die "previous engine release has an invalid OpenAI blue-green capability marker"
  OPENAI_PREVIOUS_SHARED_SUPPORTED=1
fi
if [[ -e "$PREVIOUS_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
    || -L "$PREVIOUS_RELEASE/$GEMINI_BLUEGREEN_MARKER" ]]; then
  [[ -f "$PREVIOUS_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
      && ! -L "$PREVIOUS_RELEASE/$GEMINI_BLUEGREEN_MARKER" \
      && $(<"$PREVIOUS_RELEASE/$GEMINI_BLUEGREEN_MARKER") == gemini-bluegreen-v1 ]] \
    || die "previous engine release has an invalid Gemini blue-green capability marker"
  GEMINI_PREVIOUS_SHARED_SUPPORTED=1
fi
if [[ $OPENAI_SHARED_SUPPORTED == 0 && $OPENAI_PREVIOUS_SHARED_SUPPORTED == 0 ]]; then
  die 'legacy OpenAI rollback requires a previous shared-daemon release as its availability anchor'
fi
if [[ $GEMINI_SUPPORTED == 1 && $GEMINI_SHARED_SUPPORTED == 0 \
    && $GEMINI_PREVIOUS_SHARED_SUPPORTED == 0 ]]; then
  die 'legacy Gemini rollback requires a previous blue-green release as its availability anchor'
fi

if [[ $DRY_RUN == 1 ]]; then
  log "dry-run: would apply pending engine PostgreSQL migrations for $(basename -- "$CURRENT_RELEASE")"
else
  privileged_command "$ENGINE_MIGRATION_HELPER" "$(basename -- "$CURRENT_RELEASE")" \
    || die "engine PostgreSQL migration failed; refusing to start the new engine slot"
fi

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
# An active-but-unready combined unit can hold the target port and, on the first split, unprotected
# Codex children. It is not an availability anchor, so stop its cgroup before binding the fixed slot.
BLOCKING_UNITS=("$(legacy_slot_unit "$TARGET_PORT")")
if [[ $TARGET_PORT == 8787 ]]; then BLOCKING_UNITS+=("$LEGACY_UNIT"); fi
for blocking_unit in "${BLOCKING_UNITS[@]}"; do
  if [[ $ACTIVE_UNIT != "$blocking_unit" ]] && ! unit_stopped "$blocking_unit"; then
    warn "stopping active but unready combined unit before starting $TARGET_UNIT: $blocking_unit"
    systemctl_command stop "$blocking_unit"
    systemctl_command disable "$blocking_unit"
  fi
done
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
  systemctl_command kill --kill-whom=main -s SIGUSR1 "$ACTIVE_UNIT"
  OLD_SIGNALLED=1
  run sleep "$PRE_DRAIN_SECONDS"
  if [[ $DRY_RUN == 0 ]]; then draining_port "$ACTIVE_PORT" || post_admission_die "old engine did not flip readiness to 503"; fi
  if [[ $DRY_RUN == 0 ]]; then
    slot_serves_current "$TARGET_PORT" || post_admission_die "target lost readiness during old-engine pre-drain"
    control_ready || post_admission_die "stable Control API lost readiness during old-engine pre-drain"
  fi
  systemctl_command stop "$ACTIVE_UNIT" || post_admission_die "could not stop old engine unit $ACTIVE_UNIT"
  systemctl_command disable "$ACTIVE_UNIT" || post_admission_die "could not disable old engine unit $ACTIVE_UNIT"
  if [[ $DRY_RUN == 0 ]] && ! unit_stopped "$ACTIVE_UNIT"; then
    post_admission_die "old engine cgroup remains active; refusing to overlap its Codex homes with OpenAI"
  fi
fi
systemctl_command enable "$TARGET_UNIT" || post_admission_die "could not enable target $TARGET_UNIT"

# Do not infer safety from readiness. A failed/restarting bridge or spare can still own a pre-lock
# Codex child, so every non-target engine cgroup must be inactive before OpenAI may acquire homes.
for old_unit in "$LEGACY_UNIT" "$(legacy_slot_unit 8787)" "$(legacy_slot_unit 8788)" \
  "$(slot_unit 8787)" "$(slot_unit 8788)"; do
  [[ $old_unit != "$TARGET_UNIT" ]] || continue
  if ! unit_stopped "$old_unit"; then
    warn "stopping non-target engine cgroup before OpenAI handoff: $old_unit"
    systemctl_command stop "$old_unit" \
      || post_admission_die "could not stop non-target engine cgroup $old_unit"
  fi
  systemctl_command disable "$old_unit" \
    || post_admission_die "could not disable non-target engine unit $old_unit"
  if [[ $DRY_RUN == 0 ]] && ! unit_stopped "$old_unit"; then
    post_admission_die "non-target engine cgroup remains active before OpenAI handoff: $old_unit"
  fi
done

# OpenAI mirrors the Anthropic two-slot cutover. The native provider shares one sealed roster
# between generations, so there is no ownership boundary to migrate: readiness alone gates
# admission, exactly like the Gemini fleet.
if [[ $OPENAI_SHARED_SUPPORTED == 0 ]]; then
  OPENAI_LEGACY_TARGET=1
  OPENAI_TARGET_PORT=8793
  OPENAI_TARGET_UNIT=$OPENAI_LEGACY_UNIT
  OPENAI_TARGET_STARTED=1
  if openai_slot_serves_release 8797 "$PREVIOUS_RELEASE"; then
    OPENAI_ACTIVE_PORT=8797
    OPENAI_ACTIVE_UNIT=$(openai_slot_unit 8797)
  elif ! openai_legacy_serves_current; then
    post_admission_die 'legacy OpenAI rollback has neither its previous shared anchor nor a ready target'
  fi
  if unit_active "$(openai_slot_unit 8793)"; then
    post_admission_die 'shared OpenAI slot 8793 blocks the rollback singleton port'
  fi
else
  if openai_legacy_ready; then
    OPENAI_ACTIVE_PORT=8793
    OPENAI_ACTIVE_UNIT=$OPENAI_LEGACY_UNIT
    OPENAI_TARGET_PORT=8797
  else
    if unit_active "$OPENAI_LEGACY_UNIT"; then
      warn "stopping unready legacy OpenAI runtime before daemon reconciliation"
      systemctl_command stop "$OPENAI_LEGACY_UNIT" \
        || post_admission_die "could not stop unready legacy OpenAI runtime"
    fi
    # The sealed roster is shared state, not a per-generation ownership domain. Candidate
    # readiness proves its profiles are authenticated before any traffic anchor is touched.
    OPENAI_READY_8793=0; OPENAI_READY_8797=0
    OPENAI_CURRENT_8793=0; OPENAI_CURRENT_8797=0
    openai_unit_for_ready_port 8793 >/dev/null && OPENAI_READY_8793=1
    openai_unit_for_ready_port 8797 >/dev/null && OPENAI_READY_8797=1
    openai_slot_serves_current 8793 && OPENAI_CURRENT_8793=1
    openai_slot_serves_current 8797 && OPENAI_CURRENT_8797=1
    case "$OPENAI_CURRENT_8793:$OPENAI_CURRENT_8797" in
      1:0)
        OPENAI_TARGET_PORT=8793
        if (( OPENAI_READY_8797 == 1 )); then
          OPENAI_ACTIVE_PORT=8797; OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8797)
        fi
        log 'OpenAI current release already serves on 8793; preserving it without another cutover'
        ;;
      0:1)
        OPENAI_TARGET_PORT=8797
        if (( OPENAI_READY_8793 == 1 )); then
          OPENAI_ACTIVE_PORT=8793; OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8793)
        fi
        log 'OpenAI current release already serves on 8797; preserving it without another cutover'
        ;;
      1:1)
        if systemctl_raw is-enabled --quiet "$(openai_slot_unit 8797)"; then
          OPENAI_TARGET_PORT=8797; OPENAI_ACTIVE_PORT=8793
          OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8793)
        else
          OPENAI_TARGET_PORT=8793; OPENAI_ACTIVE_PORT=8797
          OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8797)
        fi
        ;;
      0:0)
        case "$OPENAI_READY_8793:$OPENAI_READY_8797" in
          1:0) OPENAI_ACTIVE_PORT=8793; OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8793); OPENAI_TARGET_PORT=8797 ;;
          0:1) OPENAI_ACTIVE_PORT=8797; OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8797); OPENAI_TARGET_PORT=8793 ;;
          0:0) OPENAI_TARGET_PORT=8793 ;;
          1:1) OPENAI_TARGET_PORT=8797; OPENAI_ACTIVE_PORT=8793; OPENAI_ACTIVE_UNIT=$(openai_unit_for_ready_port 8793) ;;
        esac
        ;;
    esac
  fi
  OPENAI_TARGET_UNIT=$(openai_slot_unit "$OPENAI_TARGET_PORT")
fi
log "OpenAI cutover decision: ${OPENAI_ACTIVE_UNIT:-no ready old unit} -> $OPENAI_TARGET_UNIT"
if openai_target_serves_current; then OPENAI_TARGET_PREEXISTING=1; fi
OPENAI_TARGET_STARTED=1
if ! openai_target_serves_current; then
  systemctl_command stop "$OPENAI_TARGET_UNIT"
  systemctl_command start "$OPENAI_TARGET_UNIT" \
    || post_admission_die "could not start OpenAI target $OPENAI_TARGET_UNIT"
fi
wait_openai_target \
  || post_admission_die "$OPENAI_TARGET_UNIT did not become ready on current release"
log "waiting ${HEALTH_WINDOW_SECONDS}s for Caddy to health-include OpenAI slot $OPENAI_TARGET_PORT"
run sleep "$HEALTH_WINDOW_SECONDS"
if [[ $DRY_RUN == 0 ]]; then
  openai_target_serves_current \
    || post_admission_die "OpenAI target lost readiness during Caddy inclusion"
  stable_openai_ready \
    || post_admission_die "stable OpenAI origin lost readiness while admitting the target"
fi
if [[ $OPENAI_LEGACY_TARGET == 0 ]]; then
  # Both generations read the same sealed roster, so authenticated-home parity is inherent; the
  # candidate's own readiness probe is the admission proof. The old origin remains fully ready
  # while clients converge.
  if [[ $DRY_RUN == 0 ]]; then
    openai_target_serves_current \
      || post_admission_die "OpenAI target lost readiness during capacity parity admission"
    stable_openai_ready \
      || post_admission_die "stable OpenAI origin lost readiness during capacity parity admission"
  fi
fi
# Make the admitted candidate boot-durable before touching the old availability anchor. A failure
# here is still a pre-drain failure and recovery can discard the candidate without any topology gap.
systemctl_command enable "$OPENAI_TARGET_UNIT" \
  || post_admission_die "could not enable OpenAI target $OPENAI_TARGET_UNIT"
if [[ $DRY_RUN == 0 ]]; then
  systemctl_raw is-enabled --quiet "$OPENAI_TARGET_UNIT" \
    || post_admission_die "OpenAI target is not enabled before old-slot drain"
fi

if [[ -n $OPENAI_ACTIVE_UNIT && $OPENAI_ACTIVE_UNIT != "$OPENAI_TARGET_UNIT" ]]; then
  log "pre-draining $OPENAI_ACTIVE_UNIT with SIGUSR1"
  # A marker-less rollback cannot restart this shared slot through `current`: the old binary would
  # ignore shared-daemon mode. Once SIGUSR1 makes the handoff one-way, recovery finishes on the
  # already admitted singleton instead of resurrecting an unsafe mixed generation.
  if [[ $OPENAI_LEGACY_TARGET == 1 ]]; then OPENAI_OLD_SIGNALLED=1; fi
  systemctl_command kill --kill-whom=main -s SIGUSR1 "$OPENAI_ACTIVE_UNIT" \
    || post_admission_die "could not pre-drain $OPENAI_ACTIVE_UNIT"
  OPENAI_OLD_SIGNALLED=1
  run sleep "$PRE_DRAIN_SECONDS"
  if [[ $DRY_RUN == 0 ]]; then
    openai_draining_port "$OPENAI_ACTIVE_PORT" \
      || post_admission_die "$OPENAI_ACTIVE_UNIT did not flip readiness to 503"
    openai_target_serves_current \
      || post_admission_die "OpenAI target lost readiness during old-slot pre-drain"
    stable_openai_ready \
      || post_admission_die "stable OpenAI origin lost readiness during old-slot pre-drain"
  fi
  systemctl_command disable "$OPENAI_ACTIVE_UNIT" \
    || post_admission_die "could not disable old OpenAI unit $OPENAI_ACTIVE_UNIT"
  if [[ $OPENAI_LEGACY_TARGET == 1 ]]; then
    # Ownership is moving back into the singleton, so no shared gateway may retain a connection.
    systemctl_command stop "$OPENAI_ACTIVE_UNIT" \
      || post_admission_die "could not stop old OpenAI unit $OPENAI_ACTIVE_UNIT"
    if [[ $DRY_RUN == 0 ]] && ! unit_stopped "$OPENAI_ACTIVE_UNIT"; then
      post_admission_die "old OpenAI cgroup remains active before legacy ownership commit"
    fi
  else
    # Both generations use the same persistent daemon cohort. Once readiness is 503 and the new
    # gateway is admitted, the old HTTP process may finish detached usage/settlement in the
    # background. Waiting for systemd here put its 620-second drain directly on the deploy path.
    systemctl_command --no-block stop "$OPENAI_ACTIVE_UNIT" \
      || post_admission_die "could not queue retirement of old OpenAI unit $OPENAI_ACTIVE_UNIT"
    wait_openai_retirement_ack "$OPENAI_ACTIVE_UNIT" "$OPENAI_ACTIVE_PORT" \
      || post_admission_die "old OpenAI unit did not acknowledge background retirement"
  fi
  OPENAI_OLD_STOPPED=1
fi

if [[ $DRY_RUN == 0 ]]; then
  openai_target_serves_current \
    || post_admission_die "OpenAI target failed post-ownership verification"
  stable_openai_ready \
    || post_admission_die "stable OpenAI origin failed post-ownership verification"
fi
# The target is now release-attested, redundantly authenticated, boot-durable and the sole owner.
# A later cleanup failure must not recover by stopping this proven availability anchor.
OPENAI_COMMITTED=1
if [[ $OPENAI_LEGACY_TARGET == 1 ]]; then
  for OPENAI_OTHER_UNIT in "$(openai_slot_unit 8793)" "$(openai_slot_unit 8797)"; do
    systemctl_command stop "$OPENAI_OTHER_UNIT" \
      || post_admission_die "could not stop inactive OpenAI slot $OPENAI_OTHER_UNIT"
    systemctl_command disable "$OPENAI_OTHER_UNIT" \
      || post_admission_die "could not disable inactive OpenAI slot $OPENAI_OTHER_UNIT"
  done
else
  OPENAI_OTHER_PORT=$(openai_other_port "$OPENAI_TARGET_PORT")
  OPENAI_OTHER_UNIT=$(openai_slot_unit "$OPENAI_OTHER_PORT")
  if [[ $OPENAI_OTHER_UNIT != "$OPENAI_ACTIVE_UNIT" ]]; then
    systemctl_command stop "$OPENAI_OTHER_UNIT" \
      || post_admission_die "could not stop inactive OpenAI slot $OPENAI_OTHER_UNIT"
    systemctl_command disable "$OPENAI_OTHER_UNIT" \
      || post_admission_die "could not disable inactive OpenAI slot $OPENAI_OTHER_UNIT"
  fi
  systemctl_command disable "$OPENAI_LEGACY_UNIT" \
    || post_admission_die "could not disable legacy OpenAI singleton"
fi
if [[ $DRY_RUN == 0 ]]; then
  openai_target_serves_current \
    || post_admission_die "OpenAI target failed final exact-release verification"
  stable_openai_ready || post_admission_die "stable OpenAI origin failed final readiness verification"
fi

if [[ $GEMINI_SUPPORTED == 1 ]]; then
  if [[ $GEMINI_SHARED_SUPPORTED == 0 ]]; then
    GEMINI_LEGACY_TARGET=1
    GEMINI_TARGET_PORT=8795
    GEMINI_TARGET_UNIT=$GEMINI_LEGACY_UNIT
    GEMINI_TARGET_STARTED=1
    if gemini_slot_serves_release 8799 "$PREVIOUS_RELEASE"; then
      GEMINI_ACTIVE_PORT=8799
      GEMINI_ACTIVE_UNIT=$(gemini_slot_unit 8799)
    elif ! gemini_legacy_serves_current; then
      post_admission_die 'legacy Gemini rollback has neither its previous slot anchor nor a ready target'
    fi
    if unit_active "$(gemini_slot_unit 8795)"; then
      post_admission_die 'Gemini slot 8795 blocks the rollback singleton port'
    fi
  else
    if gemini_legacy_ready; then
      # First transition from the singleton: the alternate port is admitted before the legacy
      # process is touched, so installing the slot topology itself is zero-downtime.
      GEMINI_ACTIVE_PORT=8795
      GEMINI_ACTIVE_UNIT=$GEMINI_LEGACY_UNIT
      GEMINI_TARGET_PORT=8799
    else
      if unit_active "$GEMINI_LEGACY_UNIT"; then
        warn "stopping unready legacy Gemini runtime before slot reconciliation"
        systemctl_command stop "$GEMINI_LEGACY_UNIT" \
          || post_admission_die "could not stop unready legacy Gemini runtime"
      fi
      GEMINI_READY_8795=0; GEMINI_READY_8799=0
      GEMINI_CURRENT_8795=0; GEMINI_CURRENT_8799=0
      gemini_unit_for_ready_port 8795 >/dev/null && GEMINI_READY_8795=1
      gemini_unit_for_ready_port 8799 >/dev/null && GEMINI_READY_8799=1
      gemini_slot_serves_current 8795 && GEMINI_CURRENT_8795=1
      gemini_slot_serves_current 8799 && GEMINI_CURRENT_8799=1
      case "$GEMINI_CURRENT_8795:$GEMINI_CURRENT_8799" in
        1:0)
          GEMINI_TARGET_PORT=8795
          if (( GEMINI_READY_8799 == 1 )); then
            GEMINI_ACTIVE_PORT=8799; GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8799)
          fi
          log 'Gemini current release already serves on 8795; preserving it without another cutover'
          ;;
        0:1)
          GEMINI_TARGET_PORT=8799
          if (( GEMINI_READY_8795 == 1 )); then
            GEMINI_ACTIVE_PORT=8795; GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8795)
          fi
          log 'Gemini current release already serves on 8799; preserving it without another cutover'
          ;;
        1:1)
          if systemctl_raw is-enabled --quiet "$(gemini_slot_unit 8799)"; then
            GEMINI_TARGET_PORT=8799; GEMINI_ACTIVE_PORT=8795
            GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8795)
          else
            GEMINI_TARGET_PORT=8795; GEMINI_ACTIVE_PORT=8799
            GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8799)
          fi
          ;;
        0:0)
          case "$GEMINI_READY_8795:$GEMINI_READY_8799" in
            1:0) GEMINI_ACTIVE_PORT=8795; GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8795); GEMINI_TARGET_PORT=8799 ;;
            0:1) GEMINI_ACTIVE_PORT=8799; GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8799); GEMINI_TARGET_PORT=8795 ;;
            0:0) GEMINI_TARGET_PORT=8795 ;;
            1:1) GEMINI_TARGET_PORT=8799; GEMINI_ACTIVE_PORT=8795; GEMINI_ACTIVE_UNIT=$(gemini_unit_for_ready_port 8795) ;;
          esac
          ;;
      esac
    fi
    GEMINI_TARGET_UNIT=$(gemini_slot_unit "$GEMINI_TARGET_PORT")
  fi
  log "Gemini cutover decision: ${GEMINI_ACTIVE_UNIT:-no ready old unit} -> $GEMINI_TARGET_UNIT"
  if gemini_target_serves_current; then GEMINI_TARGET_PREEXISTING=1; fi
  GEMINI_TARGET_STARTED=1
  if ! gemini_target_serves_current; then
    systemctl_command stop "$GEMINI_TARGET_UNIT"
    systemctl_command start "$GEMINI_TARGET_UNIT" \
      || post_admission_die "could not start Gemini target $GEMINI_TARGET_UNIT"
  fi
  wait_gemini_target \
    || post_admission_die "$GEMINI_TARGET_UNIT did not become ready on current release"
  log "waiting ${HEALTH_WINDOW_SECONDS}s for Caddy to health-include Gemini slot $GEMINI_TARGET_PORT"
  run sleep "$HEALTH_WINDOW_SECONDS"
  if [[ $DRY_RUN == 0 ]]; then
    gemini_target_serves_current \
      || post_admission_die "Gemini target lost readiness during Caddy inclusion"
    stable_gemini_ready \
      || post_admission_die "stable Gemini origin lost readiness while admitting the target"
  fi
  systemctl_command enable "$GEMINI_TARGET_UNIT" \
    || post_admission_die "could not enable Gemini target $GEMINI_TARGET_UNIT"
  if [[ $DRY_RUN == 0 ]]; then
    systemctl_raw is-enabled --quiet "$GEMINI_TARGET_UNIT" \
      || post_admission_die "Gemini target is not enabled before old-slot drain"
  fi

  if [[ -n $GEMINI_ACTIVE_UNIT && $GEMINI_ACTIVE_UNIT != "$GEMINI_TARGET_UNIT" ]]; then
    log "pre-draining $GEMINI_ACTIVE_UNIT with SIGUSR1"
    systemctl_command kill --kill-whom=main -s SIGUSR1 "$GEMINI_ACTIVE_UNIT" \
      || post_admission_die "could not pre-drain $GEMINI_ACTIVE_UNIT"
    GEMINI_OLD_SIGNALLED=1
    run sleep "$PRE_DRAIN_SECONDS"
    if [[ $DRY_RUN == 0 ]]; then
      gemini_draining_port "$GEMINI_ACTIVE_PORT" \
        || post_admission_die "$GEMINI_ACTIVE_UNIT did not flip readiness to 503"
      gemini_target_serves_current \
        || post_admission_die "Gemini target lost readiness during old-slot pre-drain"
      stable_gemini_ready \
        || post_admission_die "stable Gemini origin lost readiness during old-slot pre-drain"
    fi
    systemctl_command disable "$GEMINI_ACTIVE_UNIT" \
      || post_admission_die "could not disable old Gemini unit $GEMINI_ACTIVE_UNIT"
    if [[ $GEMINI_LEGACY_TARGET == 1 ]]; then
      systemctl_command stop "$GEMINI_ACTIVE_UNIT" \
        || post_admission_die "could not stop old Gemini unit $GEMINI_ACTIVE_UNIT"
      if [[ $DRY_RUN == 0 ]] && ! unit_stopped "$GEMINI_ACTIVE_UNIT"; then
        post_admission_die "old Gemini cgroup remains active before legacy commit"
      fi
    else
      # Readiness fencing removes this generation from new routing; asynchronous stop lets its
      # established SSE requests finish under the normal bounded settlement deadline.
      systemctl_command --no-block stop "$GEMINI_ACTIVE_UNIT" \
        || post_admission_die "could not queue retirement of old Gemini unit $GEMINI_ACTIVE_UNIT"
      wait_gemini_retirement_ack "$GEMINI_ACTIVE_UNIT" "$GEMINI_ACTIVE_PORT" \
        || post_admission_die "old Gemini unit did not acknowledge background retirement"
    fi
    GEMINI_OLD_STOPPED=1
  fi

  if [[ $DRY_RUN == 0 ]]; then
    gemini_target_serves_current \
      || post_admission_die "Gemini target failed post-drain verification"
    stable_gemini_ready \
      || post_admission_die "stable Gemini origin failed post-drain verification"
    gemini_provider_envelope \
      || post_admission_die "stable Gemini origin does not expose the expected native provider envelope"
  fi
  GEMINI_COMMITTED=1
  if [[ $GEMINI_LEGACY_TARGET == 1 ]]; then
    for GEMINI_OTHER_UNIT in "$(gemini_slot_unit 8795)" "$(gemini_slot_unit 8799)"; do
      systemctl_command stop "$GEMINI_OTHER_UNIT" \
        || post_admission_die "could not stop inactive Gemini slot $GEMINI_OTHER_UNIT"
      systemctl_command disable "$GEMINI_OTHER_UNIT" \
        || post_admission_die "could not disable inactive Gemini slot $GEMINI_OTHER_UNIT"
    done
  else
    GEMINI_OTHER_PORT=$(gemini_other_port "$GEMINI_TARGET_PORT")
    GEMINI_OTHER_UNIT=$(gemini_slot_unit "$GEMINI_OTHER_PORT")
    if [[ $GEMINI_OTHER_UNIT != "$GEMINI_ACTIVE_UNIT" ]]; then
      systemctl_command stop "$GEMINI_OTHER_UNIT" \
        || post_admission_die "could not stop inactive Gemini slot $GEMINI_OTHER_UNIT"
      systemctl_command disable "$GEMINI_OTHER_UNIT" \
        || post_admission_die "could not disable inactive Gemini slot $GEMINI_OTHER_UNIT"
    fi
    systemctl_command disable "$GEMINI_LEGACY_UNIT" \
      || post_admission_die "could not disable legacy Gemini singleton"
  fi
else
  # Rollback to a release predating Gemini stops every provider incarnation.
  for unit in "$GEMINI_LEGACY_UNIT" "$(gemini_slot_unit 8795)" "$(gemini_slot_unit 8799)"; do
    if unit_active "$unit"; then
      systemctl_command stop "$unit" \
        || post_admission_die "could not stop unsupported Gemini runtime $unit during rollback"
    fi
    systemctl_command disable "$unit" \
      || post_admission_die "could not disable unsupported Gemini runtime $unit during rollback"
  done
fi

if [[ $DRY_RUN == 0 ]]; then
  OTHER_PORT=$(other_port "$TARGET_PORT")
  OTHER_UNIT=$(slot_unit "$OTHER_PORT")
  slot_serves_current "$TARGET_PORT" || post_admission_die "target failed final exact-release verification"
  control_ready || post_admission_die "stable Control API failed final readiness verification"
  ! unit_stopped "$OTHER_UNIT" && post_admission_die "inactive engine slot is not fully stopped after cutover: $OTHER_UNIT"
  ready_port "$OTHER_PORT" && post_admission_die "inactive engine slot remains ready after cutover: $OTHER_UNIT"
  # Repair boot-time drift as well as live drift. Disabling an already inactive unit is safe and
  # prevents an out-of-band enable from resurrecting a second writer after the next host reboot.
  systemctl_command disable "$OTHER_UNIT" || post_admission_die "could not disable $OTHER_UNIT"
  systemctl_raw is-enabled --quiet "$TARGET_UNIT" \
    || post_admission_die "target engine slot is not enabled after cutover: $TARGET_UNIT"
  openai_target_serves_current \
    || post_admission_die "OpenAI target failed final exact-release verification"
  stable_openai_ready || post_admission_die "stable OpenAI origin failed final readiness verification"
  systemctl_raw is-enabled --quiet "$OPENAI_TARGET_UNIT" \
    || post_admission_die "OpenAI target is not enabled: $OPENAI_TARGET_UNIT"
  if [[ $OPENAI_LEGACY_TARGET == 1 ]]; then
    for OPENAI_OTHER_UNIT in "$(openai_slot_unit 8793)" "$(openai_slot_unit 8797)"; do
      ! unit_stopped "$OPENAI_OTHER_UNIT" \
        && post_admission_die "shared OpenAI slot is not fully stopped: $OPENAI_OTHER_UNIT"
      if systemctl_raw is-enabled --quiet "$OPENAI_OTHER_UNIT"; then
        post_admission_die "shared OpenAI slot remains enabled: $OPENAI_OTHER_UNIT"
      fi
    done
  else
    openai_slot_retired "$OPENAI_OTHER_UNIT" "$OPENAI_OTHER_PORT" \
      || post_admission_die "inactive OpenAI slot is neither stopped nor retiring: $OPENAI_OTHER_UNIT"
    if systemctl_raw is-enabled --quiet "$OPENAI_OTHER_UNIT"; then
      post_admission_die "inactive OpenAI slot remains enabled: $OPENAI_OTHER_UNIT"
    fi
    ! unit_stopped "$OPENAI_LEGACY_UNIT" \
      && post_admission_die "legacy OpenAI singleton is not fully stopped"
    if systemctl_raw is-enabled --quiet "$OPENAI_LEGACY_UNIT"; then
      post_admission_die "legacy OpenAI singleton remains enabled"
    fi
  fi
  if [[ $GEMINI_SUPPORTED == 1 ]]; then
    gemini_target_serves_current \
      || post_admission_die "Gemini target failed final exact-release verification"
    stable_gemini_ready \
      || post_admission_die "stable Gemini origin failed final readiness verification"
    systemctl_raw is-enabled --quiet "$GEMINI_TARGET_UNIT" \
      || post_admission_die "Gemini target is not enabled: $GEMINI_TARGET_UNIT"
    if [[ $GEMINI_LEGACY_TARGET == 1 ]]; then
      for GEMINI_OTHER_UNIT in "$(gemini_slot_unit 8795)" "$(gemini_slot_unit 8799)"; do
        ! unit_stopped "$GEMINI_OTHER_UNIT" \
          && post_admission_die "Gemini slot is not fully stopped: $GEMINI_OTHER_UNIT"
        if systemctl_raw is-enabled --quiet "$GEMINI_OTHER_UNIT"; then
          post_admission_die "Gemini slot remains enabled: $GEMINI_OTHER_UNIT"
        fi
      done
    else
      gemini_slot_retired "$GEMINI_OTHER_UNIT" "$GEMINI_OTHER_PORT" \
        || post_admission_die "inactive Gemini slot is neither stopped nor retiring: $GEMINI_OTHER_UNIT"
      if systemctl_raw is-enabled --quiet "$GEMINI_OTHER_UNIT"; then
        post_admission_die "inactive Gemini slot remains enabled: $GEMINI_OTHER_UNIT"
      fi
      ! unit_stopped "$GEMINI_LEGACY_UNIT" \
        && post_admission_die "legacy Gemini singleton is not fully stopped"
      if systemctl_raw is-enabled --quiet "$GEMINI_LEGACY_UNIT"; then
        post_admission_die "legacy Gemini singleton remains enabled"
      fi
    fi
  else
    for GEMINI_OTHER_UNIT in "$GEMINI_LEGACY_UNIT" \
      "$(gemini_slot_unit 8795)" "$(gemini_slot_unit 8799)"; do
      if systemctl_raw is-enabled --quiet "$GEMINI_OTHER_UNIT"; then
        post_admission_die "unsupported Gemini provider remains enabled after rollback: $GEMINI_OTHER_UNIT"
      fi
    done
  fi
  if systemctl_raw is-enabled --quiet "$OTHER_UNIT"; then
    post_admission_die "inactive engine slot remains enabled after cutover: $OTHER_UNIT"
  fi
  ! unit_stopped "$LEGACY_UNIT" && post_admission_die "legacy engine cgroup is not fully stopped after cutover"
  if systemctl_raw is-enabled --quiet "$LEGACY_UNIT"; then
    systemctl_command disable "$LEGACY_UNIT" || post_admission_die "could not disable $LEGACY_UNIT"
  fi
  for combined_unit in "$(legacy_slot_unit 8787)" "$(legacy_slot_unit 8788)"; do
    ! unit_stopped "$combined_unit" \
      && post_admission_die "combined bridge cgroup is not fully stopped after cutover: $combined_unit"
    if systemctl_raw is-enabled --quiet "$combined_unit"; then
      systemctl_command disable "$combined_unit" \
        || post_admission_die "could not disable combined bridge unit $combined_unit"
    fi
  done
fi

commit_cutover
if [[ $DRY_RUN == 1 ]]; then
  log "dry-run complete; no provider or Caddy state changed"
else
  log "provider cutover complete; $TARGET_UNIT, $OPENAI_TARGET_UNIT and ${GEMINI_TARGET_UNIT:-Gemini-disabled} serve $(basename -- "$CURRENT_RELEASE")"
fi
