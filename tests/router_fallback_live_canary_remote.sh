#!/usr/bin/env bash
# Sourced remotely by router_fallback_live_canary.sh. Do not invoke directly: the bootstrap keeps
# APITOKEN_API_KEY as an unexported shell variable so shims and the canary router cannot inherit it.
set -euo pipefail
set +x
: "${APITOKEN_API_KEY:?missing stdin-delivered API key}"
: "${APITOKEN_CANARY_EXPECTED_SHA:?missing expected deployment SHA}"
export -n APITOKEN_API_KEY APITOKEN_CANARY_EXPECTED_SHA 2>/dev/null || true

DATA=$(mktemp -d /tmp/router-fallback-canary.XXXXXXXX)
umask 077
PIDS=()
cleanup() {
  local pid
  APITOKEN_API_KEY=
  unset APITOKEN_API_KEY
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait "${PIDS[@]}" 2>/dev/null || true
  rm -rf -- "$DATA"
}
trap cleanup EXIT

ROUTER_ACTIVE_PORT=$(sed -n \
  's/^[[:space:]]*reverse_proxy 127\.0\.0\.1:\(880[01]\)[[:space:]]*$/\1/p' \
  /etc/caddy/router-active.caddy)
[[ $ROUTER_ACTIVE_PORT == 8800 || $ROUTER_ACTIVE_PORT == 8801 ]] || {
  printf 'production router active-backend state is not a blue-green slot\n' >&2
  exit 1
}
ROUTER_SERVICE="claude-router@$ROUTER_ACTIVE_PORT.service"
ROUTER_SERVICE_PID=$(systemctl show --property MainPID --value "$ROUTER_SERVICE")
[[ $ROUTER_SERVICE_PID =~ ^[1-9][0-9]*$ && -x /proc/$ROUTER_SERVICE_PID/exe ]] || {
  printf 'production %s has no executable MainPID\n' "$ROUTER_SERVICE" >&2
  exit 1
}
DEPLOYED_BINARY=$(readlink -f "/proc/$ROUTER_SERVICE_PID/exe")
DEPLOYED_SHA=$(basename -- "$(dirname -- "$DEPLOYED_BINARY")")
[[ $DEPLOYED_SHA == "$APITOKEN_CANARY_EXPECTED_SHA" ]] || {
  printf 'running router SHA %s does not match expected GREEN SHA %s\n' \
    "$DEPLOYED_SHA" "$APITOKEN_CANARY_EXPECTED_SHA" >&2
  exit 1
}
[[ $(<"$(dirname -- "$DEPLOYED_BINARY")/.release-sha") == "$DEPLOYED_SHA" ]] || {
  printf 'running router immutable release marker does not match its directory\n' >&2
  exit 1
}

cat >"$DATA/shim.py" <<'PY'
#!/usr/bin/env python3
import argparse
import http.client
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--namespace", choices=("anthropic", "openai", "google"), required=True)
parser.add_argument("--upstream-port", type=int, required=True)
parser.add_argument("--ready-file", type=Path, required=True)
args = parser.parse_args()
lock = threading.Lock()
state = {"executions": {}, "models": {}, "last_preflight": None}
hop = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"}
marker_header = "x-apitoken-canary-mode"

def bump(mode, model):
    with lock:
        state["executions"][mode] = state["executions"].get(mode, 0) + 1
        state["models"].setdefault(mode, []).append(model)

