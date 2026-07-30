#!/usr/bin/env bash
set -euo pipefail

# Supervise one official Unix-socket app-server per authenticated CODEX_HOME. HTTP gateway slots
# connect through the websocket-aware `codex app-server proxy`, so blue-green overlap never creates
# a second owner of auth.json. Root runs lifecycle commands; the template runs `serve` as deploy.

umask 077

CODEX_AS_HOMES_DIR=${CODEX_AS_HOMES_DIR:-/srv/claude-api/data/codex-homes}
CODEX_AS_CONFIG_ENV=${CODEX_AS_CONFIG_ENV:-/srv/claude-api/data/config.env}
CODEX_AS_RUNTIME_DIR=${CODEX_AS_RUNTIME_DIR:-/run/apitoken/codex-app-servers}
CODEX_AS_CONTROL_DIR=${CODEX_AS_CONTROL_DIR:-/run/apitoken/codex-app-server-control}
CODEX_AS_TRANSITION_FILE=$CODEX_AS_CONTROL_DIR/openai-bluegreen-transition
CODEX_AS_LOCK_FILE=$CODEX_AS_CONTROL_DIR/lifecycle.lock
CODEX_AS_UNIT_TEMPLATE=${CODEX_AS_UNIT_TEMPLATE:-claude-api-codex-app-server@}
CODEX_AS_LEGACY_UNIT=${CODEX_AS_LEGACY_UNIT:-claude-api-openai.service}
CODEX_AS_SYSTEMCTL=${CODEX_AS_SYSTEMCTL:-/usr/bin/systemctl}
CODEX_AS_PROC_ROOT=${CODEX_AS_PROC_ROOT:-/proc}
CODEX_AS_READY_TIMEOUT=${CODEX_AS_READY_TIMEOUT:-180}
# Legacy migration can wait one full 300s discovery interval plus one 600s admitted turn. Normal
# SIGUSR2-coordinated daemon rolls complete much sooner but share this conservative ceiling.
CODEX_AS_RETIRE_TIMEOUT=${CODEX_AS_RETIRE_TIMEOUT:-960}
CODEX_AS_SERVICE_USER=${CODEX_AS_SERVICE_USER:-deploy}
# Service availability is not a fleet-size policy. One authenticated home is useful capacity and
# must remain routable. Normal shared blue-green compares the exact old/candidate home sets. The
# historical singleton-to-daemon migration needs one *different* seed only because two processes
# may not own the same auth.json during that one-time handoff.
CODEX_AS_MIN_READY=1
CODEX_AS_MIGRATION_SEED_COUNT=1
# A cutover gate is deliberately much stricter than routine reconciliation. Both HTTP generations
# stay in Caddy while the candidate proves that its authenticated clients survive real health probes
# for a full minute; the production protocol failures that motivated this guard appeared after the
# old two-second check had already passed.
CODEX_AS_CUTOVER_STABILITY_SECONDS=${CODEX_AS_CUTOVER_STABILITY_SECONDS:-60}
CODEX_AS_LOCK_TIMEOUT=${CODEX_AS_LOCK_TIMEOUT:-1200}
CODEX_AS_LEGACY_EXPLICIT_HOME_NAME=${CODEX_AS_LEGACY_EXPLICIT_HOME_NAME:-mikala1158qqq-gmail-com}
CODEX_AS_PROXY_FILE=proxy.url
CODEX_AS_TRANSITION_PROXY_FILE=proxy.url.openai-bluegreen
CODEX_AS_TRANSITION_SENTINEL=disabled://openai-bluegreen-transition
CODEX_AS_DRAIN_FILE=.app-server-draining
CODEX_AS_DRAIN_SENTINEL=openai-bluegreen-drain-v1
CODEX_AS_SOCKET_SUFFIX=.sock
CODEX_AS_CLIENT_MARKER=openai-codex-client-v1
CODEX_AS_GATEWAY_UNITS=${CODEX_AS_GATEWAY_UNITS:-claude-api-openai@8793.service claude-api-openai@8797.service}
CODEX_AS_ROLLBACK_GATEWAY_UNIT=${CODEX_AS_ROLLBACK_GATEWAY_UNIT:-claude-api-openai@8797.service}
CODEX_AS_TRANSITION_TO_DAEMON=to-daemon
CODEX_AS_TRANSITION_TO_LEGACY=to-legacy
CODEX_AS_RECONCILE_CHANGED=0

codex_as_log() { printf '[codex-app-servers] %s\n' "$*"; }
codex_as_warn() { printf '[codex-app-servers] WARNING: %s\n' "$*" >&2; }
codex_as_fail() { printf '[codex-app-servers] ERROR: %s\n' "$*" >&2; return 1; }
codex_as_is_root() { [[ ${EUID:-$(id -u)} -eq 0 ]]; }
# Cutover stability is elapsed-time policy, not a number of health queries. `/proc/uptime` is
# monotonic on the Linux host and therefore cannot move backwards during an NTP correction. Bash's
# process-local elapsed clock keeps the helper testable on non-Linux developer machines.
codex_as_monotonic_seconds() {
  local uptime ignored
  if [[ -r /proc/uptime ]]; then
    read -r uptime ignored </proc/uptime || return 1
    [[ $uptime =~ ^[0-9]+([.][0-9]+)?$ ]] || return 1
    printf '%s\n' "${uptime%%.*}"
  else
    printf '%s\n' "$SECONDS"
  fi
}
codex_as_wait_tick() { sleep 1; }
codex_as_secure_control_dir() {
  mkdir -p -- "$CODEX_AS_CONTROL_DIR" \
    && chown root:root "$CODEX_AS_CONTROL_DIR" \
    && chmod 0750 "$CODEX_AS_CONTROL_DIR"
}

codex_as_acquire_lifecycle_lock() {
  codex_as_is_root \
    || { codex_as_fail 'lifecycle commands must run as root'; return 1; }
  [[ $CODEX_AS_LOCK_TIMEOUT =~ ^[1-9][0-9]*$ ]] || return 1
  codex_as_secure_control_dir || return 1
  exec 9>"$CODEX_AS_LOCK_FILE"
  chown root:root "$CODEX_AS_LOCK_FILE"
  chmod 0600 "$CODEX_AS_LOCK_FILE"
  flock -w "$CODEX_AS_LOCK_TIMEOUT" 9 \
    || { codex_as_fail 'timed out waiting for the app-server lifecycle lock'; return 1; }
}

codex_as_sha256_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$path" | awk '{print $1}'
  else
    shasum -a 256 -- "$path" | awk '{print $1}'
  fi
}

codex_as_sha256_text() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

codex_as_identity() {
  local path=$1
  if stat -c '%u %g %a %d %i' -- "$path" >/dev/null 2>&1; then
    stat -c '%u %g %a %d %i' -- "$path"
  else
    stat -f '%u %g %Lp %d %i' -- "$path"
  fi
}

codex_as_expected_owner() {
  local uid gid
  uid=$(id -u "$CODEX_AS_SERVICE_USER") || return 1
  gid=$(id -g "$CODEX_AS_SERVICE_USER") || return 1
  printf '%s %s\n' "$uid" "$gid"
}

codex_as_config_value() {
  local key=$1 line value count=0
  [[ -f $CODEX_AS_CONFIG_ENV && ! -L $CODEX_AS_CONFIG_ENV ]] || return 1
  while IFS= read -r line; do
    case "$line" in
      "$key="*)
        count=$((count + 1))
        value=${line#*=}
        ;;
    esac
  done <"$CODEX_AS_CONFIG_ENV"
  (( count == 1 )) || return 1
  printf '%s\n' "$value"
}

