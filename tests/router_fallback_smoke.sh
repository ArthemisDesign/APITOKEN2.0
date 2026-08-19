#!/usr/bin/env bash
# Reproducible phase-6.4c load smoke for the real claude-router binary and deterministic TCP mocks.
# No live subscription, PostgreSQL, quota or production credential is used.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd -- "$HERE/.." && pwd)
ROUTER_BIN=${CLAUDE_ROUTER_BIN:-$ROOT/target/debug/claude-router}
[[ -x $ROUTER_BIN ]] || { printf 'missing %s; run cargo build first\n' "$ROUTER_BIN" >&2; exit 2; }

DATA=$(mktemp -d)
umask 077
PIDS=()
cleanup() {
  local pid
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait "${PIDS[@]}" 2>/dev/null || true
  rm -rf -- "$DATA"
}
trap cleanup EXIT

wait_ready_file() {
  local file=$1 pid=$2
  for _ in $(seq 1 80); do
    [[ -s $file ]] && return 0
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.05
  done
  printf 'mock did not publish readiness: %s\n' "$file" >&2
  return 1
}

start_plane() {
  local plane=$1
  local ready=$DATA/$plane.ready events=$DATA/$plane.events log=$DATA/$plane.log
  python3 "$HERE/router_fallback_mock.py" \
    --plane "$plane" --ready-file "$ready" --events-file "$events" >"$log" 2>&1 &
  local pid=$!
  PIDS+=("$pid")
  wait_ready_file "$ready" "$pid"
  printf '%s %s\n' "$pid" "$(tr -d '\n' <"$ready")"
}

read -r ANTHROPIC_PID ANTHROPIC_PORT < <(start_plane anthropic)
read -r OPENAI_PID OPENAI_PORT < <(start_plane openai)
read -r GOOGLE_PID GOOGLE_PORT < <(start_plane google)
PIDS+=("$ANTHROPIC_PID" "$OPENAI_PID" "$GOOGLE_PID")

ROUTER_PORT=$(python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
)
mkdir -m 700 "$DATA/router-spool"
CLAUDE_ROUTER_BODY_SPOOL_ROOT="$DATA/router-spool" \
CLAUDE_ROUTER_HOST=127.0.0.1 \
CLAUDE_ROUTER_PORT=$ROUTER_PORT \
CLAUDE_ROUTER_ANTHROPIC_ORIGIN=http://127.0.0.1:$ANTHROPIC_PORT \
CLAUDE_ROUTER_OPENAI_ORIGIN=http://127.0.0.1:$OPENAI_PORT \
CLAUDE_ROUTER_GEMINI_ORIGIN=http://127.0.0.1:$GOOGLE_PORT \
CLAUDE_ROUTER_FALLBACK_ENABLED=1 \
"$ROUTER_BIN" >"$DATA/router.log" 2>&1 &
ROUTER_PID=$!
PIDS+=("$ROUTER_PID")
for _ in $(seq 1 80); do
  curl -sf --max-time 1 "http://127.0.0.1:$ROUTER_PORT/ready" >/dev/null && break
  kill -0 "$ROUTER_PID" 2>/dev/null || break
  sleep 0.05
done
curl -sf --max-time 1 "http://127.0.0.1:$ROUTER_PORT/ready" >/dev/null || {
  tail -n 80 "$DATA/router.log" >&2
  exit 2
}

ROUTER=http://127.0.0.1:$ROUTER_PORT
CHAIN='"model":"anthropic/claude-sonnet-5","models":["openai/gpt-5.6-terra"]'
metric_value() {
  local series=$1
  curl -sf "$ROUTER/metrics" | awk -v series="$series" \
    'index($0, series " ") == 1 { print $2; found=1 } END { if (!found) exit 1 }'
}
event_count() {
  local file=$1 plane=$2 scenario=$3
  python3 - "$file" "$plane" "$scenario" <<'PY'
import json, sys
path, _plane, scenario = sys.argv[1:]
count = 0
with open(path, encoding="utf-8") as events:
    for line in events:
        event = json.loads(line)
        count += event.get("kind") == "execution" and event.get("scenario") == scenario
print(count)
PY
}
request() {
  local key=$1 body=$2 response=$3
  curl -sS --max-time 10 -o "$response" -w '%{http_code}' \
    -H 'content-type: application/json' -H "x-api-key: $key" \
    --data-binary "$body" "$ROUTER/v1/responses"
}

NOT_STARTED_SERIES='claude_router_fallback_total{from_namespace="anthropic",to_namespace="openai",reason="not_started"}'
REFUSED_SERIES='claude_router_fallback_total{from_namespace="anthropic",to_namespace="openai",reason="connect_refused"}'

