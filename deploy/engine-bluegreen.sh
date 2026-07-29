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
    'Health-gated provider cutover. Admit an Anthropic target, drain the old slot, then restart' \
    'the isolated OpenAI and supported Gemini runtimes only after the old cgroup is fully stopped.'
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
OPENAI_UNIT=claude-api-openai.service
GEMINI_UNIT=claude-api-gemini.service
PROVIDER_CAPABILITY_MARKER=.provider-runtime-v1
GEMINI_CAPABILITY_MARKER=.gemini-provider-v1
OPENAI_READY_URL=${OPENAI_READY_URL:-http://127.0.0.1:8793/ready}
OPENAI_STABLE_READY_URL=${OPENAI_STABLE_READY_URL:-http://127.0.0.1:8792/ready}
CODEX_HOME_MIGRATION_HELPER=/usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh
CODEX_LEGACY_HOME=/srv/claude-api/data/codex/home
CODEX_MIGRATED_HOME=/srv/claude-api/data/codex-homes/mikala1158qqq-gmail-com
GEMINI_READY_URL=${GEMINI_READY_URL:-http://127.0.0.1:8795/ready}
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
OPENAI_RESTART_ATTEMPTED=0
OPENAI_COMMITTED=0
GEMINI_SUPPORTED=0
GEMINI_RESTART_ATTEMPTED=0
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
openai_serves_current() {
  unit_release_binding_ok engine "$OPENAI_UNIT" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" openai \
    && curl --noproxy '*' --fail --silent --show-error --max-time 2 "$OPENAI_READY_URL" >/dev/null 2>&1
}
stable_openai_ready() {
  curl --noproxy '*' --fail --silent --show-error --max-time 2 \
    "$OPENAI_STABLE_READY_URL" >/dev/null 2>&1
}
openai_draining() {
  local status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$OPENAI_READY_URL" 2>/dev/null) || return 1
  [[ $status == 503 ]]
}
codex_legacy_home_migrated() {
  [[ ! -e $CODEX_LEGACY_HOME && ! -L $CODEX_LEGACY_HOME \
    && -d $CODEX_MIGRATED_HOME && ! -L $CODEX_MIGRATED_HOME ]]
}
gemini_serves_current() {
  unit_release_binding_ok engine "$GEMINI_UNIT" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" gemini \
    && curl --noproxy '*' --fail --silent --show-error --max-time 2 "$GEMINI_READY_URL" >/dev/null 2>&1
}
stable_gemini_ready() {
  curl --noproxy '*' --fail --silent --show-error --max-time 2 \
    "$GEMINI_STABLE_READY_URL" >/dev/null 2>&1
}
gemini_draining() {
  local status
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$GEMINI_READY_URL" 2>/dev/null) || return 1
  [[ $status == 503 ]]
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

recover() {
  local failed=0
  [[ $DRY_RUN == 0 ]] || return 0
  if [[ $OPENAI_RESTART_ATTEMPTED == 1 && $OPENAI_COMMITTED == 0 ]]; then
    warn "recovery stopping the unverified OpenAI runtime"
    systemctl_raw stop "$OPENAI_UNIT" || failed=1
  fi
  if [[ $GEMINI_RESTART_ATTEMPTED == 1 && $GEMINI_COMMITTED == 0 ]]; then
    warn "recovery stopping the unverified Gemini runtime"
    systemctl_raw stop "$GEMINI_UNIT" || failed=1
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
validate_service_unit "$OPENAI_UNIT"
validate_service_unit "$GEMINI_UNIT"
validate_service_unit "$(slot_unit 8787)"
validate_service_unit "$(slot_unit 8788)"
validate_service_unit "$(legacy_slot_unit 8787)"
validate_service_unit "$(legacy_slot_unit 8788)"
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "$ENGINE_RELEASE_ROOT" /srv/claude-api engine)

log "preflighting PostgreSQL engine blue-green cutover (dry-run=$DRY_RUN target=${REQUESTED_TARGET_PORT:-auto})"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
privileged_command test -s "$POSTGRES_ENV" || die "PostgreSQL authority is not active: $POSTGRES_ENV"
privileged_command test -f /etc/systemd/system/claude-api-anthropic@.service \
  || die "fixed Anthropic slot template is not installed"
privileged_command test -f /etc/systemd/system/claude-api@.service \
  || die "combined bridge slot template is not installed"
privileged_command test -f "/etc/systemd/system/$OPENAI_UNIT" || die "OpenAI provider unit is not installed"
[[ -x $CODEX_HOME_MIGRATION_HELPER && ! -L $CODEX_HOME_MIGRATION_HELPER ]] \
  || die "Codex home migration helper is missing or unsafe"
"$CODEX_HOME_MIGRATION_HELPER" --check \
  || die "legacy Codex home is not safe to migrate"
privileged_command caddy validate --adapter caddyfile --config "$CADDY_CONFIG" >/dev/null
privileged_command grep -q '127.0.0.1:8788' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the 8788 engine slot"
privileged_command grep -q '127.0.0.1:8790' "$CADDY_CONFIG" \
  || die "Caddy is missing the stable loopback Control API listener on 127.0.0.1:8790"
privileged_command grep -q '127.0.0.1:8792' "$CADDY_CONFIG" \
  || die "Caddy is missing the stable OpenAI listener on 127.0.0.1:8792"
privileged_command grep -q '127.0.0.1:8793' "$CADDY_CONFIG" \
  || die "Caddy is not configured with the OpenAI runtime on 127.0.0.1:8793"
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
if [[ -f "$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER" \
    && ! -L "$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER" \
    && $(<"$CURRENT_RELEASE/$GEMINI_CAPABILITY_MARKER") == gemini-provider-v1 ]]; then
  GEMINI_SUPPORTED=1
  privileged_command test -f "/etc/systemd/system/$GEMINI_UNIT" \
    || die "Gemini provider unit is not installed"
  privileged_command grep -q '127.0.0.1:8794' "$CADDY_CONFIG" \
    || die "Caddy is missing the stable Gemini listener on 127.0.0.1:8794"
  privileged_command grep -q '127.0.0.1:8795' "$CADDY_CONFIG" \
    || die "Caddy is not configured with the Gemini runtime on 127.0.0.1:8795"
fi
PREVIOUS_RELEASE=$(release_path_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/previous") \
  || die "previous engine release is required for provider cutover rollback"
validate_release_marker "$PREVIOUS_RELEASE" "$(basename -- "$PREVIOUS_RELEASE")"
[[ -f "$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER" \
    && ! -L "$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER" ]] \
  || die "previous engine release lacks the fixed-provider rollback capability"
[[ $(<"$PREVIOUS_RELEASE/$PROVIDER_CAPABILITY_MARKER") == provider-runtime-v1 ]] \
  || die "previous engine release has an invalid fixed-provider capability marker"

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

# The first provider split reaches this point with the old combined process fully gone. Future
# releases keep the old singleton serving while Anthropic rolls, then drain it sequentially here.
if ! openai_serves_current || ! codex_legacy_home_migrated; then
  OPENAI_RESTART_ATTEMPTED=1
  if unit_active "$OPENAI_UNIT"; then
    log "pre-draining $OPENAI_UNIT with SIGUSR1"
    systemctl_command kill --kill-whom=main -s SIGUSR1 "$OPENAI_UNIT" \
      || post_admission_die "could not pre-drain $OPENAI_UNIT"
    run sleep "$PRE_DRAIN_SECONDS"
    if [[ $DRY_RUN == 0 ]]; then
      openai_draining || post_admission_die "$OPENAI_UNIT did not flip readiness to 503"
    fi
  fi
  systemctl_command stop "$OPENAI_UNIT" \
    || post_admission_die "could not stop $OPENAI_UNIT for Codex home migration"
  if [[ $DRY_RUN == 0 ]] && ! unit_stopped "$OPENAI_UNIT"; then
    post_admission_die "$OPENAI_UNIT cgroup remains active; refusing to move its Codex home"
  fi
  privileged_command "$CODEX_HOME_MIGRATION_HELPER" --apply \
    || post_admission_die "could not migrate the legacy Codex home"
  systemctl_command restart "$OPENAI_UNIT" \
    || post_admission_die "could not restart $OPENAI_UNIT"
  wait_for_release_service "OpenAI provider" engine "$OPENAI_UNIT" "$ENGINE_RELEASE_ROOT" \
    "$CURRENT_RELEASE" "$OPENAI_READY_URL" "$READINESS_TIMEOUT" openai \
    || post_admission_die "$OPENAI_UNIT did not become ready on current release"
fi
log "waiting ${HEALTH_WINDOW_SECONDS}s for Caddy to health-include the OpenAI runtime"
run sleep "$HEALTH_WINDOW_SECONDS"
if [[ $DRY_RUN == 0 ]]; then
  openai_serves_current || post_admission_die "OpenAI runtime failed final exact-release verification"
  stable_openai_ready || post_admission_die "stable OpenAI origin failed readiness verification"
fi
systemctl_command enable "$OPENAI_UNIT" || post_admission_die "could not enable $OPENAI_UNIT"
OPENAI_COMMITTED=1

if [[ $GEMINI_SUPPORTED == 1 ]]; then
  if ! gemini_serves_current; then
    GEMINI_RESTART_ATTEMPTED=1
    if unit_active "$GEMINI_UNIT"; then
      log "pre-draining $GEMINI_UNIT with SIGUSR1"
      systemctl_command kill --kill-whom=main -s SIGUSR1 "$GEMINI_UNIT" \
        || post_admission_die "could not pre-drain $GEMINI_UNIT"
      run sleep "$PRE_DRAIN_SECONDS"
      if [[ $DRY_RUN == 0 ]]; then
        gemini_draining || post_admission_die "$GEMINI_UNIT did not flip readiness to 503"
      fi
    fi
    systemctl_command restart "$GEMINI_UNIT" \
      || post_admission_die "could not restart $GEMINI_UNIT"
    wait_for_release_service "Gemini provider" engine "$GEMINI_UNIT" "$ENGINE_RELEASE_ROOT" \
      "$CURRENT_RELEASE" "$GEMINI_READY_URL" "$READINESS_TIMEOUT" gemini \
      || post_admission_die "$GEMINI_UNIT did not become ready on current release"
  fi
  log "waiting ${HEALTH_WINDOW_SECONDS}s for Caddy to health-include the Gemini runtime"
  run sleep "$HEALTH_WINDOW_SECONDS"
  if [[ $DRY_RUN == 0 ]]; then
    gemini_serves_current || post_admission_die "Gemini runtime failed final exact-release verification"
    stable_gemini_ready || post_admission_die "stable Gemini origin failed readiness verification"
    gemini_provider_envelope \
      || post_admission_die "stable Gemini origin does not expose the expected native provider envelope"
  fi
  systemctl_command enable "$GEMINI_UNIT" || post_admission_die "could not enable $GEMINI_UNIT"
  GEMINI_COMMITTED=1
else
  # Rollback to a release predating Gemini must restore the two established providers instead of
  # trying to launch an unsupported provider mode from the old binary.
  if unit_active "$GEMINI_UNIT"; then
    systemctl_command stop "$GEMINI_UNIT" \
      || post_admission_die "could not stop unsupported Gemini runtime during rollback"
  fi
  systemctl_command disable "$GEMINI_UNIT" \
    || post_admission_die "could not disable unsupported Gemini runtime during rollback"
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
  systemctl_raw is-enabled --quiet "$OPENAI_UNIT" \
    || post_admission_die "OpenAI provider is not enabled after cutover: $OPENAI_UNIT"
  if [[ $GEMINI_SUPPORTED == 1 ]]; then
    systemctl_raw is-enabled --quiet "$GEMINI_UNIT" \
      || post_admission_die "Gemini provider is not enabled after cutover: $GEMINI_UNIT"
  elif systemctl_raw is-enabled --quiet "$GEMINI_UNIT"; then
    post_admission_die "unsupported Gemini provider remains enabled after rollback"
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
  log "provider cutover complete; $TARGET_UNIT, $OPENAI_UNIT and Gemini-supported=$GEMINI_SUPPORTED serve $(basename -- "$CURRENT_RELEASE")"
fi
