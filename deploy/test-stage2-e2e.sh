#!/usr/bin/env bash
set -euo pipefail

# Destructive only to a uniquely named temporary database/role and /tmp directory. The live
# claude_engine database and systemd service are never referenced by this test.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }
[[ $# -eq 1 && -x $1 ]] || { echo "usage: $0 /path/to/claude-api" >&2; exit 2; }

BIN=$(readlink -f "$1")
ROOT=/tmp/stage2-engine-e2e-$$
NAME=stage2_e2e_$$
CONTAINER=${ENGINE_POSTGRES_CONTAINER:-deploy-commerce-postgres-1}
BASE_PORT=$((20000 + ($$ % 10000)))
ENGINE_A_PORT=$BASE_PORT
ENGINE_B_PORT=$((BASE_PORT + 1))
SQLITE_PORT=$((BASE_PORT + 2))
ENGINE_LOCK_PORT=$((BASE_PORT + 3))
UPSTREAM_PORT=$((BASE_PORT + 4))
PIDS=()

cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  for pid in "${PIDS[@]:-}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    wait "$pid" 2>/dev/null || true
  done
  docker exec "$CONTAINER" psql -U commerce -d postgres -v ON_ERROR_STOP=1 \
    -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname='$NAME'" \
    -c "DROP DATABASE IF EXISTS $NAME" \
    -c "DROP ROLE IF EXISTS $NAME" >/dev/null 2>&1 || true
  rm -rf "$ROOT"
  exit "$rc"
}
trap cleanup EXIT INT TERM

install -d -m 0700 "$ROOT"
ENGINE_POSTGRES_DB=$NAME \
ENGINE_POSTGRES_ROLE=$NAME \
ENGINE_DATA_DIR=$ROOT/pg \
  "$(dirname "$0")/engine-postgres-provision.sh"

set -a
# shellcheck disable=SC1090
source "$ROOT/pg/engine-postgres.pending"
set +a

install -m 0600 /dev/null "$ROOT/oauth-token"
install -m 0600 /dev/null "$ROOT/customer-key"
printf '%s\n' 'sk-ant-oat01-stage2-test-only' >"$ROOT/oauth-token"
printf '%s\n' 'sk-pool-stage2-e2e-customer-key' >"$ROOT/customer-key"

# Seed the legacy authority first, then exercise the exact SQLite -> PostgreSQL importer.
env -u CLAUDE_API_DATABASE_URL SUBS_DB="$ROOT/source.db" SUB_CFG_DIR="$ROOT" \
  "$BIN" sub add-file stage2@example.test --token-file "$ROOT/oauth-token" --fleet prod
env -u CLAUDE_API_DATABASE_URL SUBS_DB="$ROOT/source.db" SUB_CFG_DIR="$ROOT" \
  "$BIN" sub set-plan stage2@example.test max20
env -u CLAUDE_API_DATABASE_URL SUBS_DB="$ROOT/source.db" SUB_CFG_DIR="$ROOT" \
  "$BIN" account create --id acct-stage2-e2e --mult-bp 2000
env -u CLAUDE_API_DATABASE_URL SUBS_DB="$ROOT/source.db" SUB_CFG_DIR="$ROOT" \
  "$BIN" account topup acct-stage2-e2e --usd 100 --ref stage2-e2e-seed
env -u CLAUDE_API_DATABASE_URL SUBS_DB="$ROOT/source.db" SUB_CFG_DIR="$ROOT" \
  "$BIN" key issue --account acct-stage2-e2e --key-file "$ROOT/customer-key"

SUBS_DB="$ROOT/source.db" "$BIN" db migrate-postgres --sqlite "$ROOT/source.db"
SUBS_DB="$ROOT/source.db" "$BIN" db verify-postgres

python3 -c '
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("content-length", "0"))
        self.rfile.read(n)
        body = json.dumps({
            "id":"msg_stage2", "type":"message", "role":"assistant",
            "model":"claude-sonnet-4-20250514",
            "content":[{"type":"text", "text":"ok"}],
            "stop_reason":"end_turn", "stop_sequence":None,
            "usage":{"input_tokens":10,"output_tokens":5}
        }).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("request-id", "req_mock_upstream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_): pass
ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
' "$UPSTREAM_PORT" >"$ROOT/upstream.log" 2>&1 &
PIDS+=("$!")

start_engine() {
  local instance=$1 port=$2 db_path=$3 log=$4
  env \
    SUB_CFG_DIR="$ROOT" SUBS_DB="$db_path" SUBS_FLEET=prod \
    CLAUDE_API_INSTANCE_ID="$instance" \
    CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT="$port" \
    CLAUDE_API_UPSTREAM="http://127.0.0.1:$UPSTREAM_PORT" \
    CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
    CLAUDE_API_POLL=0 CLAUDE_API_BILLING=1 \
    CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_DRAIN_DEADLINE_SECS=10 \
    "$BIN" serve >"$log" 2>&1 &
  STARTED_PID=$!
  PIDS+=("$STARTED_PID")
}

wait_ready() {
  local port=$1 log=$2
  for _ in $(seq 1 100); do
    if [[ $(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/ready" 2>/dev/null || true) == 200 ]]; then
      return 0
    fi
    sleep 0.1
  done
  tail -n 80 "$log" >&2
  return 1
}

start_engine stage2-e2e-a "$ENGINE_A_PORT" "$ROOT/engine-shared.db" "$ROOT/engine-a.log"
ENGINE_A_PID=$STARTED_PID
start_engine stage2-e2e-b "$ENGINE_B_PORT" "$ROOT/engine-shared.db" "$ROOT/engine-b.log"
ENGINE_B_PID=$STARTED_PID
wait_ready "$ENGINE_A_PORT" "$ROOT/engine-a.log"
wait_ready "$ENGINE_B_PORT" "$ROOT/engine-b.log"

# PostgreSQL mode deliberately permits the same legacy path because all authority is now fenced.
# SQLite fallback still must reject an identical lock path.
env -u CLAUDE_API_DATABASE_URL \
  SUB_CFG_DIR="$ROOT" SUBS_DB="$ROOT/source.db" SUBS_FLEET=prod \
  CLAUDE_API_INSTANCE_ID=stage2-e2e-sqlite CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT="$SQLITE_PORT" \
  CLAUDE_API_UPSTREAM="http://127.0.0.1:$UPSTREAM_PORT" CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
  CLAUDE_API_POLL=0 CLAUDE_API_BILLING=1 "$BIN" serve >"$ROOT/engine-sqlite.log" 2>&1 &
SQLITE_PID=$!
PIDS+=("$SQLITE_PID")
wait_ready "$SQLITE_PORT" "$ROOT/engine-sqlite.log"
set +e
env -u CLAUDE_API_DATABASE_URL SUB_CFG_DIR="$ROOT" SUBS_DB="$ROOT/source.db" SUBS_FLEET=prod \
  CLAUDE_API_INSTANCE_ID=stage2-e2e-lock CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT="$ENGINE_LOCK_PORT" \
  CLAUDE_API_UPSTREAM="http://127.0.0.1:$UPSTREAM_PORT" CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
  CLAUDE_API_POLL=0 CLAUDE_API_BILLING=1 "$BIN" serve >"$ROOT/engine-lock.log" 2>&1
LOCK_RC=$?
set -e
[[ $LOCK_RC -ne 0 ]] || { echo "duplicate singleton unexpectedly started" >&2; exit 1; }

request() {
  local port=$1 output=$2
  curl --fail-with-body --silent --show-error \
    -H 'content-type: application/json' \
    -H 'anthropic-version: 2023-06-01' \
    -H 'x-api-key: sk-pool-stage2-e2e-customer-key' \
    --data '{"model":"claude-sonnet-4-20250514","max_tokens":64,"messages":[{"role":"user","content":"test"}]}' \
    "http://127.0.0.1:$port/v1/messages" >"$output"
}

request "$ENGINE_A_PORT" "$ROOT/response-a.json" & REQUEST_A=$!
request "$ENGINE_B_PORT" "$ROOT/response-b.json" & REQUEST_B=$!
wait "$REQUEST_A"
wait "$REQUEST_B"
python3 -c 'import json,sys; assert all(json.load(open(p))["type"] == "message" for p in sys.argv[1:])' \
  "$ROOT/response-a.json" "$ROOT/response-b.json"

for _ in $(seq 1 50); do
  VALUES=$(docker exec "$CONTAINER" psql -U commerce -d "$NAME" -AtF: -c \
    "SELECT (SELECT count(*) FROM reservations WHERE state='settled'),
            (SELECT count(*) FROM ledger WHERE kind='charge' AND request_id IS NOT NULL),
            (SELECT count(*) FROM settlement_outbox WHERE state='done'),
            (SELECT count(*) FROM capacity_leases WHERE state='active'),
            (SELECT coalesce(sum(inflight),0) FROM pool_state),
            (SELECT reserved_nano FROM accounts WHERE id='acct-stage2-e2e')")
  [[ $VALUES == "2:2:2:0:0:0" ]] && break
  sleep 0.1
done
[[ $VALUES == "2:2:2:0:0:0" ]] || { echo "unexpected authority totals: $VALUES" >&2; exit 1; }

ROLE_FLAGS=$(docker exec "$CONTAINER" psql -U commerce -d postgres -AtF: -c \
  "SELECT rolsuper,rolcreatedb,rolcreaterole,rolreplication FROM pg_roles WHERE rolname='$NAME'")
[[ $ROLE_FLAGS == "f:f:f:f" ]] || { echo "engine role is over-privileged: $ROLE_FLAGS" >&2; exit 1; }
if docker exec "$CONTAINER" psql -U "$NAME" -d commerce -Atqc 'SELECT count(*) FROM users' \
  >/dev/null 2>&1; then
  echo "engine role unexpectedly read the commerce users table" >&2
  exit 1
fi

kill -USR1 "$ENGINE_A_PID"
for _ in $(seq 1 50); do
  PRE_DRAIN_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$ENGINE_A_PORT/ready" || true)
  [[ $PRE_DRAIN_STATUS == 503 ]] && break
  sleep 0.1
done
[[ $PRE_DRAIN_STATUS == 503 ]] || { echo "SIGUSR1 did not flip readiness" >&2; exit 1; }
[[ $(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:$ENGINE_A_PORT/health") == 200 ]] \
  || { echo "pre-drain stopped the listener instead of preserving in-flight service" >&2; exit 1; }

kill -TERM "$ENGINE_A_PID" "$ENGINE_B_PID" "$SQLITE_PID"
wait "$ENGINE_A_PID"
wait "$ENGINE_B_PID"
wait "$SQLITE_PID"
PIDS=("${PIDS[0]}")

echo "stage2 e2e passed: same-path PG owners, exactly-once charges, drained capacity, pre-drain, SQLite singleton fallback, graceful shutdown"