printf '[1] concurrent exact not_started continuations\n'
LOAD_REQUESTS=${ROUTER_FALLBACK_LOAD_REQUESTS:-24}
BEFORE=$(metric_value "$NOT_STARTED_SERIES")
LOAD_PIDS=()
for index in $(seq 1 "$LOAD_REQUESTS"); do
  (
    code=$(request mock-load-key "{$CHAIN,\"input\":\"signed_retry\"}" "$DATA/load-$index.body")
    [[ $code == 200 ]]
  ) &
  LOAD_PIDS+=("$!")
done
LOAD_FAILED=0
for pid in "${LOAD_PIDS[@]}"; do
  wait "$pid" || LOAD_FAILED=1
done
[[ $LOAD_FAILED == 0 ]] || { printf 'a concurrent fallback request failed\n' >&2; exit 1; }
AFTER=$(metric_value "$NOT_STARTED_SERIES")
(( AFTER - BEFORE == LOAD_REQUESTS )) || {
  printf 'not_started metric delta=%s, expected=%s\n' "$((AFTER - BEFORE))" "$LOAD_REQUESTS" >&2
  exit 1
}
[[ $(event_count "$DATA/anthropic.events" anthropic signed_retry) == "$LOAD_REQUESTS" ]]
[[ $(event_count "$DATA/openai.events" openai signed_retry) == "$LOAD_REQUESTS" ]]

printf '[2] provider filter then strict policy before attempt 1\n'
BEFORE=$(metric_value "$NOT_STARTED_SERIES")
POLICY_BODY='{"model":"anthropic/claude-sonnet-5","models":["openai/gpt-5.6-terra","google/gemini-3.6-flash"],"provider":{"only":["anthropic","openai"]},"input":"policy_filter"}'
CODE=$(request mock-policy-filter-key "$POLICY_BODY" "$DATA/policy.body")
[[ $CODE == 200 ]] || { printf 'policy-filter request returned %s\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$NOT_STARTED_SERIES")
[[ $AFTER == "$BEFORE" ]]
python3 - "$DATA/anthropic.events" "$DATA/openai.events" "$DATA/google.events" <<'PY'
import json, sys
events = []
for path in sys.argv[1:]:
    with open(path, encoding="utf-8") as source:
        events.extend(json.loads(line) for line in source)
preflights = [event for event in events if event.get("kind") == "preflight" and event.get("scenario") == "policy_filter"]
assert preflights, "policy preflight was not observed"
assert preflights[0]["candidates"] == ["anthropic/claude-sonnet-5", "openai/gpt-5.6-terra"], preflights
executions = [event for event in events if event.get("kind") == "execution" and event.get("scenario") == "policy_filter"]
assert len(executions) == 1 and executions[0]["attempt"] == "none", executions
PY
[[ $(event_count "$DATA/anthropic.events" anthropic policy_filter) == 0 ]]
[[ $(event_count "$DATA/openai.events" openai policy_filter) == 1 ]]
[[ $(event_count "$DATA/google.events" google policy_filter) == 0 ]]

printf '[3] unsigned 503 is terminal and does not inflate telemetry\n'
BEFORE=$(metric_value "$NOT_STARTED_SERIES")
OPENAI_BEFORE=$(event_count "$DATA/openai.events" openai unsigned_stop)
CODE=$(request mock-unsigned-key "{$CHAIN,\"input\":\"unsigned_stop\"}" "$DATA/unsigned.body")
[[ $CODE == 503 ]] || { printf 'unsigned 503 returned %s\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$NOT_STARTED_SERIES")
[[ $AFTER == "$BEFORE" ]]
[[ $(event_count "$DATA/openai.events" openai unsigned_stop) == "$OPENAI_BEFORE" ]]

printf '[4] last-good catalog plus killed first plane proves ConnectionRefused\n'
kill "$ANTHROPIC_PID"
wait "$ANTHROPIC_PID" 2>/dev/null || true
BEFORE=$(metric_value "$REFUSED_SERIES")
OPENAI_BEFORE=$(event_count "$DATA/openai.events" openai connect_refused)
CODE=$(request mock-connect-key "{$CHAIN,\"input\":\"connect_refused\"}" "$DATA/refused.body")
[[ $CODE == 200 ]] || { printf 'ConnectionRefused fallback returned %s\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$REFUSED_SERIES")
(( AFTER - BEFORE == 1 )) || { printf 'connect_refused metric delta=%s, expected=1\n' "$((AFTER - BEFORE))" >&2; exit 1; }
(( $(event_count "$DATA/openai.events" openai connect_refused) - OPENAI_BEFORE == 1 ))

curl -sf "$ROUTER/metrics" >"$DATA/metrics.final"
[[ $(grep -c '^claude_router_fallback_total{' "$DATA/metrics.final") == 18 ]]
! grep -Eq 'model=|credential=|group=|request_id=' "$DATA/metrics.final"
printf 'router fallback smoke passed: load=%s, policy/provider preflight fenced, ambiguous 503 terminal, ConnectionRefused continued\n' "$LOAD_REQUESTS"
