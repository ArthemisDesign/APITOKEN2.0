#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/apitoken-codex-app-servers-test.XXXXXXXX")
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf '[codex-app-servers-test] ERROR: %s\n' "$*" >&2; exit 1; }

CODEX_AS_HOMES_DIR=$TEMP/homes
CODEX_AS_CONFIG_ENV=$TEMP/config.env
CODEX_AS_RUNTIME_DIR=$TEMP/runtime
CODEX_AS_CONTROL_DIR=$TEMP/control
CODEX_AS_SERVICE_USER=$(id -un)
CODEX_AS_LEGACY_EXPLICIT_HOME_NAME=legacy-account
mkdir -p "$CODEX_AS_HOMES_DIR" "$CODEX_AS_RUNTIME_DIR" "$CODEX_AS_CONTROL_DIR"
chmod 0700 "$CODEX_AS_HOMES_DIR" "$CODEX_AS_RUNTIME_DIR" "$CODEX_AS_CONTROL_DIR"

# shellcheck source=deploy/codex-app-servers.sh
source "$ROOT/deploy/codex-app-servers.sh"

make_home() {
  local name=$1 home
  home=$CODEX_AS_HOMES_DIR/$name
  mkdir "$home"
  chmod 0700 "$home"
  printf '{}\n' >"$home/auth.json"
  chmod 0600 "$home/auth.json"
}

make_home legacy-account
make_home rotating-account
make_home standby-account

records=$(codex_as_home_records)
[[ $(grep -c . <<<"$records") == 3 ]] || fail 'authenticated home discovery is incomplete'
legacy_id=$(codex_as_home_id legacy-account)
rotating_id=$(codex_as_home_id rotating-account)
standby_id=$(codex_as_home_id standby-account)
[[ $legacy_id =~ ^[0-9a-f]{16}$ && $rotating_id =~ ^[0-9a-f]{16}$ \
    && $standby_id =~ ^[0-9a-f]{16}$ ]] \
  || fail 'home ids are not opaque fixed-width digests'
[[ $legacy_id != "$rotating_id" && $legacy_id != "$standby_id" \
    && $rotating_id != "$standby_id" ]] || fail 'distinct homes received one opaque id'
[[ $(codex_as_socket "$rotating_id") == "$CODEX_AS_RUNTIME_DIR/$rotating_id.sock" ]] \
  || fail 'app-server socket is not mapped to the short opaque runtime path'
