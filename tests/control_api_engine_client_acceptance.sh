#!/usr/bin/env bash
# Keyless assembled acceptance: built claude-api + built EngineClient + disposable PostgreSQL.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
BIN=${CLAUDE_API_BIN:-$ROOT/target/release/claude-api}
DATABASE_URL=${CLAUDE_API_TEST_DATABASE_URL:-}
PORT=${CONTROL_API_ACCEPTANCE_PORT:-17480}
CONTROL_KEY=control-api-acceptance-key-0000000000000000
TMP=$(mktemp -d)
PID=

fail() { printf 'control-api-acceptance: %s\n' "$*" >&2; exit 1; }
cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  if [[ -n ${PID:-} ]] && kill -0 "$PID" 2>/dev/null; then
    kill -TERM "$PID" 2>/dev/null || true
    for _ in $(seq 1 50); do
      kill -0 "$PID" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$PID" 2>/dev/null; then kill -KILL "$PID" 2>/dev/null || true; fi
    wait "$PID" 2>/dev/null || true
  fi
  rm -rf -- "$TMP"
  exit "$rc"
}
trap cleanup EXIT INT TERM
umask 077

[[ -x $BIN ]] || fail "built claude-api binary is required: $BIN"
[[ -n $DATABASE_URL ]] || fail 'CLAUDE_API_TEST_DATABASE_URL is required'
[[ $PORT =~ ^[0-9]+$ ]] || fail 'CONTROL_API_ACCEPTANCE_PORT must be numeric'
(( PORT >= 1024 && PORT < 32768 )) || fail 'acceptance port must be unprivileged and below the ephemeral range'
command -v curl >/dev/null || fail 'curl is required'
command -v node >/dev/null || fail 'node is required'

# Reserve-grade fail-closed port preflight. The server performs the authoritative bind immediately after.
if (exec 9<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
  exec 9>&-
  fail "acceptance port is already in use: $PORT"
fi

install -d -m 0700 "$TMP/home" "$TMP/config" "$TMP/spool"
SERVER_LOG=$TMP/server.log

COMMON_ENV=(
  HOME="$TMP/home"
  SUB_CFG_DIR="$TMP/config"
  SUBS_DB="$TMP/config/subscriptions.db"
  SUBS_FLEET=acceptance
  CLAUDE_API_DATABASE_URL="$DATABASE_URL"
  CLAUDE_API_PROVIDER=anthropic
  CLAUDE_API_GEMINI_ENABLED=0
  CLAUDE_API_CODEX_ENABLED=0
  CLAUDE_API_KIMI_ENABLED=0
  CLAUDE_API_GLM_ENABLED=0
  CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0
  CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0
  CLAUDE_API_UPSTREAM=http://127.0.0.1:1
  CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1
  CLAUDE_API_HOST=127.0.0.1
  CLAUDE_API_PORT="$PORT"
  CLAUDE_API_BODY_SPOOL_ROOT="$TMP/spool"
  CLAUDE_API_INSTANCE_ID="control-api-acceptance-$PPID-$$"
  CLAUDE_API_CONTROL_KEY="$CONTROL_KEY"
  CLAUDE_API_TRUST_LOOPBACK=0
  CLAUDE_API_BILLING=1
  CLAUDE_API_BILLING_READERS=1
  CLAUDE_API_POLL=0
  CLAUDE_API_TARIFF_OVERRIDES=0
  CLAUDE_API_DRAIN_DEADLINE_SECS=5
)

env "${COMMON_ENV[@]}" "$BIN" db migrate-engine >"$TMP/migrate.log" 2>&1
env "${COMMON_ENV[@]}" "$BIN" serve >"$SERVER_LOG" 2>&1 &
PID=$!

ready=0
for _ in $(seq 1 100); do
  if ! kill -0 "$PID" 2>/dev/null; then
    tail -n 100 "$SERVER_LOG" >&2 || true
    fail 'built server exited before readiness'
  fi
  if [[ $(curl --silent --output /dev/null --write-out '%{http_code}' \
      "http://127.0.0.1:$PORT/ready" 2>/dev/null || true) == 200 ]]; then
    ready=1
    break
  fi
  sleep 0.1
done
if (( ready == 0 )); then
  tail -n 100 "$SERVER_LOG" >&2 || true
  fail 'built server did not become ready within 10 seconds'
fi

CONTROL_API_ACCEPTANCE_BASE_URL="http://127.0.0.1:$PORT" \
CONTROL_API_ACCEPTANCE_CONTROL_KEY="$CONTROL_KEY" \
CONTROL_API_ACCEPTANCE_SERVER_LOG="$SERVER_LOG" \
  node "$ROOT/packages/engine-client/acceptance/control-api.mjs"
