#!/usr/bin/env bash
# Loopback large-payload admission smoke: generated bodies, no giant fixture.
# Client → claude-router (missing `model` JSON) must cross the raised request cap and fail
# locally 400. A 413 means the router cap is still below the requested size.
#
# Default size is 8 MiB. Production candidate gate uses 8,32,64,128,256.
# Run 256 locally with LARGE_PAYLOAD_SMOKE_MIB=8,32,64,128,256.
#
# Запуск: cargo build && bash tests/large_payload_smoke.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BIN="${CLAUDE_API_BIN:-$ROOT/target/debug/claude-api}"
RTR="${CLAUDE_ROUTER_BIN:-$ROOT/target/debug/claude-router}"
[ -x "$BIN" ] || { echo "нет бинаря $BIN — сначала: cargo build" >&2; exit 2; }
[ -x "$RTR" ] || { echo "нет бинаря $RTR — сначала: cargo build" >&2; exit 2; }

DATA="$(mktemp -d)"
export SUB_CFG_DIR="$DATA"
umask 077
MOCKP=${MOCKP:-9131}; SRVP=${SRVP:-9132}; RTRP=${RTRP:-9133}
SIZES=${LARGE_PAYLOAD_SMOKE_MIB:-8}
cleanup() { kill "${RTRPID:-}" "${SRV:-}" "${MOCK:-}" 2>/dev/null; rm -rf "$DATA"; }
trap cleanup EXIT

python3 "$HERE/mock_upstream.py" "$MOCKP" >"$DATA/mock.log" 2>&1 & MOCK=$!
for _ in $(seq 1 40); do
  kill -0 "$MOCK" 2>/dev/null && curl -s -m1 -o /dev/null "http://127.0.0.1:$MOCKP/" && break
  sleep 0.1
done
if ! kill -0 "$MOCK" 2>/dev/null; then
  echo "mock upstream не поднялся" >&2; tail -n 80 "$DATA/mock.log" >&2; exit 2
fi

token_file="$DATA/token-a"
printf '%s\n' "faketokenaaaaaaaaaaaa" > "$token_file"
"$BIN" sub add-file "sub-a@test.io" --token-file "$token_file" >/dev/null 2>&1 || exit 2
"$BIN" sub set-plan "sub-a@test.io" max20 >/dev/null 2>&1 || exit 2

ADMIN_KEY="large-payload-smoke-admin-key"
mkdir -m 700 "$DATA/engine-spool" "$DATA/router-spool"
CLAUDE_API_PROVIDER=anthropic \
CLAUDE_API_BODY_SPOOL_ROOT="$DATA/engine-spool" \
CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=$SRVP CLAUDE_API_KEYS="$ADMIN_KEY" \
CLAUDE_API_BILLING=0 CLAUDE_API_POLL=0 CLAUDE_API_UPSTREAM="http://127.0.0.1:$MOCKP" \
CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
"$BIN" serve >"$DATA/srv.log" 2>&1 & SRV=$!
for i in $(seq 1 40); do curl -s -m1 "http://127.0.0.1:$SRVP/health" >/dev/null 2>&1 && break; sleep 0.25; done
if ! kill -0 "$SRV" 2>/dev/null || ! curl -sf -m1 "http://127.0.0.1:$SRVP/health" >/dev/null; then
  echo "mock engine не поднялся" >&2; tail -n 80 "$DATA/srv.log" >&2; exit 2
fi

CLAUDE_ROUTER_BODY_SPOOL_ROOT="$DATA/router-spool" \
CLAUDE_ROUTER_HOST=127.0.0.1 CLAUDE_ROUTER_PORT=$RTRP \
CLAUDE_ROUTER_ANTHROPIC_ORIGIN="http://127.0.0.1:$SRVP" \
CLAUDE_ROUTER_OPENAI_ORIGIN="http://127.0.0.1:1" \
CLAUDE_ROUTER_GEMINI_ORIGIN="http://127.0.0.1:2" \
"$RTR" >"$DATA/rtr.log" 2>&1 & RTRPID=$!
for i in $(seq 1 40); do curl -s -m1 "http://127.0.0.1:$RTRP/health" >/dev/null 2>&1 && break; sleep 0.25; done
if ! kill -0 "$RTRPID" 2>/dev/null || ! curl -sf -m1 "http://127.0.0.1:$RTRP/health" >/dev/null; then
  echo "router не поднялся" >&2; tail -n 80 "$DATA/rtr.log" >&2; exit 2
fi

printf 'Bearer %s\n' "$ADMIN_KEY" >"$DATA/authorization"
chmod 600 "$DATA/authorization"
python3 "$HERE/large_payload_mock_load.py" \
  --url "http://127.0.0.1:$RTRP/v1/chat/completions" \
  --sizes-mib "$SIZES" \
  --concurrency 2 \
  --authorization-file "$DATA/authorization" >"$DATA/load.json" || {
  echo "load driver failed" >&2
  cat "$DATA/load.json" >&2
  exit 1
}
python3 - "$DATA/load.json" <<'PY'
import json, sys
load = json.load(open(sys.argv[1], encoding="utf-8"))
statuses = [int(row.get("status", 0)) for row in load["requests"]]
if not statuses:
    raise SystemExit("no requests")
bad = [status for status in statuses if not (400 <= status < 500) or status in (408, 413, 431)]
if bad:
    print("unexpected statuses:", statuses, file=sys.stderr)
    raise SystemExit(1)
print("large-payload smoke ok statuses=" + ",".join(str(s) for s in statuses))
PY