production_socket=/run/apitoken/codex-app-servers/$rotating_id.sock
(( ${#production_socket} < 80 )) \
  || fail 'app-server socket path lost its cross-platform length bound'

seeds=$(codex_as_select_transition_seeds "$records" 2) \
  || fail 'a redundant discovered-only transition cohort was not selected'
[[ $(grep -c . <<<"$seeds") == 2 && $seeds != *"$legacy_id"* \
    && $seeds == *"$rotating_id"* && $seeds == *"$standby_id"* ]] \
  || fail 'the transition cohort was not exactly the two discovered-only homes'
if codex_as_select_transition_seeds "$legacy_id"$'\t''legacy-account' 2 >/dev/null; then
  fail 'transition cohort selection accepted only the legacy explicit home'
fi

proxy_secret='http://fixture-user:fixture-pass@127.0.0.1:18080'
proxy_home=$CODEX_AS_HOMES_DIR/rotating-account
printf '%s\n' "$proxy_secret" >"$proxy_home/proxy.url"
chmod 0600 "$proxy_home/proxy.url"
owner=$(codex_as_expected_owner)
read -r expected_uid expected_gid <<<"$owner"
had_proxy=$(codex_as_install_transition_marker "$proxy_home" "$expected_uid" "$expected_gid")
[[ $had_proxy == 1 ]] || fail 'transition did not preserve the existing proxy'
[[ $(<"$proxy_home/proxy.url") == "$CODEX_AS_TRANSITION_SENTINEL" ]] \
  || fail 'legacy gateway exclusion marker is missing'
[[ $(codex_as_proxy_value "$proxy_home") == "$proxy_secret" ]] \
  || fail 'daemon did not resolve the preserved transition proxy'
proxy_digest=$(codex_as_proxy_digest "$proxy_secret")
[[ $proxy_digest =~ ^[0-9a-f]{64}$ && $proxy_digest != *fixture* ]] \
  || fail 'runtime proxy state is not an opaque digest'
codex_as_restore_transition_marker "$proxy_home" "$had_proxy"
[[ $(<"$proxy_home/proxy.url") == "$proxy_secret" ]] \
  || fail 'transition commit did not atomically restore the proxy'
[[ ! -e $proxy_home/$CODEX_AS_TRANSITION_PROXY_FILE ]] \
  || fail 'transition proxy copy survived commit'

plain_home=$CODEX_AS_HOMES_DIR/legacy-account
had_proxy=$(codex_as_install_transition_marker "$plain_home" "$expected_uid" "$expected_gid")
[[ $had_proxy == 0 && -z $(codex_as_proxy_value "$plain_home") ]] \
  || fail 'no-proxy transition changed effective egress'
codex_as_restore_transition_marker "$plain_home" "$had_proxy"
[[ ! -e $plain_home/proxy.url ]] || fail 'no-proxy transition left a marker behind'

printf 'ftp://unsupported.example\n' >"$plain_home/proxy.url"
chmod 0600 "$plain_home/proxy.url"
if codex_as_proxy_value "$plain_home" >/dev/null 2>&1; then
  fail 'unsupported daemon proxy scheme was accepted'
fi
printf 'http://first.example\nhttp://second.example\n' >"$plain_home/proxy.url"
chmod 0600 "$plain_home/proxy.url"
if codex_as_proxy_value "$plain_home" >/dev/null 2>&1; then
  fail 'multi-line daemon proxy metadata was accepted'
fi
rm -f -- "$plain_home/proxy.url"

fake_binary=$TEMP/codex
printf '%s\n' '#!/bin/sh' 'printf "codex-cli 0.145.0\\n"' >"$fake_binary"
chmod 0700 "$fake_binary"
fake_digest=$(codex_as_sha256_file "$fake_binary")
{
  printf 'UNRELATED_SECRET=must-not-be-sourced\n'
  printf 'CLAUDE_API_CODEX_BIN=%s\n' "$fake_binary"
  printf 'CLAUDE_API_CODEX_BIN_SHA256=%s\n' "$fake_digest"
  printf 'CLAUDE_API_CODEX_VERSION=codex-cli 0.145.0\n'
} >"$CODEX_AS_CONFIG_ENV"
chmod 0600 "$CODEX_AS_CONFIG_ENV"
codex_as_load_desired || fail 'attested Codex config was rejected'
[[ $CODEX_AS_BINARY == "$fake_binary" && $CODEX_AS_BINARY_SHA256 == "$fake_digest" ]] \
  || fail 'attested Codex config was parsed incorrectly'
serve_body=$(sed -n '/^codex_as_serve()/,/^}/p' "$ROOT/deploy/codex-app-servers.sh")
! grep -Fq 'rm -f -- "$socket"' <<<"$serve_body" \
  || fail 'app-server supervisor can unlink a live official control socket'

# A spawned websocket bridge is not enough to roll the next daemon. The gateway publishes this lease
# only after the official websocket initialized and account/read authenticated; PID + per-process
# nonce validation prevents both stale markers and PID reuse from manufacturing readiness.
(
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  CODEX_AS_PROC_ROOT=$TEMP/ready-proc
  fixture_gateway_pid=111
  fixture_proxy_pid=222
  fixture_lease=0123456789abcdef0123456789abcdef
  mkdir -p "$CODEX_AS_PROC_ROOT/$fixture_gateway_pid" "$CODEX_AS_PROC_ROOT/$fixture_proxy_pid"
  printf 'PPid:\t%s\n' "$fixture_gateway_pid" \
    >"$CODEX_AS_PROC_ROOT/$fixture_proxy_pid/status"
  printf 'CODEX_HOME=%s\0CLAUDE_API_CODEX_CLIENT_LEASE=%s\0' \
    "$proxy_home" "$fixture_lease" >"$CODEX_AS_PROC_ROOT/$fixture_proxy_pid/environ"
  printf '/fixture/codex\0app-server\0proxy\0--sock\0fixture.sock\0' \
    >"$CODEX_AS_PROC_ROOT/$fixture_proxy_pid/cmdline"
  codex_as_gateway_main_pids() { printf '%s\n' "$fixture_gateway_pid"; }
  marker=$CODEX_AS_RUNTIME_DIR/$rotating_id.client.$fixture_gateway_pid.$fixture_proxy_pid.$fixture_lease.ready
  printf '%s\n' "$CODEX_AS_CLIENT_MARKER" >"$marker"
  chmod 0600 "$marker"
  [[ $(codex_as_home_ready_client_count "$proxy_home") == 1 ]] \
    || fail 'authenticated websocket client lease was not counted'
  printf 'CODEX_HOME=%s\0CLAUDE_API_CODEX_CLIENT_LEASE=%032d\0' "$proxy_home" 0 \
    >"$CODEX_AS_PROC_ROOT/$fixture_proxy_pid/environ"
  [[ $(codex_as_home_ready_client_count "$proxy_home") == 0 ]] \
    || fail 'stale client lease was accepted for a new proxy process'
  printf 'CODEX_HOME=%s\0CLAUDE_API_CODEX_CLIENT_LEASE=%s\0' \
    "$proxy_home" "$fixture_lease" >"$CODEX_AS_PROC_ROOT/$fixture_proxy_pid/environ"
  chmod 0644 "$marker"
  if codex_as_home_ready_client_count "$proxy_home" >/dev/null 2>&1; then
    fail 'unsafe client readiness marker was accepted'
  fi
  chmod 0600 "$marker"
  rm -f -- "$marker"
)

# A desired-version change must roll while a peer that still serves the previous pin remains up.
# Treating that peer as unconverged is correct, but treating it as unavailable would deadlock the
# first restart and make a pin update impossible.
roll_log=$TEMP/roll.log
drain_active=0
codex_as_unit_healthy() { return 1; }
codex_as_unit_active() { return 0; }
codex_as_find_home() { printf '%s\n' "$proxy_home"; }
codex_as_other_serving_count() { printf '1\n'; }
codex_as_begin_drain() { drain_active=1; printf 'begin\n' >>"$roll_log"; }
codex_as_wait_proxy_drain() { printf 'drained\n' >>"$roll_log"; }
codex_as_wait_healthy() { printf 'healthy\n' >>"$roll_log"; }
codex_as_home_draining() { (( drain_active == 1 )); }
codex_as_end_drain() { drain_active=0; printf 'returned\n' >>"$roll_log"; }
fake_systemctl=$TEMP/systemctl
printf '%s\n' '#!/bin/sh' 'printf "%s\n" "$1" >>"$CODEX_AS_TEST_ROLL_LOG"' >"$fake_systemctl"
chmod 0700 "$fake_systemctl"
CODEX_AS_SYSTEMCTL=$fake_systemctl
export CODEX_AS_TEST_ROLL_LOG=$roll_log
codex_as_start_or_roll "$rotating_id" || fail 'old-but-serving peer did not permit a rolling update'
[[ $(tr '\n' ' ' <"$roll_log") == 'begin drained restart healthy returned ' ]] \
  || fail 'rolling update did not drain, restart, verify, and return the home in order'

for reconciler_state in static disabled indirect; do
  codex_as_unit_file_state_reconciler_owned "$reconciler_state" \
    || fail "safe reconciler-owned UnitFileState was rejected: $reconciler_state"
done
for boot_state in enabled enabled-runtime linked linked-runtime alias generated masked ''; do
  if codex_as_unit_file_state_reconciler_owned "$boot_state"; then
    fail "unsafe or unknown UnitFileState was accepted: ${boot_state:-empty}"
  fi
done

# SIGUSR2 belongs exclusively to the Rust gateway handler. `systemctl kill` targets the entire
# cgroup unless `--kill-whom=main` is explicit, which would terminate every authenticated Codex
# proxy child at once and reproduce the production outage.
(
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  signal_log=$TEMP/gateway-signal.log
  signal_systemctl=$TEMP/gateway-signal-systemctl
  printf '%s\n' \
    '#!/bin/sh' \
    'case "$1" in' \
    '  is-active) exit 0 ;;' \
    '  kill) printf "%s\n" "$*" >> "$CODEX_AS_TEST_SIGNAL_LOG"; exit 0 ;;' \
    '  *) exit 1 ;;' \
    'esac' >"$signal_systemctl"
  chmod 0700 "$signal_systemctl"
  export CODEX_AS_TEST_SIGNAL_LOG=$signal_log
  CODEX_AS_SYSTEMCTL=$signal_systemctl
  CODEX_AS_GATEWAY_UNITS=claude-api-openai@8793.service
  codex_as_signal_gateways || fail 'gateway reconcile signal failed'
  [[ $(<"$signal_log") == \
      'kill --kill-whom=main --signal=SIGUSR2 claude-api-openai@8793.service' ]] \
    || fail 'gateway reconcile signal escaped MainPID and could kill proxy children'
)

# Cutover admission is a read-only assertion, not a repair mechanism. Starting the candidate has
# already created its clients; the deployment gate may only observe that the redundant cohort stays
# attached. The same lifecycle boundary must reject an incomplete desired snapshot before invoking
# any start/roll/retire mutation.
(
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  observer_systemctl=$TEMP/observer-systemctl
  printf '%s\n' '#!/bin/sh' 'exit 1' >"$observer_systemctl"
  chmod 0700 "$observer_systemctl"
  CODEX_AS_SYSTEMCTL=$observer_systemctl
  CODEX_AS_CUTOVER_STABILITY_SECONDS=3
  codex_as_is_root() { return 0; }
  codex_as_load_desired() { return 0; }
  codex_as_gateway_active_count() { printf '2\n'; }
  codex_as_signal_gateways() { fail 'cutover admission mutated live gateway clients'; }
  codex_as_wait_ready_cohort() {
    [[ $1 == 3 ]] || fail 'cutover admission ignored its stability window'
    printf '2\n'
  }
  codex_as_admit_cutover || fail 'observational cutover admission rejected a stable cohort'

  CODEX_AS_MIN_READY=2
  codex_as_home_records() { printf '1111111111111111\tonly-home\n'; }
  codex_as_start_or_roll() { fail 'incomplete desired snapshot mutated the serving cohort'; }
  if codex_as_reconcile >/dev/null 2>&1; then
    fail 'reconciliation accepted a desired snapshot below the redundancy floor'
  fi
)

# One expired/disconnected account is excluded instead of failing an otherwise redundant cohort.
# The same fixture must still fail closed when fewer than two authenticated homes remain.
(
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  CODEX_AS_READY_TIMEOUT=3
  CODEX_AS_MIN_READY=2
  codex_as_gateway_active_count() { printf '1\n'; }
  codex_as_home_records() {
    printf '1111111111111111\tready-one\n'
    printf '2222222222222222\tready-two\n'
    printf '3333333333333333\texpired\n'
  }
  codex_as_home_draining() { return 1; }
  codex_as_unit_healthy() { return 0; }
  codex_as_home_ready_client_count() {
    [[ $1 == */expired ]] && printf '0\n' || printf '1\n'
  }
  [[ $(codex_as_wait_ready_cohort) == 2 ]] \
    || fail 'one expired account rejected a redundant authenticated cohort'
  CODEX_AS_MIN_READY=3
  if codex_as_wait_ready_cohort >/dev/null; then
    fail 'cohort readiness passed without the required authenticated redundancy'
  fi

  # A transient dip resets the entire cutover streak; two good samples before the dip cannot be
  # combined with later samples to manufacture a stable admission window.
  CODEX_AS_MIN_READY=2
  CODEX_AS_READY_TIMEOUT=8
  stability_calls=$TEMP/stability-calls
  : >"$stability_calls"
  codex_as_authenticated_home_count() {
    local call
    call=$(wc -l <"$stability_calls" | tr -d '[:space:]')
    printf 'x\n' >>"$stability_calls"
    case "$call" in
      2) printf '1\n' ;;
      *) printf '2\n' ;;
    esac
  }
  [[ $(codex_as_wait_ready_cohort 3) == 2 ]] \
    || fail 'stable cohort did not recover after a transient dip'
  (( $(wc -l <"$stability_calls") >= 6 )) \
    || fail 'cutover stability streak did not reset after losing redundancy'
)

# Exercise the first singleton-to-daemon handoff as an availability state machine. The fixture
# systemctl checks the invariant after every start/stop/restart: either the singleton is active, or
# an active shared gateway has both transition daemons. A future reordering that removes the legacy
# anchor before redundant ownership exists therefore fails this test rather than reaching production.
(
  # Restore the real helper functions after the focused rolling-update stubs above.
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  codex_as_is_root() { return 0; }
  codex_as_secure_control_dir() { mkdir -p "$CODEX_AS_CONTROL_DIR"; chmod 0750 "$CODEX_AS_CONTROL_DIR"; }
  codex_as_load_desired() { return 0; }

  systemd_state=$TEMP/forward-systemd
  transition_log=$TEMP/forward-transition.log
  mkdir -p "$systemd_state"
  : >"$transition_log"
  export CODEX_AS_TEST_SYSTEMD_STATE=$systemd_state
  export CODEX_AS_TEST_TRANSITION_LOG=$transition_log
  forward_systemctl=$TEMP/forward-systemctl
  printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'state=$CODEX_AS_TEST_SYSTEMD_STATE' \
    'log=$CODEX_AS_TEST_TRANSITION_LOG' \
    'command=$1; shift' \
    'case "$command" in' \
    '  is-active) [ "${1:-}" = --quiet ] && shift; [ -f "$state/active.$1" ]; exit ;;' \
    '  is-enabled) [ "${1:-}" = --quiet ] && shift; [ -f "$state/enabled.$1" ]; exit ;;' \
    '  start|restart) touch "$state/active.$1"; printf "%s %s\n" "$command" "$1" >>"$log" ;;' \
    '  stop) rm -f -- "$state/active.$1"; printf "stop %s\n" "$1" >>"$log" ;;' \
    '  disable) rm -f -- "$state/enabled.$1"; printf "disable %s\n" "$1" >>"$log" ;;' \
    '  enable) touch "$state/enabled.$1"; printf "enable %s\n" "$1" >>"$log" ;;' \
    '  kill) printf "kill %s\n" "$*" >>"$log"; exit ;;' \
    '  list-units) for item in "$state"/known.*; do [ -e "$item" ] || continue; unit=${item##*/known.}; printf "%s loaded inactive dead fixture\n" "$unit"; done; exit ;;' \
    '  *) exit 1 ;;' \
    'esac' \
    'if [ -f "$state/active.$CODEX_AS_TEST_LEGACY_UNIT" ]; then exit 0; fi' \
    'if [ -f "$state/active.$CODEX_AS_TEST_SHARED_UNIT" ] &&' \
    '   [ -f "$state/active.$CODEX_AS_TEST_ROTATING_DAEMON" ] &&' \
    '   [ -f "$state/active.$CODEX_AS_TEST_STANDBY_DAEMON" ]; then exit 0; fi' \
    'printf "availability invariant failed after %s\n" "$command" >&2' \
    'exit 42' >"$forward_systemctl"
  chmod 0700 "$forward_systemctl"
  CODEX_AS_SYSTEMCTL=$forward_systemctl

  legacy_unit=$CODEX_AS_LEGACY_UNIT
  shared_unit=$CODEX_AS_ROLLBACK_GATEWAY_UNIT
  legacy_daemon=$(codex_as_unit "$legacy_id")
  rotating_daemon=$(codex_as_unit "$rotating_id")
  standby_daemon=$(codex_as_unit "$standby_id")
  export CODEX_AS_TEST_LEGACY_UNIT=$legacy_unit
  export CODEX_AS_TEST_SHARED_UNIT=$shared_unit
  export CODEX_AS_TEST_LEGACY_DAEMON=$legacy_daemon
  export CODEX_AS_TEST_ROTATING_DAEMON=$rotating_daemon
  export CODEX_AS_TEST_STANDBY_DAEMON=$standby_daemon
  touch "$systemd_state/known.$legacy_daemon" "$systemd_state/known.$rotating_daemon" \
    "$systemd_state/known.$standby_daemon"
  touch "$systemd_state/active.$legacy_unit"

  codex_as_home_has_process() {
    [[ $1 == "$CODEX_AS_HOMES_DIR/$CODEX_AS_LEGACY_EXPLICIT_HOME_NAME" ]]
  }
  codex_as_unit_healthy() { codex_as_unit_active "$1"; }
  codex_as_wait_healthy() { codex_as_unit_active "$1"; }
  codex_as_signal_gateways() { return 0; }
  codex_as_wait_proxy_restore() { return 0; }
  codex_as_wait_ready_cohort() { printf '2\n'; }
  codex_as_unit_reconciler_owned() { return 0; }

  codex_as_prepare_transition || fail 'legacy-to-shared ownership preparation failed'
  [[ -f "$systemd_state/active.$legacy_unit" \
      && ! -f "$systemd_state/active.$legacy_daemon" \
      && -f "$systemd_state/active.$rotating_daemon" \
      && -f "$systemd_state/active.$standby_daemon" ]] \
    || fail 'redundant transition cohort was not admitted beside the singleton'
  transition_ids=$(codex_as_transition_home_ids | sort | tr '\n' ' ')
  expected_transition_ids=$(printf '%s\n%s\n' "$rotating_id" "$standby_id" | sort | tr '\n' ' ')
  [[ $transition_ids == "$expected_transition_ids" ]] \
    || fail 'persisted transition state did not contain both daemon homes'
  touch "$systemd_state/active.$shared_unit"
  "$forward_systemctl" stop "$legacy_unit" \
    || fail 'stopping the singleton created an ownerless OpenAI state'
  codex_as_commit_transition || fail 'legacy-to-shared ownership transition did not commit'
  codex_as_verify || fail 'committed shared ownership did not verify'
  [[ ! -e $CODEX_AS_TRANSITION_FILE \
      && -f "$systemd_state/active.$shared_unit" \
      && -f "$systemd_state/active.$legacy_daemon" \
      && -f "$systemd_state/active.$rotating_daemon" \
      && -f "$systemd_state/active.$standby_daemon" ]] \
    || fail 'shared transition did not leave one gateway and the complete daemon cohort'
  codex_as_signal_gateways() { fail 'steady-state reconciliation perturbed live gateway clients'; }
  codex_as_authenticated_home_count() { printf '2\n'; }
  codex_as_reconcile \
    || fail 'steady-state reconciliation could not verify the existing daemon cohort'
)

# Exercise the one compatibility rollback that crosses from shared-daemon releases to the legacy
# singleton. This is a state-machine test: the old and new generations must own disjoint homes while
# both HTTP origins overlap, commit must stop every daemon, and abort must stop the singleton before
# returning its seed to the shared generation.
(
  # Restore the real helper functions after the focused rolling-update stubs above.
  # shellcheck source=deploy/codex-app-servers.sh
  source "$ROOT/deploy/codex-app-servers.sh"
  codex_as_is_root() { return 0; }
  codex_as_secure_control_dir() { mkdir -p "$CODEX_AS_CONTROL_DIR"; chmod 0750 "$CODEX_AS_CONTROL_DIR"; }
  codex_as_load_desired() { return 0; }

  systemd_state=$TEMP/reverse-systemd
  transition_log=$TEMP/reverse-transition.log
  mkdir -p "$systemd_state"
  : >"$transition_log"
  export CODEX_AS_TEST_SYSTEMD_STATE=$systemd_state
  export CODEX_AS_TEST_TRANSITION_LOG=$transition_log
  reverse_systemctl=$TEMP/reverse-systemctl
  printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'state=$CODEX_AS_TEST_SYSTEMD_STATE' \
    'log=$CODEX_AS_TEST_TRANSITION_LOG' \
    'command=$1; shift' \
    'case "$command" in' \
    '  is-active) [ "${1:-}" = --quiet ] && shift; [ -f "$state/active.$1" ] ;;' \
    '  is-enabled) [ "${1:-}" = --quiet ] && shift; [ -f "$state/enabled.$1" ] ;;' \
    '  start|restart) touch "$state/active.$1"; printf "%s %s\n" "$command" "$1" >>"$log" ;;' \
    '  stop) rm -f -- "$state/active.$1"; printf "stop %s\n" "$1" >>"$log" ;;' \
    '  disable) rm -f -- "$state/enabled.$1"; printf "disable %s\n" "$1" >>"$log" ;;' \
    '  kill) printf "kill %s\n" "$*" >>"$log" ;;' \
    '  list-units) for path in "$state"/known.*; do [ -e "$path" ] || continue; unit=${path##*/known.}; printf "%s loaded inactive dead fixture\n" "$unit"; done ;;' \
    '  *) exit 1 ;;' \
    'esac' \
    'case "$command" in is-active|is-enabled|list-units|kill) exit ;; esac' \
    'if [ -f "$state/active.$CODEX_AS_TEST_LEGACY_UNIT" ]; then exit 0; fi' \
    'if [ -f "$state/active.$CODEX_AS_TEST_SHARED_UNIT" ] &&' \
    '   [ -f "$state/active.$CODEX_AS_TEST_ROTATING_DAEMON" ] &&' \
    '   [ -f "$state/active.$CODEX_AS_TEST_STANDBY_DAEMON" ]; then exit 0; fi' \
    'printf "availability invariant failed after %s\n" "$command" >&2' \
    'exit 42' >"$reverse_systemctl"
  chmod 0700 "$reverse_systemctl"
  CODEX_AS_SYSTEMCTL=$reverse_systemctl

  legacy_unit=$CODEX_AS_LEGACY_UNIT
  rollback_gateway=$CODEX_AS_ROLLBACK_GATEWAY_UNIT
  legacy_daemon=$(codex_as_unit "$legacy_id")
  rotating_daemon=$(codex_as_unit "$rotating_id")
  standby_daemon=$(codex_as_unit "$standby_id")
  export CODEX_AS_TEST_LEGACY_UNIT=$legacy_unit
  export CODEX_AS_TEST_SHARED_UNIT=$rollback_gateway
  export CODEX_AS_TEST_LEGACY_DAEMON=$legacy_daemon
  export CODEX_AS_TEST_ROTATING_DAEMON=$rotating_daemon
  export CODEX_AS_TEST_STANDBY_DAEMON=$standby_daemon
  touch "$systemd_state/known.$legacy_daemon" "$systemd_state/known.$rotating_daemon" \
    "$systemd_state/known.$standby_daemon"
  touch "$systemd_state/active.$legacy_daemon" "$systemd_state/active.$rotating_daemon" \
    "$systemd_state/active.$standby_daemon"
  touch "$systemd_state/active.$rollback_gateway"

  codex_as_unit_healthy() { codex_as_unit_active "$1"; }
  codex_as_other_serving_count() { printf '1\n'; }
  codex_as_wait_proxy_drain() { printf 'proxy-drained\n' >>"$transition_log"; }
  codex_as_wait_proxy_restore() { return 0; }
  codex_as_wait_ready_cohort() { printf '2\n'; }
  codex_as_unit_reconciler_owned() { return 0; }
  codex_as_home_proxy_count() { printf '0\n'; }
  codex_as_home_has_process() {
    local home=$1
    [[ $home == "$CODEX_AS_HOMES_DIR/$CODEX_AS_LEGACY_EXPLICIT_HOME_NAME" ]] || return 1
    [[ -f "$systemd_state/active.$legacy_unit" || -f "$systemd_state/active.$legacy_daemon" ]]
  }
  codex_as_start_or_roll() {
    local instance=$1 home
    home=$(codex_as_find_home "$instance") || return 1
    touch "$systemd_state/active.$(codex_as_unit "$instance")"
    if codex_as_home_draining "$home"; then rm -f -- "$home/$CODEX_AS_DRAIN_FILE"; fi
    printf 'start-seed %s\n' "$instance" >>"$transition_log"
  }

  codex_as_prepare_legacy_transition \
    || fail 'shared-to-legacy ownership preparation failed'
  [[ $(codex_as_transition_value mode) == "$CODEX_AS_TRANSITION_TO_LEGACY" ]] \
    || fail 'legacy transition mode was not persisted'
  [[ ! -f "$systemd_state/active.$legacy_daemon" ]] \
    || fail 'legacy seed daemon survived ownership preparation'
  [[ -f "$plain_home/$CODEX_AS_DRAIN_FILE" ]] \
    || fail 'legacy seed was not fenced from shared gateways'
  [[ $(<"$proxy_home/proxy.url") == "$CODEX_AS_TRANSITION_SENTINEL" ]] \
    || fail 'discovered peer was not fenced from the old singleton scan'
  [[ $(<"$proxy_home/$CODEX_AS_TRANSITION_PROXY_FILE") == "$proxy_secret" ]] \
    || fail 'discovered peer proxy was not preserved during rollback overlap'

  touch "$systemd_state/active.$legacy_unit"
  codex_as_prepare_legacy_transition \
    || fail 'legacy transition preparation was not idempotent after singleton admission'
  rm -f -- "$systemd_state/active.$rollback_gateway"
  codex_as_commit_legacy_transition \
    || fail 'legacy ownership transition did not commit after the shared slot stopped'
  codex_as_verify_legacy || fail 'committed legacy ownership did not verify'
  [[ ! -e $CODEX_AS_TRANSITION_FILE && ! -e $plain_home/$CODEX_AS_DRAIN_FILE ]] \
    || fail 'legacy transition state survived commit'
  [[ $(<"$proxy_home/proxy.url") == "$proxy_secret" \
      && ! -e $proxy_home/$CODEX_AS_TRANSITION_PROXY_FILE ]] \
    || fail 'legacy commit did not restore discovered-home proxy metadata'
  [[ ! -f "$systemd_state/active.$legacy_daemon" \
      && ! -f "$systemd_state/active.$rotating_daemon" \
      && ! -f "$systemd_state/active.$standby_daemon" ]] \
    || fail 'a daemon owner survived beside the committed singleton'

  # Build the same overlap again, then prove abort order and restoration.
  rm -f -- "$systemd_state/active.$legacy_unit"
  touch "$systemd_state/active.$legacy_daemon" "$systemd_state/active.$rotating_daemon" \
    "$systemd_state/active.$standby_daemon"
  touch "$systemd_state/active.$rollback_gateway"
  codex_as_prepare_legacy_transition || fail 'second legacy preparation failed'
  touch "$systemd_state/active.$legacy_unit"
  printf 'abort-begin\n' >>"$transition_log"
  codex_as_abort_legacy_transition || fail 'legacy ownership abort failed'
  abort_log=$(sed -n '/^abort-begin$/,$p' "$transition_log" | tr '\n' ' ')
  [[ $abort_log == *"stop $legacy_unit start-seed $legacy_id"* ]] \
    || fail 'abort did not stop the singleton before restarting its seed daemon'
  [[ ! -f "$systemd_state/active.$legacy_unit" \
      && -f "$systemd_state/active.$legacy_daemon" \
      && -f "$systemd_state/active.$rotating_daemon" \
      && -f "$systemd_state/active.$standby_daemon" ]] \
    || fail 'abort did not restore the shared daemon cohort exclusively'
  codex_as_verify || fail 'shared daemon ownership did not verify after abort'
)

printf '[codex-app-servers-test] OK\n'
