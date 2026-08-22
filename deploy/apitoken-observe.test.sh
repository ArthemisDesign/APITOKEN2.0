#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
WRAPPER=$ROOT/deploy/apitoken-observe.sh

fail() { printf 'apitoken-observe.test: %s\n' "$*" >&2; exit 1; }

expect_denied() {
  local cmd=$1 output status=0
  output=$(SSH_ORIGINAL_COMMAND="$cmd" bash "$WRAPPER" 2>&1) || status=$?
  (( status == 2 )) || fail "expected denial for '$cmd', got $status: $output"
  grep -Fq 'denied' <<<"$output" || grep -Fq 'not permitted' <<<"$output" \
    || grep -Fq 'invalid' <<<"$output" \
    || grep -Fq 'usage:' <<<"$output" \
    || grep -Fq 'unexpected arguments' <<<"$output" \
    || fail "denial for '$cmd' lacked a reason: $output"
}

expect_help() {
  local output
  output=$(SSH_ORIGINAL_COMMAND=help bash "$WRAPPER")
  grep -Fq 'log-only host session' <<<"$output" || fail "help missing banner: $output"
  grep -Fq 'denied: shell, sudo' <<<"$output" || fail "help missing deny list: $output"
}

[[ -f $WRAPPER && ! -L $WRAPPER ]] || fail 'wrapper source is missing'
bash -n "$WRAPPER"
bash -n "$ROOT/deploy/install-observe.sh"
grep -Fq 'Never exec a user shell' "$WRAPPER" || fail 'wrapper lost its no-shell contract'
grep -Fq 'Never call sudo' "$WRAPPER" || fail 'wrapper lost its no-sudo contract'
! grep -Eq '"\$SYSTEMCTL" (start|stop|restart|kill)' "$WRAPPER" \
  || fail 'wrapper must not invoke mutating systemctl verbs'
if grep -Eq '^[[:space:]]*sudo |sudo -n |exec sudo' "$WRAPPER"; then
  fail 'wrapper must not call sudo'
fi

expect_help
expect_denied 'sudo systemctl restart claude-api-anthropic@8787.service'
expect_denied '/bin/bash'
expect_denied 'bash -l'
expect_denied 'systemctl restart caddy'
expect_denied 'apitoken-watchdog retry abc'
expect_denied 'logs apitoken-postgres.service; reboot'
expect_denied 'logs ../../etc/passwd'
expect_denied 'logs claude-api-anthropic@8787.service --since bad;id'
expect_denied 'logs sshd.service'
expect_denied 'logs apitoken-postgres.service'
expect_denied 'status extra'

# Permitted unit names must parse. journalctl is absent on macOS developer hosts (127);
# on Linux it may exit 1 when the unit has no journal.
status=0
SSH_ORIGINAL_COMMAND='logs claude-api-anthropic@8787.service' bash "$WRAPPER" >/dev/null 2>&1 \
  || status=$?
(( status == 0 || status == 1 || status == 127 )) \
  || fail "permitted logs command failed unexpectedly with $status"

printf 'apitoken-observe.test: passed\n'