codex_as_load_desired() {
  CODEX_AS_BINARY=$(codex_as_config_value CLAUDE_API_CODEX_BIN) \
    || { codex_as_fail 'Codex binary path is absent or duplicated'; return 1; }
  CODEX_AS_BINARY_SHA256=$(codex_as_config_value CLAUDE_API_CODEX_BIN_SHA256) \
    || { codex_as_fail 'Codex binary digest is absent or duplicated'; return 1; }
  CODEX_AS_VERSION=$(codex_as_config_value CLAUDE_API_CODEX_VERSION) \
    || { codex_as_fail 'Codex version is absent or duplicated'; return 1; }
  [[ $CODEX_AS_BINARY == /* && -f $CODEX_AS_BINARY && ! -L $CODEX_AS_BINARY \
      && -x $CODEX_AS_BINARY ]] \
    || { codex_as_fail 'Codex binary is missing or unsafe'; return 1; }
  [[ $CODEX_AS_BINARY_SHA256 =~ ^[0-9a-f]{64}$ ]] \
    || { codex_as_fail 'Codex binary digest is malformed'; return 1; }
  [[ $CODEX_AS_VERSION =~ ^codex-cli\ [0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || { codex_as_fail 'Codex version is malformed'; return 1; }
  [[ $(codex_as_sha256_file "$CODEX_AS_BINARY") == "$CODEX_AS_BINARY_SHA256" ]] \
    || { codex_as_fail 'Codex binary digest does not match config'; return 1; }
}

codex_as_home_id() {
  local name=$1
  printf '%s' "$name" | codex_as_sha256_text | cut -c1-16
}

codex_as_socket() {
  local id=$1
  [[ $id =~ ^[0-9a-f]{16}$ ]] || return 1
  printf '%s/%s%s\n' "$CODEX_AS_RUNTIME_DIR" "$id" "$CODEX_AS_SOCKET_SUFFIX"
}

codex_as_validate_secret() {
  local path=$1 expected_uid=$2 expected_gid=$3 identity uid gid mode device inode
  [[ -f $path && ! -L $path ]] || return 1
  identity=$(codex_as_identity "$path") || return 1
  read -r uid gid mode device inode <<<"$identity"
  [[ $uid == "$expected_uid" && $gid == "$expected_gid" && $mode == 600 ]]
}

codex_as_validate_home() {
  local home=$1 expected_uid=$2 expected_gid=$3 identity uid gid mode device inode
  [[ -d $home && ! -L $home ]] || return 1
  identity=$(codex_as_identity "$home") || return 1
  read -r uid gid mode device inode <<<"$identity"
  [[ $uid == "$expected_uid" && $gid == "$expected_gid" && $mode == 700 ]] || return 1
  codex_as_validate_secret "$home/auth.json" "$expected_uid" "$expected_gid"
}

# Emit only opaque id + safe basename. Paths and account-shaped names never reach logs.
codex_as_home_records() {
  local owner expected_uid expected_gid entry name id seen=' '
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  [[ -d $CODEX_AS_HOMES_DIR && ! -L $CODEX_AS_HOMES_DIR ]] || return 1
  for entry in "$CODEX_AS_HOMES_DIR"/*; do
    [[ -e $entry || -L $entry ]] || continue
    name=${entry##*/}
    [[ $name =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || continue
    codex_as_validate_home "$entry" "$expected_uid" "$expected_gid" || continue
    id=$(codex_as_home_id "$name")
    [[ $seen != *" $id "* ]] || { codex_as_fail 'opaque Codex home id collision'; return 1; }
    seen+="$id "
    printf '%s\t%s\n' "$id" "$name"
  done
}

codex_as_find_home() {
  local wanted=$1 id name found=''
  [[ $wanted =~ ^[0-9a-f]{16}$ ]] || return 1
  while IFS=$'\t' read -r id name; do
    [[ $id == "$wanted" ]] || continue
    [[ -z $found ]] || return 1
    found=$name
  done < <(codex_as_home_records)
  [[ -n $found ]] || return 1
  printf '%s\n' "$CODEX_AS_HOMES_DIR/$found"
}

codex_as_proxy_value() {
  local home=$1 marker saved
  local owner expected_uid expected_gid value
  marker=$home/$CODEX_AS_PROXY_FILE
  saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  if [[ -e $saved || -L $saved ]]; then
    codex_as_validate_secret "$saved" "$expected_uid" "$expected_gid" || return 1
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
    [[ $(<"$marker") == "$CODEX_AS_TRANSITION_SENTINEL" ]] || return 1
    value=$(<"$saved")
  elif [[ -e $marker || -L $marker ]]; then
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
    value=$(<"$marker")
    if [[ $value == "$CODEX_AS_TRANSITION_SENTINEL" ]]; then
      printf '\n'
      return 0
    fi
  else
    printf '\n'
    return 0
  fi
  [[ $value != *$'\r'* && $value != *$'\n'* ]] || return 1
  [[ -z $value || $value == http://* || $value == https://* \
      || $value == socks5://* || $value == socks5h://* ]] || return 1
  printf '%s\n' "$value"
}

codex_as_proxy_digest() {
  local value=$1
  printf 'proxy-v1:%s' "$value" | codex_as_sha256_text
}

codex_as_state_value() {
  local file=$1 key=$2 line value count=0
  [[ -f $file && ! -L $file ]] || return 1
  while IFS= read -r line; do
    case "$line" in
      "$key="*) count=$((count + 1)); value=${line#*=} ;;
    esac
  done <"$file"
  (( count == 1 )) || return 1
  printf '%s\n' "$value"
}

codex_as_write_state() {
  local id=$1 home=$2 proxy_digest=$3 state temporary identity uid gid mode device inode
  state=$CODEX_AS_RUNTIME_DIR/$id.state
  temporary=$state.tmp.$$
  identity=$(codex_as_identity "$home") || return 1
  read -r uid gid mode device inode <<<"$identity"
  {
    printf 'binary_sha256=%s\n' "$CODEX_AS_BINARY_SHA256"
    printf 'version=%s\n' "$CODEX_AS_VERSION"
    printf 'proxy_sha256=%s\n' "$proxy_digest"
    printf 'home_identity=%s:%s\n' "$device" "$inode"
  } >"$temporary"
  chmod 0600 "$temporary"
  mv -f -- "$temporary" "$state"
}

codex_as_serve() {
  local id=${1:-} home actual_version proxy proxy_digest socket
  local -a clean_env overrides
  [[ $id =~ ^[0-9a-f]{16}$ ]] || { codex_as_fail 'invalid app-server instance id'; return 1; }
  codex_as_load_desired || return 1
  home=$(codex_as_find_home "$id") \
    || { codex_as_fail 'app-server instance has no authenticated home'; return 1; }
  actual_version=$(
    env -i CODEX_HOME="$home" PATH=/usr/local/bin:/usr/bin:/bin \
      "$CODEX_AS_BINARY" --version
  ) || { codex_as_fail 'Codex version probe failed'; return 1; }
  [[ $actual_version == "$CODEX_AS_VERSION" ]] \
    || { codex_as_fail 'Codex version does not match config'; return 1; }
  proxy=$(codex_as_proxy_value "$home") \
    || { codex_as_fail 'home proxy metadata is unsafe'; return 1; }
  proxy_digest=$(codex_as_proxy_digest "$proxy")
  [[ -d $CODEX_AS_RUNTIME_DIR && ! -L $CODEX_AS_RUNTIME_DIR ]] \
    || { codex_as_fail 'app-server runtime directory is unavailable'; return 1; }
  socket=$(codex_as_socket "$id") || return 1
  # Do not unlink an existing socket here. The official app-server probes it, refuses a live
  # listener, rejects non-socket/symlink paths and removes only a genuinely stale Unix socket.
  # Pre-unlinking could detach a live listener and let two home owners overlap invisibly.
  codex_as_write_state "$id" "$home" "$proxy_digest"

  clean_env=(/usr/bin/env -i "CODEX_HOME=$home" PATH=/usr/local/bin:/usr/bin:/bin NO_COLOR=1 TERM=dumb)
  if [[ -n $proxy ]]; then
    clean_env+=("HTTP_PROXY=$proxy" "HTTPS_PROXY=$proxy" "ALL_PROXY=$proxy")
    clean_env+=("http_proxy=$proxy" "https_proxy=$proxy" "all_proxy=$proxy")
    if [[ -n ${NO_PROXY:-} ]]; then clean_env+=("NO_PROXY=$NO_PROXY" "no_proxy=$NO_PROXY"); fi
  else
    local name
    for name in HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; do
      [[ -n ${!name:-} ]] && clean_env+=("$name=${!name}")
    done
  fi
  overrides=(
    include_permissions_instructions=false
    include_apps_instructions=false
    include_collaboration_mode_instructions=false
    include_environment_context=false
    skills.include_instructions=false
    features.plugins=false
    features.apps=false
    features.multi_agent_v2=false
    project_doc_max_bytes=0
    'mcp_servers={}'
  )
  local -a args=()
  local override
  for override in "${overrides[@]}"; do args+=(--config "$override"); done
  args+=(app-server --listen "unix://$socket")
  exec "${clean_env[@]}" "$CODEX_AS_BINARY" "${args[@]}"
}

codex_as_unit() { printf '%s%s.service\n' "$CODEX_AS_UNIT_TEMPLATE" "$1"; }

codex_as_unit_active() {
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$(codex_as_unit "$1")" >/dev/null 2>&1
}

codex_as_unit_file_state_reconciler_owned() {
  case "$1" in
    disabled|indirect|static) return 0 ;;
    *) return 1 ;;
  esac
}

# Template instances are normally `static`: systemd can start them explicitly, but they have no
# install target and cannot resurrect at boot on their own. `systemctl is-enabled` exits zero for
# static units, so its status cannot distinguish that safe state from an actually enabled instance.
codex_as_unit_reconciler_owned() {
  local id=$1 state
  state=$("$CODEX_AS_SYSTEMCTL" show -p UnitFileState --value "$(codex_as_unit "$id")" 2>/dev/null) \
    || return 1
  codex_as_unit_file_state_reconciler_owned "$state"
}

codex_as_home_draining() {
  local home=$1 marker=$1/$CODEX_AS_DRAIN_FILE owner expected_uid expected_gid
  [[ -e $marker || -L $marker ]] || return 1
  owner=$(codex_as_expected_owner) || return 0
  read -r expected_uid expected_gid <<<"$owner"
  codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 0
  [[ $(<"$marker") == "$CODEX_AS_DRAIN_SENTINEL" ]]
}

# A serving unit is an attested daemon that can keep traffic alive, even when it still runs the
# previous desired Codex pin. A converged/healthy unit additionally matches the new pin and proxy
# state. Keeping those two predicates separate is what makes a rolling pin update possible.
codex_as_unit_serving() {
  local id=$1 home unit pid executable state recorded_sha identity uid gid mode device inode socket
  home=$(codex_as_find_home "$id") || return 1
  unit=$(codex_as_unit "$id")
  codex_as_unit_active "$id" || return 1
  pid=$("$CODEX_AS_SYSTEMCTL" show -p MainPID --value "$unit" 2>/dev/null) || return 1
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  executable=$(readlink -f -- "$CODEX_AS_PROC_ROOT/$pid/exe" 2>/dev/null) || return 1
  [[ $executable == /* && -f $executable && -x $executable ]] || return 1
  socket=$(codex_as_socket "$id") || return 1
  [[ -S $socket && ! -L $socket ]] || return 1
  state=$CODEX_AS_RUNTIME_DIR/$id.state
  recorded_sha=$(codex_as_state_value "$state" binary_sha256) || return 1
  [[ $recorded_sha =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ $(codex_as_sha256_file "$executable") == "$recorded_sha" ]] || return 1
  [[ $(codex_as_state_value "$state" version) =~ ^codex-cli\ [0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || return 1
  [[ $(codex_as_state_value "$state" proxy_sha256) =~ ^[0-9a-f]{64}$ ]] || return 1
  identity=$(codex_as_identity "$home") || return 1
  read -r uid gid mode device inode <<<"$identity"
  [[ $(codex_as_state_value "$state" home_identity) == "$device:$inode" ]]
}

codex_as_unit_healthy() {
  local id=$1 home unit pid executable state proxy proxy_digest
  codex_as_unit_serving "$id" || return 1
  home=$(codex_as_find_home "$id") || return 1
  unit=$(codex_as_unit "$id")
  pid=$("$CODEX_AS_SYSTEMCTL" show -p MainPID --value "$unit" 2>/dev/null) || return 1
  executable=$(readlink -f -- "$CODEX_AS_PROC_ROOT/$pid/exe" 2>/dev/null) || return 1
  [[ $executable == "$CODEX_AS_BINARY" ]] || return 1
  state=$CODEX_AS_RUNTIME_DIR/$id.state
  [[ $(codex_as_state_value "$state" binary_sha256) == "$CODEX_AS_BINARY_SHA256" ]] || return 1
  [[ $(codex_as_state_value "$state" version) == "$CODEX_AS_VERSION" ]] || return 1
  proxy=$(codex_as_proxy_value "$home") || return 1
  proxy_digest=$(codex_as_proxy_digest "$proxy")
  [[ $(codex_as_state_value "$state" proxy_sha256) == "$proxy_digest" ]] || return 1
}

codex_as_wait_healthy() {
  local id=$1 deadline=$(( $(date +%s) + CODEX_AS_READY_TIMEOUT ))
  while (( $(date +%s) < deadline )); do
    codex_as_unit_healthy "$id" && return 0
    sleep 1
  done
  return 1
}

codex_as_other_serving_count() {
  local excluded=$1 id name home clients gateways count=0
  gateways=$(codex_as_gateway_active_count) || return 1
  while IFS=$'\t' read -r id name; do
    [[ $id == "$excluded" ]] && continue
    home=$CODEX_AS_HOMES_DIR/$name
    codex_as_home_draining "$home" && continue
    codex_as_unit_serving "$id" || continue
    if (( gateways > 0 )); then
      clients=$(codex_as_home_ready_client_count "$home") || return 1
      (( clients >= 1 )) || continue
    fi
    count=$((count + 1))
  done < <(codex_as_home_records)
  printf '%s\n' "$count"
}

codex_as_signal_gateways() {
  local unit
  for unit in $CODEX_AS_GATEWAY_UNITS; do
    [[ $unit =~ ^claude-api-openai@[0-9]+\.service$ ]] || return 1
    "$CODEX_AS_SYSTEMCTL" is-active --quiet "$unit" >/dev/null 2>&1 || continue
    # `systemctl kill` defaults to the whole cgroup. The gateway's disposable `codex app-server
    # proxy` children do not handle SIGUSR2; signalling the cgroup therefore tears down every
    # authenticated websocket at once. Only the Rust main process owns the reconcile handler.
    if ! "$CODEX_AS_SYSTEMCTL" kill --kill-whom=main --signal=SIGUSR2 "$unit"; then
      "$CODEX_AS_SYSTEMCTL" is-active --quiet "$unit" >/dev/null 2>&1 || continue
      return 1
    fi
  done
}

codex_as_home_proxy_count() {
  local home=$1 environ entry process_dir
  local count=0
  local -a args
  for environ in "$CODEX_AS_PROC_ROOT"/[0-9]*/environ; do
    [[ -r $environ ]] || continue
    local owns_home=0
    while IFS= read -r -d '' entry; do
      if [[ $entry == "CODEX_HOME=$home" ]]; then
        owns_home=1
        break
      fi
    done <"$environ" 2>/dev/null || true
    (( owns_home == 1 )) || continue
    process_dir=${environ%/environ}
    [[ -r $process_dir/cmdline ]] || continue
    args=()
    while IFS= read -r -d '' entry; do args+=("$entry"); done <"$process_dir/cmdline" 2>/dev/null || true
    local index
    for (( index = 0; index + 1 < ${#args[@]}; index++ )); do
      if [[ ${args[index]} == app-server && ${args[index + 1]} == proxy ]]; then
        count=$((count + 1))
        break
      fi
    done
  done
  printf '%s\n' "$count"
}

codex_as_gateway_active_count() {
  local unit count=0
  for unit in $CODEX_AS_GATEWAY_UNITS; do
    [[ $unit =~ ^claude-api-openai@[0-9]+\.service$ ]] || return 1
    "$CODEX_AS_SYSTEMCTL" is-active --quiet "$unit" >/dev/null 2>&1 \
      && count=$((count + 1))
  done
  printf '%s\n' "$count"
}

codex_as_gateway_main_pids() {
  local unit pid
  for unit in $CODEX_AS_GATEWAY_UNITS; do
    [[ $unit =~ ^claude-api-openai@[0-9]+\.service$ ]] || return 1
    "$CODEX_AS_SYSTEMCTL" is-active --quiet "$unit" >/dev/null 2>&1 || continue
    pid=$("$CODEX_AS_SYSTEMCTL" show -p MainPID --value "$unit" 2>/dev/null) || return 1
    [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
    printf '%s\n' "$pid"
  done
}

# Exact availability owners that must survive one cutover admission unchanged. A unit name keeps
# the snapshot unambiguous even if the kernel later reuses a PID. The legacy singleton is included
# only during the one-time ownership migration where it remains the old traffic anchor.
codex_as_cutover_owner_snapshot() {
  local unit pid
  for unit in $CODEX_AS_GATEWAY_UNITS; do
    [[ $unit =~ ^claude-api-openai@[0-9]+\.service$ ]] || return 1
    "$CODEX_AS_SYSTEMCTL" is-active --quiet "$unit" >/dev/null 2>&1 || continue
    pid=$("$CODEX_AS_SYSTEMCTL" show -p MainPID --value "$unit" 2>/dev/null) || return 1
    [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
    printf '%s=%s\n' "$unit" "$pid"
  done
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    pid=$("$CODEX_AS_SYSTEMCTL" show -p MainPID --value "$CODEX_AS_LEGACY_UNIT" 2>/dev/null) \
      || return 1
    [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
    printf '%s=%s\n' "$CODEX_AS_LEGACY_UNIT" "$pid"
  fi
}

codex_as_proxy_matches_client() {
  local proxy_pid=$1 gateway_pid=$2 home=$3 lease=$4 entry found_home=0 found_lease=0
  local -a args
  [[ $proxy_pid =~ ^[1-9][0-9]*$ && $gateway_pid =~ ^[1-9][0-9]*$ ]] || return 1
  [[ -r $CODEX_AS_PROC_ROOT/$proxy_pid/environ \
      && -r $CODEX_AS_PROC_ROOT/$proxy_pid/cmdline \
      && -r $CODEX_AS_PROC_ROOT/$proxy_pid/status ]] || return 1
  [[ $(awk '$1 == "PPid:" { print $2 }' "$CODEX_AS_PROC_ROOT/$proxy_pid/status") \
      == "$gateway_pid" ]] || return 1
  while IFS= read -r -d '' entry; do
    [[ $entry == "CODEX_HOME=$home" ]] && found_home=1
    [[ $entry == "CLAUDE_API_CODEX_CLIENT_LEASE=$lease" ]] && found_lease=1
  done <"$CODEX_AS_PROC_ROOT/$proxy_pid/environ" 2>/dev/null || true
  (( found_home == 1 && found_lease == 1 )) || return 1
  args=()
  while IFS= read -r -d '' entry; do args+=("$entry"); done \
    <"$CODEX_AS_PROC_ROOT/$proxy_pid/cmdline" 2>/dev/null || true
  local index
  for (( index = 0; index + 1 < ${#args[@]}; index++ )); do
    [[ ${args[index]} == app-server && ${args[index + 1]} == proxy ]] && return 0
  done
  return 1
}

# List the live gateway generations through which one home has completed websocket initialization
# and account/read. A marker names both gateway and proxy PIDs; its random per-process lease prevents
# PID reuse from manufacturing readiness.
codex_as_home_ready_gateway_pids() {
  local home=$1 name id marker base gateway_pid proxy_pid lease owner expected_uid expected_gid
  local gateway_pid_lines active_pids=' ' counted=' '
  name=${home##*/}
  id=$(codex_as_home_id "$name") || return 1
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  gateway_pid_lines=$(codex_as_gateway_main_pids) || return 1
  while IFS= read -r gateway_pid; do
    [[ $gateway_pid =~ ^[1-9][0-9]*$ ]] || continue
    active_pids+="$gateway_pid "
  done <<<"$gateway_pid_lines"
  for marker in "$CODEX_AS_RUNTIME_DIR/$id.client."*.ready; do
    [[ -e $marker || -L $marker ]] || continue
    base=${marker##*/}
    [[ $base =~ ^${id}\.client\.([1-9][0-9]*)\.([1-9][0-9]*)\.([0-9a-f]{32})\.ready$ ]] \
      || { codex_as_fail 'malformed app-server client marker'; return 1; }
    gateway_pid=${BASH_REMATCH[1]}
    proxy_pid=${BASH_REMATCH[2]}
    lease=${BASH_REMATCH[3]}
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" \
      || { codex_as_fail 'unsafe app-server client marker'; return 1; }
    [[ $(<"$marker") == "$CODEX_AS_CLIENT_MARKER" ]] \
      || { codex_as_fail 'invalid app-server client marker'; return 1; }
    if [[ $active_pids != *" $gateway_pid "* ]] \
        || ! codex_as_proxy_matches_client "$proxy_pid" "$gateway_pid" "$home" "$lease"; then
      continue
    fi
    [[ $counted != *" $gateway_pid "* ]] || continue
    counted+="$gateway_pid "
    printf '%s\n' "$gateway_pid"
  done
}

# Count only authenticated websocket clients, not proxy processes that have merely spawned.
codex_as_home_ready_client_count() {
  local gateway_pids count
  gateway_pids=$(codex_as_home_ready_gateway_pids "$1") || return 1
  count=$(grep -c . <<<"$gateway_pids" || true)
  printf '%s\n' "$count"
}

# Authenticated home IDs visible through one exact HTTP gateway generation. IDs are opaque and
# emitted in deterministic desired-roster order, so sets can be compared without exposing accounts.
codex_as_gateway_ready_home_ids() {
  local gateway_pid=$1 id name home gateway_pids
  [[ $gateway_pid =~ ^[1-9][0-9]*$ ]] || return 1
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    codex_as_home_draining "$home" && continue
    codex_as_unit_healthy "$id" || continue
    gateway_pids=$(codex_as_home_ready_gateway_pids "$home") || return 1
    grep -Fxq -- "$gateway_pid" <<<"$gateway_pids" || continue
    printf '%s\n' "$id"
  done < <(codex_as_home_records)
}

# Return the cohort size only when every live shared gateway sees the exact same authenticated home
# set. This is the OpenAI equivalent of Claude's two HTTP generations reading the same live pool:
# one home is sufficient, but a candidate missing any home that the old generation can serve never
# earns cutover ownership.
codex_as_cutover_ready_home_count() {
  local gateway_pids gateway_pid ids reference='' seen=0 count
  gateway_pids=$(codex_as_gateway_main_pids) || return 1
  [[ -n $gateway_pids ]] || { printf '0\n'; return 0; }
  while IFS= read -r gateway_pid; do
    [[ $gateway_pid =~ ^[1-9][0-9]*$ ]] || return 1
    ids=$(codex_as_gateway_ready_home_ids "$gateway_pid") || return 1
    if (( seen == 0 )); then
      reference=$ids
      seen=1
    elif [[ $ids != "$reference" ]]; then
      printf '0\n'
      return 0
    fi
  done <<<"$gateway_pids"
  count=$(grep -c . <<<"$reference" || true)
  printf '%s\n' "$count"
}

codex_as_begin_drain() {
  local home=$1 marker=$1/$CODEX_AS_DRAIN_FILE owner expected_uid expected_gid temporary
  if [[ -e $marker || -L $marker ]]; then
    codex_as_home_draining "$home" || return 1
    return 0
  fi
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  temporary=$home/.$CODEX_AS_DRAIN_FILE.$$
  if ! printf '%s\n' "$CODEX_AS_DRAIN_SENTINEL" >"$temporary" \
      || ! chown "$expected_uid:$expected_gid" "$temporary" \
      || ! chmod 0600 "$temporary" \
      || ! mv -- "$temporary" "$marker"; then
    rm -f -- "$temporary"
    return 1
  fi
}

codex_as_end_drain() {
  local home=$1 marker=$1/$CODEX_AS_DRAIN_FILE
  codex_as_home_draining "$home" || return 1
  rm -f -- "$marker"
  codex_as_signal_gateways || return 1
  codex_as_wait_proxy_restore "$home"
}

codex_as_wait_proxy_drain() {
  local home=$1 proxies deadline=$(( $(date +%s) + CODEX_AS_RETIRE_TIMEOUT ))
  codex_as_signal_gateways || return 1
  while true; do
    proxies=$(codex_as_home_proxy_count "$home") || return 1
    (( proxies == 0 )) && return 0
    (( $(date +%s) < deadline )) || return 1
    sleep 1
  done
}

codex_as_wait_proxy_restore() {
  local home=$1 gateways clients consecutive=0
  local deadline=$(( $(date +%s) + CODEX_AS_READY_TIMEOUT ))
  while (( $(date +%s) < deadline )); do
    gateways=$(codex_as_gateway_active_count) || return 1
    (( gateways == 0 )) && return 0
    clients=$(codex_as_home_ready_client_count "$home") || return 1
    if (( clients >= gateways )); then
      consecutive=$((consecutive + 1))
      (( consecutive >= 2 )) && return 0
    else
      consecutive=0
    fi
    sleep 1
  done
  return 1
}

codex_as_authenticated_home_count() {
  local gateways id name home clients count=0
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways >= 1 )) || { printf '0\n'; return 0; }
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    codex_as_home_draining "$home" && continue
    codex_as_unit_healthy "$id" || continue
    clients=$(codex_as_home_ready_client_count "$home") || return 1
    (( clients >= gateways )) || continue
    count=$((count + 1))
  done < <(codex_as_home_records)
  printf '%s\n' "$count"
}

# A newly expired account must not turn an otherwise safe engine rollout into an availability
# incident. Wait for at least one authenticated home, while requiring every counted home to be
# attached to every live HTTP generation. Rolling a daemon remains separately guarded by
# `codex_as_start_or_roll`, which refuses to restart the last serving home.
codex_as_wait_ready_cohort() {
  local required_consecutive=${1:-2}
  local ready consecutive=0 deadline=$(( $(date +%s) + CODEX_AS_READY_TIMEOUT ))
  [[ $CODEX_AS_MIN_READY =~ ^[1-9][0-9]*$ ]] || return 1
  [[ $required_consecutive =~ ^[1-9][0-9]*$ ]] || return 1
  (( required_consecutive < CODEX_AS_READY_TIMEOUT )) || return 1
  while (( $(date +%s) < deadline )); do
    ready=$(codex_as_authenticated_home_count) || return 1
    if (( ready >= CODEX_AS_MIN_READY )); then
      consecutive=$((consecutive + 1))
      (( consecutive >= required_consecutive )) && { printf '%s\n' "$ready"; return 0; }
    else
      consecutive=0
    fi
    sleep 1
  done
  return 1
}

# Prove that the same authenticated cohort stays continuously ready for an elapsed interval.
# A health observation is intentionally expensive: it validates every marker, gateway MainPID,
# proxy parent, lease, and CODEX_HOME. Counting observations as seconds made a 60-second policy take
# roughly 180 seconds in production and collide with its own deadline. Track monotonic elapsed time
# instead; an unhealthy observation resets the interval, while observation cost counts naturally.
codex_as_wait_ready_cohort_stable() {
  local stable_seconds=$1 expected_owners=${2:-} owners ready now stable_since=-1
  local deadline=$(( $(codex_as_monotonic_seconds) + CODEX_AS_READY_TIMEOUT ))
  [[ $CODEX_AS_MIN_READY =~ ^[1-9][0-9]*$ ]] || return 1
  [[ $stable_seconds =~ ^[1-9][0-9]*$ ]] || return 1
  [[ -n $expected_owners ]] || return 1
  (( stable_seconds < CODEX_AS_READY_TIMEOUT )) || return 1
  while true; do
    now=$(codex_as_monotonic_seconds) || return 1
    (( now <= deadline )) || return 1
    owners=$(codex_as_cutover_owner_snapshot) || return 1
    [[ $owners == "$expected_owners" ]] || return 1
    ready=$(codex_as_cutover_ready_home_count) || return 1
    # Fence both sides of the expensive full-cohort observation. A gateway generation that exits
    # or restarts during the query is a failed candidate, even if systemd has already replaced it.
    owners=$(codex_as_cutover_owner_snapshot) || return 1
    [[ $owners == "$expected_owners" ]] || return 1
    now=$(codex_as_monotonic_seconds) || return 1
    (( now <= deadline )) || return 1
    if (( ready >= CODEX_AS_MIN_READY )); then
      (( stable_since >= 0 )) || stable_since=$now
      if (( now - stable_since >= stable_seconds )); then
        printf '%s\n' "$ready"
        return 0
      fi
    else
      stable_since=-1
    fi
    codex_as_wait_tick
  done
}

codex_as_admit_cutover() {
  local gateways owners ready mode ids transition_count
  codex_as_is_root \
    || { codex_as_fail 'cutover admission must run as root'; return 1; }
  [[ $CODEX_AS_CUTOVER_STABILITY_SECONDS =~ ^[1-9][0-9]*$ ]] \
    || { codex_as_fail 'cutover stability window is malformed'; return 1; }
  codex_as_load_desired || return 1
  owners=$(codex_as_cutover_owner_snapshot) || return 1
  [[ -n $owners ]] \
    || { codex_as_fail 'cutover has no stable gateway ownership snapshot'; return 1; }
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways >= 1 )) \
    || { codex_as_fail 'no shared OpenAI gateway is active for cutover admission'; return 1; }
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] \
      || { codex_as_fail 'legacy overlap has no persisted ownership transition'; return 1; }
    mode=$(codex_as_transition_value mode) || return 1
    [[ $mode == "$CODEX_AS_TRANSITION_TO_DAEMON" ]] \
      || { codex_as_fail 'legacy overlap has the wrong ownership transition'; return 1; }
    ids=$(codex_as_transition_home_ids) || return 1
    transition_count=$(grep -c . <<<"$ids" || true)
    (( transition_count >= CODEX_AS_MIGRATION_SEED_COUNT )) \
      || { codex_as_fail 'legacy overlap has no disjoint daemon migration seed'; return 1; }
  fi
  # Admission is deliberately observational. Candidate startup already discovers the persistent
  # daemon sockets; a deployment gate must not send signals, restart transports, or issue account
  # RPCs against sessions that are already working.
  ready=$(codex_as_wait_ready_cohort_stable \
    "$CODEX_AS_CUTOVER_STABILITY_SECONDS" "$owners") \
    || { codex_as_fail 'old and candidate gateways did not retain the same authenticated Codex cohort throughout cutover admission'; return 1; }
  codex_as_log "$ready app-server(s) survived the ${CODEX_AS_CUTOVER_STABILITY_SECONDS}s cutover admission window across $gateways gateway(s)"
}

codex_as_start_or_roll() {
  local id=$1 unit home others
  unit=$(codex_as_unit "$id")
  home=$(codex_as_find_home "$id") || return 1
  if codex_as_unit_healthy "$id"; then
    if codex_as_home_draining "$home"; then
      CODEX_AS_RECONCILE_CHANGED=1
      codex_as_end_drain "$home" || return 1
    fi
    return 0
  fi
  if codex_as_unit_active "$id"; then
    others=$(codex_as_other_serving_count "$id") || return 1
    (( others >= 1 )) \
      || { codex_as_fail 'refusing to restart the last serving Codex app-server'; return 1; }
    codex_as_begin_drain "$home" \
      || { codex_as_fail "could not drain app-server $id"; return 1; }
    if ! codex_as_wait_proxy_drain "$home"; then
      codex_as_end_drain "$home" || true
      codex_as_fail "gateway proxies did not drain app-server $id"
      return 1
    fi
    codex_as_log "rolling app-server $id while $others peer(s) remain serving"
    if ! "$CODEX_AS_SYSTEMCTL" restart "$unit"; then
      codex_as_fail "could not restart app-server $id"
      return 1
    fi
    CODEX_AS_RECONCILE_CHANGED=1
  else
    codex_as_log "starting app-server $id"
    "$CODEX_AS_SYSTEMCTL" start "$unit" \
      || { codex_as_fail "could not start app-server $id"; return 1; }
    CODEX_AS_RECONCILE_CHANGED=1
  fi
  if ! codex_as_wait_healthy "$id"; then
    codex_as_fail "app-server $id did not become ready"
    return 1
  fi
  if codex_as_home_draining "$home"; then
    codex_as_end_drain "$home" \
      || { codex_as_fail "could not return app-server $id to rotation"; return 1; }
  fi
}

codex_as_transition_value() {
  codex_as_state_value "$CODEX_AS_TRANSITION_FILE" "$1"
}

codex_as_transition_home_ids() {
  local raw id seen=' '
  local -a ids
  raw=$(codex_as_transition_value home_ids) || return 1
  read -r -a ids <<<"$raw"
  (( ${#ids[@]} >= 1 )) || return 1
  for id in "${ids[@]}"; do
    [[ $id =~ ^[0-9a-f]{16}$ && $seen != *" $id "* ]] || return 1
    seen+="$id "
    printf '%s\n' "$id"
  done
}

codex_as_reconcile_transition() {
  local mode id ids gateways
  mode=$(codex_as_transition_value mode) || return 1
  case "$mode" in
    "$CODEX_AS_TRANSITION_TO_DAEMON")
      ids=$(codex_as_transition_home_ids) || return 1
      while IFS= read -r id; do
        codex_as_start_or_roll "$id" || return 1
      done <<<"$ids"
      ;;
    "$CODEX_AS_TRANSITION_TO_LEGACY")
      gateways=$(codex_as_gateway_active_count) || return 1
      if (( gateways == 0 )); then
        codex_as_commit_legacy_transition
      else
        codex_as_log 'legacy rollback transition is controller-owned while a shared gateway remains active'
      fi
      ;;
    *) codex_as_fail 'OpenAI ownership transition mode is invalid' ;;
  esac
}

codex_as_stop_all_units() {
  local unit id
  while IFS= read -r unit; do
    [[ $unit =~ ^${CODEX_AS_UNIT_TEMPLATE}([0-9a-f]{16})\.service$ ]] || continue
    id=${BASH_REMATCH[1]}
    "$CODEX_AS_SYSTEMCTL" stop "$unit" || return 1
    "$CODEX_AS_SYSTEMCTL" disable "$unit" || return 1
    rm -f -- "$CODEX_AS_RUNTIME_DIR/$id.state" "$(codex_as_socket "$id")"
  done < <("$CODEX_AS_SYSTEMCTL" list-units --all --plain --no-legend \
    "${CODEX_AS_UNIT_TEMPLATE}*.service" 2>/dev/null | awk '{print $1}')
}

codex_as_reconcile() {
  local id name unit id_list=' ' desired_count=0 mode gateways ready records
  codex_as_is_root \
    || { codex_as_fail 'reconcile must run as root'; return 1; }
  codex_as_load_desired || return 1
  CODEX_AS_RECONCILE_CHANGED=0
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    if [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]]; then
      codex_as_reconcile_transition
    else
      codex_as_log 'legacy OpenAI owner is active; daemon reconciliation is deferred'
    fi
    return
  fi
  if [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]]; then
    mode=$(codex_as_transition_value mode) || return 1
    if [[ $mode == "$CODEX_AS_TRANSITION_TO_LEGACY" ]]; then
      codex_as_log 'legacy rollback transition is waiting for its controller; daemon reconciliation is deferred'
      return 0
    fi
    [[ $mode == "$CODEX_AS_TRANSITION_TO_DAEMON" ]] \
      || { codex_as_fail 'OpenAI ownership transition mode is invalid'; return 1; }
  fi
  [[ $CODEX_AS_MIN_READY =~ ^[1-9][0-9]*$ ]] || return 1
  records=$(codex_as_home_records) || return 1
  desired_count=$(grep -c . <<<"$records" || true)
  # Validate the complete desired snapshot before starting, rolling, or retiring anything. A
  # transiently incomplete auth directory must never make reconciliation dismantle the serving
  # cohort and only discover afterwards that redundancy has been lost.
  (( desired_count >= CODEX_AS_MIN_READY )) \
    || { codex_as_fail "only $desired_count authenticated Codex home(s) discovered; refusing to mutate the serving cohort"; return 1; }
  while IFS=$'\t' read -r id name; do
    id_list+="$id "
    codex_as_start_or_roll "$id" || return 1
  done <<<"$records"

  while IFS= read -r unit; do
    [[ $unit =~ ^${CODEX_AS_UNIT_TEMPLATE}([0-9a-f]{16})\.service$ ]] || continue
    id=${BASH_REMATCH[1]}
    [[ $id_list == *" $id "* ]] && continue
    codex_as_log "stopping retired app-server $id"
    "$CODEX_AS_SYSTEMCTL" stop "$unit" || return 1
    "$CODEX_AS_SYSTEMCTL" disable "$unit" || return 1
    rm -f -- "$CODEX_AS_RUNTIME_DIR/$id.state" "$(codex_as_socket "$id")"
    CODEX_AS_RECONCILE_CHANGED=1
  done < <("$CODEX_AS_SYSTEMCTL" list-units --all --plain --no-legend \
    "${CODEX_AS_UNIT_TEMPLATE}*.service" 2>/dev/null | awk '{print $1}')

  # Only a real daemon/topology change asks gateways to rediscover. A steady-state timer pass is
  # observational and must never perturb working proxy children or issue account RPCs merely to
  # "repair" something. Newly started homes still join in the same reconciliation.
  gateways=$(codex_as_gateway_active_count) || return 1
  if (( gateways >= 1 )); then
    if (( CODEX_AS_RECONCILE_CHANGED == 1 )); then
      codex_as_signal_gateways || return 1
      ready=$(codex_as_wait_ready_cohort) \
        || { codex_as_fail "fewer than $CODEX_AS_MIN_READY app-server(s) authenticated through every gateway"; return 1; }
    else
      ready=$(codex_as_authenticated_home_count) || return 1
      (( ready >= CODEX_AS_MIN_READY )) \
        || { codex_as_fail "steady daemon cohort has fewer than $CODEX_AS_MIN_READY authenticated app-server(s)"; return 1; }
    fi
    codex_as_log "$ready of $desired_count app-server(s) are authenticated through every gateway"
  fi

  # If the singleton is already gone, daemon ownership is authoritative. Complete an interrupted
  # transition only after the whole cohort converged; until then the seed keeps the target serving.
  if [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]]; then
    [[ $(codex_as_transition_value mode) == "$CODEX_AS_TRANSITION_TO_DAEMON" ]] || return 1
    codex_as_restore_all_transition_markers || return 1
    rm -f -- "$CODEX_AS_TRANSITION_FILE"
    codex_as_log 'completed interrupted OpenAI ownership transition'
  fi
}

codex_as_home_has_process() {
  local home=$1 environ entry
  for environ in "$CODEX_AS_PROC_ROOT"/[0-9]*/environ; do
    [[ -r $environ ]] || continue
    while IFS= read -r -d '' entry; do
      [[ $entry == "CODEX_HOME=$home" ]] && return 0
    done <"$environ" 2>/dev/null || true
  done
  return 1
}

codex_as_install_transition_marker() {
  local home=$1 expected_uid=$2 expected_gid=$3 marker saved had_proxy=0 temporary
  marker=$home/$CODEX_AS_PROXY_FILE
  saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
  [[ ! -e $saved && ! -L $saved ]] || return 1
  temporary=$home/.proxy.url.openai-bluegreen.$$
  if ! printf '%s\n' "$CODEX_AS_TRANSITION_SENTINEL" >"$temporary" \
      || ! chown "$expected_uid:$expected_gid" "$temporary" \
      || ! chmod 0600 "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  if [[ -e $marker || -L $marker ]]; then
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
    mv -- "$marker" "$saved"
    had_proxy=1
  fi
  if ! mv -- "$temporary" "$marker"; then
    [[ $had_proxy == 1 ]] && mv -- "$saved" "$marker"
    return 1
  fi
  printf '%s\n' "$had_proxy"
}

codex_as_restore_transition_marker() {
  local home=$1 had_proxy=$2 marker saved owner expected_uid expected_gid
  marker=$home/$CODEX_AS_PROXY_FILE
  saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
  [[ -f $marker && ! -L $marker && $(<"$marker") == "$CODEX_AS_TRANSITION_SENTINEL" ]] \
    || return 1
  case "$had_proxy" in
    1) codex_as_validate_secret "$saved" "$expected_uid" "$expected_gid" || return 1; mv -f -- "$saved" "$marker" ;;
    0) [[ ! -e $saved && ! -L $saved ]] || return 1; rm -f -- "$marker" ;;
    *) return 1 ;;
  esac
}

codex_as_ensure_transition_marker() {
  local home=$1 expected_uid=$2 expected_gid=$3 marker saved
  marker=$home/$CODEX_AS_PROXY_FILE
  saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
  if [[ -e $saved || -L $saved ]]; then
    codex_as_validate_secret "$saved" "$expected_uid" "$expected_gid" || return 1
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
    [[ $(<"$marker") == "$CODEX_AS_TRANSITION_SENTINEL" ]] || return 1
    return 0
  fi
  if [[ -e $marker || -L $marker ]]; then
    codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
    if [[ $(<"$marker") == "$CODEX_AS_TRANSITION_SENTINEL" ]]; then
      return 0
    fi
  fi
  codex_as_install_transition_marker "$home" "$expected_uid" "$expected_gid" >/dev/null
}

codex_as_restore_all_transition_markers() {
  local owner expected_uid expected_gid id name home marker saved had_proxy
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    marker=$home/$CODEX_AS_PROXY_FILE
    saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
    if [[ -e $saved || -L $saved ]]; then
      had_proxy=1
    elif [[ -e $marker || -L $marker ]]; then
      codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
      [[ $(<"$marker") == "$CODEX_AS_TRANSITION_SENTINEL" ]] || continue
      had_proxy=0
    else
      continue
    fi
    codex_as_restore_transition_marker "$home" "$had_proxy" || return 1
  done < <(codex_as_home_records)
}

codex_as_transition_artifacts_absent() {
  local owner expected_uid expected_gid id name home marker saved
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    marker=$home/$CODEX_AS_PROXY_FILE
    saved=$home/$CODEX_AS_TRANSITION_PROXY_FILE
    [[ ! -e $saved && ! -L $saved ]] || return 1
    if [[ -e $marker || -L $marker ]]; then
      codex_as_validate_secret "$marker" "$expected_uid" "$expected_gid" || return 1
      [[ $(<"$marker") != "$CODEX_AS_TRANSITION_SENTINEL" ]] || return 1
    fi
  done < <(codex_as_home_records)
}

codex_as_select_transition_seeds() {
  local records=$1 required=$2 id name selected='' count=0
  [[ $required =~ ^[1-9][0-9]*$ ]] || return 1
  while IFS=$'\t' read -r id name; do
    [[ $name == "$CODEX_AS_LEGACY_EXPLICIT_HOME_NAME" ]] && continue
    selected+="$id"$'\t'"$name"$'\n'
    count=$((count + 1))
    (( count < required )) || break
  done <<<"$records"
  (( count == required )) || return 1
  printf '%s' "$selected"
}

codex_as_select_legacy_seed() {
  local records=$1 id name seed=''
  while IFS=$'\t' read -r id name; do
    [[ $name == "$CODEX_AS_LEGACY_EXPLICIT_HOME_NAME" ]] || continue
    [[ -z $seed ]] || return 1
    seed="$id"$'\t'"$name"
  done <<<"$records"
  [[ -n $seed ]] || return 1
  printf '%s\n' "$seed"
}

codex_as_prepare_transition() {
  local owner expected_uid expected_gid records count id name home deadline temporary
  local legacy_seed legacy_id legacy_name legacy_home seeds home_ids='' ignored_had_proxy
  codex_as_is_root \
    || { codex_as_fail 'transition preparation must run as root'; return 1; }
  [[ ! -e $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] \
    || { codex_as_fail 'an OpenAI ownership transition is already active'; return 1; }
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1 \
    || { codex_as_fail 'legacy OpenAI owner is not active'; return 1; }
  codex_as_load_desired || return 1
  [[ $CODEX_AS_MIGRATION_SEED_COUNT =~ ^[1-9][0-9]*$ ]] \
    || { codex_as_fail 'migration seed count is malformed'; return 1; }
  records=$(codex_as_home_records) || return 1
  count=$(grep -c . <<<"$records" || true)
  (( count >= CODEX_AS_MIGRATION_SEED_COUNT + 1 )) \
    || { codex_as_fail "$CODEX_AS_MIGRATION_SEED_COUNT disjoint seed plus the legacy anchor are required for the one-time ownership migration"; return 1; }
  legacy_seed=$(codex_as_select_legacy_seed "$records") \
    || { codex_as_fail 'the explicitly configured legacy anchor is unavailable'; return 1; }
  IFS=$'\t' read -r legacy_id legacy_name <<<"$legacy_seed"
  legacy_home=$CODEX_AS_HOMES_DIR/$legacy_name
  codex_as_home_has_process "$legacy_home" \
    || { codex_as_fail 'the legacy anchor does not own its explicit home'; return 1; }
  seeds=$(codex_as_select_transition_seeds "$records" "$CODEX_AS_MIGRATION_SEED_COUNT") \
    || { codex_as_fail "no discovered-only home can seed the one-time ownership migration"; return 1; }
  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  codex_as_secure_control_dir || return 1
  codex_as_transition_artifacts_absent \
    || { codex_as_fail 'stale OpenAI transition artifacts require repair'; return 1; }
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    if ! ignored_had_proxy=$(codex_as_install_transition_marker \
        "$home" "$expected_uid" "$expected_gid"); then
      codex_as_restore_all_transition_markers || true
      codex_as_fail 'could not fence the redundant transition cohort from the legacy owner'
      return 1
    fi
    home_ids+="${home_ids:+ }$id"
  done <<<"$seeds"
  temporary=$CODEX_AS_TRANSITION_FILE.tmp.$$
  if ! printf 'mode=%s\nhome_ids=%s\n' \
      "$CODEX_AS_TRANSITION_TO_DAEMON" "$home_ids" >"$temporary" \
      || ! chmod 0600 "$temporary" \
      || ! mv -- "$temporary" "$CODEX_AS_TRANSITION_FILE"; then
    rm -f -- "$temporary"
    codex_as_restore_all_transition_markers || true
    codex_as_fail 'could not persist the OpenAI ownership transition'
    return 1
  fi

  deadline=$(( $(date +%s) + CODEX_AS_RETIRE_TIMEOUT ))
  while IFS=$'\t' read -r id name; do
    home=$CODEX_AS_HOMES_DIR/$name
    codex_as_log "waiting for legacy gateway to drain transition home $id"
    while codex_as_home_has_process "$home"; do
      if (( $(date +%s) >= deadline )); then
        codex_as_abort_transition || true
        codex_as_fail 'legacy gateway did not retire the redundant transition cohort in time'
        return 1
      fi
      sleep 1
    done
  done <<<"$seeds"
  while IFS= read -r id; do
    codex_as_start_or_roll "$id" || { codex_as_abort_transition; return 1; }
    codex_as_log "transition app-server $id is ready"
  done < <(printf '%s\n' "$home_ids" | tr ' ' '\n')
}

codex_as_abort_transition() {
  local mode
  codex_as_is_root \
    || { codex_as_fail 'transition abort must run as root'; return 1; }
  [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] || return 0
  mode=$(codex_as_transition_value mode) || return 1
  [[ $mode == "$CODEX_AS_TRANSITION_TO_DAEMON" ]] \
    || { codex_as_fail 'active transition is not a daemon handoff'; return 1; }
  # A failed commit may already have started more than the seed daemon. The legacy singleton may
  # be restarted only after every daemon owner is gone, otherwise two processes would share auth.
  codex_as_stop_all_units || return 1
  codex_as_restore_all_transition_markers || return 1
  rm -f -- "$CODEX_AS_TRANSITION_FILE"
  codex_as_log 'OpenAI ownership transition was rolled back'
}

codex_as_commit_transition() {
  local mode
  codex_as_is_root \
    || { codex_as_fail 'transition commit must run as root'; return 1; }
  [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] \
    || { codex_as_fail 'no OpenAI ownership transition is active'; return 1; }
  mode=$(codex_as_transition_value mode) || return 1
  [[ $mode == "$CODEX_AS_TRANSITION_TO_DAEMON" ]] \
    || { codex_as_fail 'active transition is not a daemon handoff'; return 1; }
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    codex_as_fail 'legacy OpenAI owner is still active'
    return 1
  fi
  # Reconciliation converges every daemon first and atomically finishes the reversible sentinel.
  # If it fails, the admitted target keeps serving through the seed and the timer resumes the work.
  codex_as_reconcile || return 1
  codex_as_log 'OpenAI ownership transition committed'
}

# Rollback across the first shared-daemon release needs the inverse handoff. The old singleton is
# explicitly configured with one legacy home and scans the rest. Fence only those scanned homes,
# drain the explicit home from the shared gateway, then the singleton can start on 8793 without any
# auth.json having two owners. At least one other daemon keeps the shared generation available.
codex_as_prepare_legacy_transition() {
  local owner expected_uid expected_gid records count seed id name home mode state_id
  local other_id other_name other_home gateways others temporary
  codex_as_is_root \
    || { codex_as_fail 'legacy transition preparation must run as root'; return 1; }
  codex_as_load_desired || return 1
  records=$(codex_as_home_records) || return 1
  count=$(grep -c . <<<"$records" || true)
  (( count >= 2 )) \
    || { codex_as_fail 'at least two authenticated homes are required for zero-downtime rollback'; return 1; }
  seed=$(codex_as_select_legacy_seed "$records") \
    || { codex_as_fail 'the explicitly configured legacy home is unavailable'; return 1; }
  IFS=$'\t' read -r id name <<<"$seed"
  home=$CODEX_AS_HOMES_DIR/$name
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways == 1 )) \
    || { codex_as_fail 'legacy rollback requires exactly one active shared gateway'; return 1; }
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_ROLLBACK_GATEWAY_UNIT" >/dev/null 2>&1 \
    || { codex_as_fail 'legacy rollback requires the shared gateway on the rollback-safe slot'; return 1; }

  if [[ -e $CODEX_AS_TRANSITION_FILE || -L $CODEX_AS_TRANSITION_FILE ]]; then
    [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] || return 1
    mode=$(codex_as_transition_value mode) || return 1
    state_id=$(codex_as_transition_value home_id) || return 1
    [[ $mode == "$CODEX_AS_TRANSITION_TO_LEGACY" && $state_id == "$id" ]] \
      || { codex_as_fail 'another OpenAI ownership transition is already active'; return 1; }
  else
    "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1 \
      && { codex_as_fail 'legacy OpenAI owner is already active'; return 1; }
    codex_as_transition_artifacts_absent \
      || { codex_as_fail 'stale OpenAI transition artifacts require repair'; return 1; }
    codex_as_unit_healthy "$id" \
      || { codex_as_fail 'the legacy seed app-server is not converged and ready'; return 1; }
    others=$(codex_as_other_serving_count "$id")
    (( others >= 1 )) \
      || { codex_as_fail 'no peer app-server can carry traffic during legacy rollback'; return 1; }
    codex_as_secure_control_dir || return 1
    temporary=$CODEX_AS_TRANSITION_FILE.tmp.$$
    if ! printf 'mode=%s\nhome_id=%s\n' "$CODEX_AS_TRANSITION_TO_LEGACY" "$id" >"$temporary" \
        || ! chmod 0600 "$temporary" \
        || ! mv -- "$temporary" "$CODEX_AS_TRANSITION_FILE"; then
      rm -f -- "$temporary"
      codex_as_fail 'could not persist the legacy OpenAI ownership transition'
      return 1
    fi
  fi

  owner=$(codex_as_expected_owner) || return 1
  read -r expected_uid expected_gid <<<"$owner"
  while IFS=$'\t' read -r other_id other_name; do
    [[ $other_id == "$id" ]] && continue
    other_home=$CODEX_AS_HOMES_DIR/$other_name
    if ! codex_as_ensure_transition_marker "$other_home" "$expected_uid" "$expected_gid"; then
      codex_as_abort_legacy_transition || true
      codex_as_fail 'could not fence a discovered home from the legacy owner'
      return 1
    fi
  done <<<"$records"

  codex_as_begin_drain "$home" \
    || { codex_as_abort_legacy_transition || true; codex_as_fail 'could not fence the legacy seed from shared gateways'; return 1; }
  if ! codex_as_wait_proxy_drain "$home"; then
    codex_as_abort_legacy_transition || true
    codex_as_fail 'shared gateways did not release the legacy seed in time'
    return 1
  fi
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    codex_as_unit_active "$id" \
      && { codex_as_fail 'legacy singleton and seed daemon overlap'; return 1; }
    codex_as_log "legacy singleton already owns drained seed $id"
    return 0
  fi
  if codex_as_unit_active "$id"; then
    if ! "$CODEX_AS_SYSTEMCTL" stop "$(codex_as_unit "$id")"; then
      codex_as_abort_legacy_transition || true
      codex_as_fail 'could not stop the legacy seed app-server'
      return 1
    fi
    rm -f -- "$CODEX_AS_RUNTIME_DIR/$id.state"
  fi
  if codex_as_unit_active "$id" || codex_as_home_has_process "$home"; then
    codex_as_abort_legacy_transition || true
    codex_as_fail 'legacy seed still has a process owner'
    return 1
  fi
  codex_as_log "legacy seed $id is fenced and ready for singleton ownership"
}

codex_as_abort_legacy_transition() {
  local mode id home gateways
  codex_as_is_root \
    || { codex_as_fail 'legacy transition abort must run as root'; return 1; }
  [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] || return 0
  mode=$(codex_as_transition_value mode) || return 1
  [[ $mode == "$CODEX_AS_TRANSITION_TO_LEGACY" ]] \
    || { codex_as_fail 'active transition is not a legacy handoff'; return 1; }
  gateways=$(codex_as_gateway_active_count) || return 1
  if "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1; then
    (( gateways >= 1 )) \
      || { codex_as_fail 'cannot abort after the shared gateway stopped; commit the legacy handoff'; return 1; }
    "$CODEX_AS_SYSTEMCTL" stop "$CODEX_AS_LEGACY_UNIT" || return 1
  fi
  codex_as_load_desired || return 1
  id=$(codex_as_transition_value home_id) || return 1
  home=$(codex_as_find_home "$id") || return 1
  codex_as_start_or_roll "$id" || return 1
  if codex_as_home_draining "$home"; then
    codex_as_end_drain "$home" || return 1
  fi
  codex_as_restore_all_transition_markers || return 1
  rm -f -- "$CODEX_AS_TRANSITION_FILE"
  codex_as_log 'legacy OpenAI ownership transition was rolled back'
}

codex_as_commit_legacy_transition() {
  local mode id home gateways
  codex_as_is_root \
    || { codex_as_fail 'legacy transition commit must run as root'; return 1; }
  if [[ ! -e $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]]; then
    codex_as_verify_legacy
    return
  fi
  [[ -f $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] || return 1
  mode=$(codex_as_transition_value mode) || return 1
  [[ $mode == "$CODEX_AS_TRANSITION_TO_LEGACY" ]] \
    || { codex_as_fail 'active transition is not a legacy handoff'; return 1; }
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1 \
    || { codex_as_fail 'legacy OpenAI owner is not active'; return 1; }
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways == 0 )) \
    || { codex_as_fail 'shared OpenAI gateway is still active'; return 1; }
  id=$(codex_as_transition_value home_id) || return 1
  home=$(codex_as_find_home "$id") || return 1
  ! codex_as_unit_active "$id" \
    || { codex_as_fail 'legacy seed app-server is still active'; return 1; }
  [[ $(codex_as_home_proxy_count "$home") == 0 ]] \
    || { codex_as_fail 'legacy seed still has a shared proxy owner'; return 1; }
  codex_as_home_has_process "$home" \
    || { codex_as_fail 'legacy singleton has not acquired its seed home'; return 1; }
  codex_as_stop_all_units || return 1
  codex_as_restore_all_transition_markers || return 1
  if codex_as_home_draining "$home"; then rm -f -- "$home/$CODEX_AS_DRAIN_FILE"; fi
  rm -f -- "$CODEX_AS_TRANSITION_FILE"
  codex_as_log 'legacy OpenAI ownership transition committed'
}

codex_as_verify_legacy() {
  local id name unit count=0 gateways
  [[ ! -e $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] \
    || { codex_as_fail 'OpenAI ownership transition is incomplete'; return 1; }
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1 \
    || { codex_as_fail 'legacy OpenAI owner is not active'; return 1; }
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways == 0 )) \
    || { codex_as_fail 'a shared OpenAI gateway remains active'; return 1; }
  codex_as_load_desired || return 1
  codex_as_transition_artifacts_absent \
    || { codex_as_fail 'OpenAI transition artifacts remain'; return 1; }
  while IFS=$'\t' read -r id name; do
    count=$((count + 1))
    codex_as_home_draining "$CODEX_AS_HOMES_DIR/$name" \
      && { codex_as_fail "home $id remains drained"; return 1; }
    ! codex_as_unit_active "$id" \
      || { codex_as_fail "app-server $id remains active beside the legacy owner"; return 1; }
    codex_as_unit_reconciler_owned "$id" \
      || { codex_as_fail "app-server $id remains boot-enabled beside the legacy owner"; return 1; }
  done < <(codex_as_home_records)
  (( count >= 1 )) || { codex_as_fail 'no authenticated Codex homes are available'; return 1; }
  while IFS= read -r unit; do
    [[ $unit =~ ^${CODEX_AS_UNIT_TEMPLATE}([0-9a-f]{16})\.service$ ]] || continue
    id=${BASH_REMATCH[1]}
    ! codex_as_unit_active "$id" \
      || { codex_as_fail "retired app-server $id remains active"; return 1; }
  done < <("$CODEX_AS_SYSTEMCTL" list-units --all --plain --no-legend \
    "${CODEX_AS_UNIT_TEMPLATE}*.service" 2>/dev/null | awk '{print $1}')
  codex_as_log "legacy owner is isolated from $count stopped app-server home(s)"
}

codex_as_verify() {
  local id name unit count=0 id_list=' ' gateways ready
  [[ $CODEX_AS_MIN_READY =~ ^[1-9][0-9]*$ ]] \
    || { codex_as_fail 'minimum ready app-server count is malformed'; return 1; }
  [[ ! -e $CODEX_AS_TRANSITION_FILE && ! -L $CODEX_AS_TRANSITION_FILE ]] \
    || { codex_as_fail 'OpenAI ownership transition is incomplete'; return 1; }
  "$CODEX_AS_SYSTEMCTL" is-active --quiet "$CODEX_AS_LEGACY_UNIT" >/dev/null 2>&1 \
    && { codex_as_fail 'legacy OpenAI home owner is still active'; return 1; }
  codex_as_load_desired || return 1
  codex_as_transition_artifacts_absent \
    || { codex_as_fail 'OpenAI transition artifacts remain'; return 1; }
  while IFS=$'\t' read -r id name; do
    count=$((count + 1))
    id_list+="$id "
    ! codex_as_home_draining "$CODEX_AS_HOMES_DIR/$name" \
      || { codex_as_fail "home $id remains drained"; return 1; }
    codex_as_unit_reconciler_owned "$id" \
      || { codex_as_fail "app-server $id must be reconciler-owned rather than boot-enabled"; return 1; }
    codex_as_unit_healthy "$id" \
      || { codex_as_fail "app-server $id is not converged and ready"; return 1; }
  done < <(codex_as_home_records)
  (( count >= CODEX_AS_MIN_READY )) \
    || { codex_as_fail "only $count app-server daemon(s) exist; $CODEX_AS_MIN_READY required"; return 1; }
  gateways=$(codex_as_gateway_active_count) || return 1
  (( gateways >= 1 )) \
    || { codex_as_fail 'no shared OpenAI gateway is active'; return 1; }
  ready=$(codex_as_wait_ready_cohort) \
    || { codex_as_fail "fewer than $CODEX_AS_MIN_READY app-server(s) authenticated through every gateway"; return 1; }
  while IFS= read -r unit; do
    [[ $unit =~ ^${CODEX_AS_UNIT_TEMPLATE}([0-9a-f]{16})\.service$ ]] || continue
    id=${BASH_REMATCH[1]}
    [[ $id_list == *" $id "* ]] || ! codex_as_unit_active "$id" \
      || { codex_as_fail "retired app-server $id is still active"; return 1; }
  done < <("$CODEX_AS_SYSTEMCTL" list-units --all --plain --no-legend \
    "${CODEX_AS_UNIT_TEMPLATE}*.service" 2>/dev/null | awk '{print $1}')
  codex_as_log "$ready of $count app-server(s) are converged, authenticated and ready"
}

codex_as_main() {
  local command=${1:-}
  if [[ $command != serve ]]; then codex_as_acquire_lifecycle_lock || return 1; fi
  case "$command" in
    serve) [[ $# == 2 ]] || { codex_as_fail 'usage: codex-app-servers.sh serve <opaque-id>'; return 1; }; codex_as_serve "$2" ;;
    reconcile) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh reconcile'; return 1; }; codex_as_reconcile ;;
    admit-cutover) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh admit-cutover'; return 1; }; codex_as_admit_cutover ;;
    prepare-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh prepare-transition'; return 1; }; codex_as_prepare_transition ;;
    commit-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh commit-transition'; return 1; }; codex_as_commit_transition ;;
    abort-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh abort-transition'; return 1; }; codex_as_abort_transition ;;
    prepare-legacy-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh prepare-legacy-transition'; return 1; }; codex_as_prepare_legacy_transition ;;
    commit-legacy-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh commit-legacy-transition'; return 1; }; codex_as_commit_legacy_transition ;;
    abort-legacy-transition) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh abort-legacy-transition'; return 1; }; codex_as_abort_legacy_transition ;;
    verify) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh verify'; return 1; }; codex_as_verify ;;
    verify-legacy) [[ $# == 1 ]] || { codex_as_fail 'usage: codex-app-servers.sh verify-legacy'; return 1; }; codex_as_verify_legacy ;;
    *) codex_as_fail 'usage: codex-app-servers.sh serve <opaque-id>|reconcile|admit-cutover|prepare-transition|commit-transition|abort-transition|prepare-legacy-transition|commit-legacy-transition|abort-legacy-transition|verify|verify-legacy' ;;
  esac
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  codex_as_main "$@"
fi