class Handler(BaseHTTPRequestHandler):
    server_version = "RouterLiveCanaryShim/1"
    def log_message(self, *_args):
        return
    def send_bytes(self, status, body, headers=()):
        self.send_response(status)
        for name, value in headers:
            lower = name.lower()
            if lower not in hop and lower not in {"content-length", "server", "date"}:
                self.send_header(name, value)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def send_json(self, status, body, headers=()):
        encoded = json.dumps(body, separators=(",", ":")).encode()
        self.send_bytes(status, encoded, [("content-type", "application/json"), *headers])
    def proxy(self, body):
        headers = {}
        for name, value in self.headers.items():
            lower = name.lower()
            if lower not in hop and lower not in {"host", marker_header}:
                headers[name] = value
        connection = http.client.HTTPConnection("127.0.0.1", args.upstream_port, timeout=60)
        connection.request(self.command, self.path, body=body, headers=headers)
        response = connection.getresponse()
        response_body = response.read()
        response_headers = response.getheaders()
        if self.path == "/internal/router/policy/preflight" and response.status < 300:
            try:
                request = json.loads(body or b"{}")
                result = json.loads(response_body)
                snapshot = {
                    "candidates": [candidate.get("id") for candidate in request.get("candidates", [])],
                    "mode": result.get("mode"),
                    "allowed": result.get("allowed"),
                }
                with lock:
                    state["last_preflight"] = snapshot
            except (json.JSONDecodeError, UnicodeDecodeError):
                pass
        self.send_bytes(response.status, response_body, response_headers)
        connection.close()
    def handle_request(self):
        if self.path == "/__state":
            with lock:
                snapshot = json.loads(json.dumps(state))
            self.send_json(200, snapshot)
            return
        length = int(self.headers.get("content-length", "0") or "0")
        if length > 32 * 1024 * 1024:
            self.send_json(413, {"error": "canary shim body too large"})
            return
        body = self.rfile.read(length) if length else b""
        is_preflight = self.path == "/internal/router/policy/preflight"
        is_catalog = self.command == "GET" and "models" in self.path
        if is_preflight or is_catalog or self.command == "GET":
            self.proxy(body)
            return
        try:
            request = json.loads(body or b"{}")
            model = request.get("model", "")
        except (json.JSONDecodeError, UnicodeDecodeError):
            model = ""
        mode = self.headers.get(marker_header, "ordinary")
        bump(mode, model)
        attempt = self.headers.get("x-apitoken-attempt", "")
        if mode.startswith("policy_guard_allow:"):
            expected = mode.split(":", 1)[1]
            if model != expected:
                self.send_json(409, {"error": "policy/provider filtering did not fence attempt 1"})
                return
        elif mode.startswith("signed_not_started:"):
            expected = mode.split(":", 1)[1]
            if model == expected and attempt == "1":
                self.send_json(503, {"error": "synthetic exact not_started"}, [("x-apitoken-execution-state", "not_started")])
                return
        elif mode.startswith("unsigned_503:"):
            expected = mode.split(":", 1)[1]
            if model == expected and attempt == "1":
                self.send_json(503, {"error": "synthetic ambiguous 503"})
                return
        self.proxy(body)
    do_GET = handle_request
    do_POST = handle_request

server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
args.ready_file.write_text(str(server.server_address[1]) + "\n", encoding="utf-8")
args.ready_file.chmod(0o600)
server.serve_forever()
PY
chmod 0700 "$DATA/shim.py"

wait_ready_file() {
  local file=$1 pid=$2
  for _ in $(seq 1 100); do
    [[ -s $file ]] && return 0
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.05
  done
  return 1
}
start_shim() {
  local namespace=$1 upstream=$2
  local ready=$DATA/$namespace.ready
  python3 "$DATA/shim.py" --namespace "$namespace" --upstream-port "$upstream" \
    --ready-file "$ready" >"$DATA/$namespace.log" 2>&1 &
  local pid=$!
  wait_ready_file "$ready" "$pid"
  printf '%s %s\n' "$pid" "$(tr -d '\n' <"$ready")"
}
read -r ANTHROPIC_PID ANTHROPIC_PORT < <(start_shim anthropic 8790)
read -r OPENAI_PID OPENAI_PORT < <(start_shim openai 8792)
read -r GOOGLE_PID GOOGLE_PORT < <(start_shim google 8794)
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
"/proc/$ROUTER_SERVICE_PID/exe" >"$DATA/router.log" 2>&1 &
CANARY_ROUTER_PID=$!
PIDS+=("$CANARY_ROUTER_PID")
for _ in $(seq 1 100); do
  curl -sf --max-time 1 "http://127.0.0.1:$ROUTER_PORT/ready" >/dev/null && break
  kill -0 "$CANARY_ROUTER_PID" 2>/dev/null || break
  sleep 0.05
done
curl -sf --max-time 1 "http://127.0.0.1:$ROUTER_PORT/ready" >/dev/null || {
  printf 'canary router did not become ready\n' >&2
  exit 1
}
ROUTER=http://127.0.0.1:$ROUTER_PORT

curl_with_key() {
  local url=$1
  shift
  printf 'header = "x-api-key: %s"\n' "$APITOKEN_API_KEY" | \
    curl --config - -sS --max-time 75 "$@" "$url"
}
prom_value() {
  local query=$1
  curl -sfG --data-urlencode "query=$query" http://127.0.0.1:9090/api/v1/query | \
    python3 -c 'import json,sys; data=json.load(sys.stdin); result=data["data"]["result"]; assert len(result)==1, result; print(result[0]["value"][1])'
}
metric_value() {
  local series=$1
  curl -sf "$ROUTER/metrics" | awk -v series="$series" \
    'index($0, series " ") == 1 { print $2; found=1 } END { if (!found) exit 1 }'
}
state_total() {
  local mode=$1
  python3 - "$mode" \
    "http://127.0.0.1:$ANTHROPIC_PORT/__state" \
    "http://127.0.0.1:$OPENAI_PORT/__state" \
    "http://127.0.0.1:$GOOGLE_PORT/__state" <<'PY'
import json, sys, urllib.request
mode = sys.argv[1]
total = 0
for url in sys.argv[2:]:
    try:
        with urllib.request.urlopen(url, timeout=2) as response:
            total += json.load(response)["executions"].get(mode, 0)
    except OSError:
        pass
print(total)
PY
}
request_router() {
  local body_file=$1 mode=$2 response_file=$3
  curl_with_key "$ROUTER/v1/responses" -o "$response_file" -w '%{http_code}' \
    -H 'content-type: application/json' -H "x-apitoken-canary-mode: $mode" \
    --data-binary "@$body_file"
}

[[ $(prom_value 'count(claude_router_fallback_total)') == 18 ]]
[[ $(prom_value 'sum(claude_router_fallback_total)') == 0 ]]
[[ $(prom_value 'count(claude_api_execution_not_started_total)') -ge 3 ]]
DOUBLE_WINNER_BEFORE=$(prom_value 'sum(claude_api_execution_group_double_winner_total) or vector(0)')
[[ $(prom_value 'max(apitoken_balance_divergence_nano) or vector(0)') == 0 ]]
SETTLEMENT_BEFORE=$(prom_value 'max(apitoken_engine_settlement_pending) or vector(0)')

CATALOG_CODE=$(curl_with_key "$ROUTER/v1/models" -o "$DATA/catalog.json" -w '%{http_code}')
[[ $CATALOG_CODE == 200 ]] || {
  printf 'live aggregate catalog did not return 200\n' >&2
  exit 1
}
cat >"$DATA/reviewed_candidates.json" <<'JSON'
{"schema_version":1,"candidates":[
{"id":"anthropic/claude-opus-5","provider_id":"anthropic","canonical_model_id":"claude-opus-5"},
{"id":"anthropic/claude-sonnet-5","provider_id":"anthropic","canonical_model_id":"claude-sonnet-5"},
{"id":"anthropic/claude-fable-5","provider_id":"anthropic","canonical_model_id":"claude-fable-5"},
{"id":"anthropic/claude-opus-4-8","provider_id":"anthropic","canonical_model_id":"claude-opus-4-8"},
{"id":"anthropic/claude-sonnet-4-6","provider_id":"anthropic","canonical_model_id":"claude-sonnet-4-6"},
{"id":"openai/gpt-5.6-sol","provider_id":"openai","canonical_model_id":"gpt-5.6-sol"},
{"id":"openai/gpt-5.6-terra","provider_id":"openai","canonical_model_id":"gpt-5.6-terra"},
{"id":"openai/gpt-5.6-luna","provider_id":"openai","canonical_model_id":"gpt-5.6-luna"},
{"id":"openai/gpt-5.5","provider_id":"openai","canonical_model_id":"gpt-5.5"},
{"id":"openai/gpt-5.4","provider_id":"openai","canonical_model_id":"gpt-5.4"},
{"id":"google/gemini-3.6-flash","provider_id":"google","canonical_model_id":"gemini-3.6-flash"},
{"id":"google/gemini-3.5-flash","provider_id":"google","canonical_model_id":"gemini-3.5-flash"},
{"id":"google/gemini-3.1-pro-preview","provider_id":"google","canonical_model_id":"gemini-3.1-pro-preview"},
{"id":"google/gemini-3.1-flash-lite","provider_id":"google","canonical_model_id":"gemini-3.1-flash-lite"},
{"id":"google/gemini-2.5-flash","provider_id":"google","canonical_model_id":"gemini-2.5-flash"}
]}
JSON
python3 - "$DATA/reviewed_candidates.json" "$DATA/catalog.json" "$DATA/candidates.json" <<'PY'
import json, sys
reviewed = json.load(open(sys.argv[1]))
live = {entry.get("id") for entry in json.load(open(sys.argv[2])).get("data", [])}
reviewed["candidates"] = [candidate for candidate in reviewed["candidates"] if candidate["id"] in live]
assert reviewed["candidates"], "aggregate catalog contains none of the reviewed canary candidates"
json.dump(reviewed, open(sys.argv[3], "w"), separators=(",", ":"))
PY
curl_with_key "http://127.0.0.1:$ANTHROPIC_PORT/internal/router/policy/preflight" \
  -H 'content-type: application/json' --data-binary "@$DATA/candidates.json" \
  -o "$DATA/preflight.json" -w '%{http_code}' >"$DATA/preflight.code"
[[ $(<"$DATA/preflight.code") == 200 ]] || {
  printf 'live policy preflight did not return 200\n' >&2
  exit 1
}

python3 - "$DATA/candidates.json" "$DATA/preflight.json" "$DATA/selection.json" <<'PY'
import json, sys
candidates = json.load(open(sys.argv[1]))["candidates"]
response = json.load(open(sys.argv[2]))
assert response.get("mode") == "strict", f'live key policy mode is {response.get("mode")!r}, strict evidence required'
allowed_ids = response.get("allowed", [])
all_ids = [candidate["id"] for candidate in candidates]
assert allowed_ids and set(allowed_ids) < set(all_ids), "strict policy needs both allowed and denied reviewed candidates"
allowed = [candidate for candidate in candidates if candidate["id"] in allowed_ids]
denied = [candidate for candidate in candidates if candidate["id"] not in allowed_ids]
fallback = next(((first, second) for first in allowed for second in allowed if first["provider_id"] != second["provider_id"]), None)
assert fallback is not None, "strict policy needs two allowed provider namespaces for a live cross-plane canary"
policy_pair = None
for blocked in denied:
    for accepted in allowed:
        used = {blocked["provider_id"], accepted["provider_id"]}
        ignored = next((candidate for candidate in candidates if candidate["provider_id"] not in used), None)
        if ignored:
            policy_pair = (blocked, accepted, ignored)
            break
    if policy_pair:
        break
assert policy_pair is not None, "cannot build strict+provider-filter canary from reviewed candidates"
blocked, accepted, ignored = policy_pair
first, second = fallback
json.dump({
    "policy": {"blocked": blocked, "accepted": accepted, "ignored": ignored},
    "fallback": {"first": first, "second": second},
}, open(sys.argv[3], "w"), separators=(",", ":"))
PY

read_selection() {
  python3 - "$DATA/selection.json" "$1" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
for part in sys.argv[2].split("."):
    value = value[part]
print(value)
PY
}
POLICY_BLOCKED=$(read_selection policy.blocked.id)
POLICY_ACCEPTED=$(read_selection policy.accepted.id)
POLICY_IGNORED=$(read_selection policy.ignored.id)
POLICY_IGNORED_NS=$(read_selection policy.ignored.provider_id)
FROM_MODEL=$(read_selection fallback.first.id)
FROM_NS=$(read_selection fallback.first.provider_id)
TO_MODEL=$(read_selection fallback.second.id)
TO_NS=$(read_selection fallback.second.provider_id)

python3 - "$DATA/policy.body" "$POLICY_BLOCKED" "$POLICY_ACCEPTED" "$POLICY_IGNORED" "$POLICY_IGNORED_NS" <<'PY'
import json, sys
path, blocked, accepted, ignored, ignored_ns = sys.argv[1:]
json.dump({"model": blocked, "models": [accepted, ignored], "provider": {"ignore": [ignored_ns]}, "input": "Reply with exactly OK.", "max_output_tokens": 8}, open(path, "w"), separators=(",", ":"))
PY
POLICY_MODE=policy_guard_allow:$POLICY_ACCEPTED
POLICY_BEFORE=$(state_total "$POLICY_MODE")
CODE=$(request_router "$DATA/policy.body" "$POLICY_MODE" "$DATA/policy.response")
[[ $CODE =~ ^2 ]] || { printf 'strict/provider-filter canary returned HTTP %s\n' "$CODE" >&2; exit 1; }
(( $(state_total "$POLICY_MODE") - POLICY_BEFORE == 1 ))
python3 - "$DATA/selection.json" \
  "http://127.0.0.1:$ANTHROPIC_PORT/__state" \
  "http://127.0.0.1:$OPENAI_PORT/__state" \
  "http://127.0.0.1:$GOOGLE_PORT/__state" <<'PY'
import json, sys, urllib.request
selection = json.load(open(sys.argv[1]))["policy"]
states = {}
for namespace, url in zip(("anthropic", "openai", "google"), sys.argv[2:]):
    with urllib.request.urlopen(url, timeout=2) as response:
        states[namespace] = json.load(response)
preflight = states[selection["blocked"]["provider_id"]]["last_preflight"]
assert preflight["mode"] == "strict", preflight
assert preflight["candidates"] == [selection["blocked"]["id"], selection["accepted"]["id"]], preflight
assert preflight["allowed"] == [selection["accepted"]["id"]], preflight
PY

python3 - "$DATA/fallback.body" "$FROM_MODEL" "$TO_MODEL" <<'PY'
import json, sys
json.dump({"model": sys.argv[2], "models": [sys.argv[3]], "input": "Reply with exactly OK.", "max_output_tokens": 8}, open(sys.argv[1], "w"), separators=(",", ":"))
PY
NOT_STARTED_SERIES="claude_router_fallback_total{from_namespace=\"$FROM_NS\",to_namespace=\"$TO_NS\",reason=\"not_started\"}"
REFUSED_SERIES="claude_router_fallback_total{from_namespace=\"$FROM_NS\",to_namespace=\"$TO_NS\",reason=\"connect_refused\"}"

SIGNED_MODE=signed_not_started:$FROM_MODEL
BEFORE=$(metric_value "$NOT_STARTED_SERIES")
SIGNED_BEFORE=$(state_total "$SIGNED_MODE")
CODE=$(request_router "$DATA/fallback.body" "$SIGNED_MODE" "$DATA/signed.response")
[[ $CODE =~ ^2 ]] || { printf 'signed serial continuation returned HTTP %s\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$NOT_STARTED_SERIES")
(( AFTER - BEFORE == 1 ))
(( $(state_total "$SIGNED_MODE") - SIGNED_BEFORE == 2 ))

UNSIGNED_MODE=unsigned_503:$FROM_MODEL
BEFORE=$(metric_value "$NOT_STARTED_SERIES")
UNSIGNED_BEFORE=$(state_total "$UNSIGNED_MODE")
CODE=$(request_router "$DATA/fallback.body" "$UNSIGNED_MODE" "$DATA/unsigned.response")
[[ $CODE == 503 ]] || { printf 'unsigned ambiguity returned HTTP %s instead of 503\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$NOT_STARTED_SERIES")
[[ $AFTER == "$BEFORE" ]]
(( $(state_total "$UNSIGNED_MODE") - UNSIGNED_BEFORE == 1 ))

case $FROM_NS in
  anthropic) FIRST_PID=$ANTHROPIC_PID ;;
  openai) FIRST_PID=$OPENAI_PID ;;
  google) FIRST_PID=$GOOGLE_PID ;;
  *) exit 1 ;;
esac
kill "$FIRST_PID"
wait "$FIRST_PID" 2>/dev/null || true
BEFORE=$(metric_value "$REFUSED_SERIES")
CODE=$(request_router "$DATA/fallback.body" ordinary "$DATA/refused.response")
[[ $CODE =~ ^2 ]] || { printf 'ConnectionRefused serial continuation returned HTTP %s\n' "$CODE" >&2; exit 1; }
AFTER=$(metric_value "$REFUSED_SERIES")
(( AFTER - BEFORE == 1 ))

[[ $(prom_value 'sum(claude_api_execution_group_double_winner_total) or vector(0)') == "$DOUBLE_WINNER_BEFORE" ]]
[[ $(prom_value 'max(apitoken_balance_divergence_nano) or vector(0)') == 0 ]]
SETTLEMENT_OK=0
for _ in $(seq 1 30); do
  SETTLEMENT_AFTER=$(prom_value 'max(apitoken_engine_settlement_pending) or vector(0)')
  python3 - "$SETTLEMENT_AFTER" "$SETTLEMENT_BEFORE" <<'PY' && { SETTLEMENT_OK=1; break; }
from decimal import Decimal
import sys
raise SystemExit(0 if Decimal(sys.argv[1]) <= Decimal(sys.argv[2]) else 1)
PY
  sleep 2
done
[[ $SETTLEMENT_OK == 1 ]] || {
  printf 'settlement backlog did not return to its pre-canary baseline\n' >&2
  exit 1
}

printf 'live fallback canary passed on deployed SHA %s: strict/provider filtering, signed continuation, unsigned terminal outcome, ConnectionRefused continuation, money detectors clean\n' "$DEPLOYED_SHA"
