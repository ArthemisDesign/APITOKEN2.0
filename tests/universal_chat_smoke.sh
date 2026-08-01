#!/usr/bin/env bash
# E2E smoke universal chat lane — этап 3.1 docs/engine/UNIFIED_ROUTER.md.
# Прогоняет ПОЛНУЮ цепочку без живых подписок и квоты (мок-апстрим):
#   клиент → claude-router (model-based dispatch) → claude-api (Anthropic
#   plane, chat→Messages адаптер) → mock upstream.
# Проверяет: non-stream и stream перевод (chunk-последовательность, usage),
# namespaced и alias dispatch в router, capability matrix (400
# unsupported_parameter), конверт ошибок (401 → OpenAI authentication_error),
# 404 неизвестной модели. Ничего не тарифицирует, `claude` не зовёт.
#
# Запуск:  cargo build && bash tests/universal_chat_smoke.sh
# Требует: python3, curl, собранные debug-бинари target/debug/claude-api и
#          target/debug/claude-router.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BIN="${CLAUDE_API_BIN:-$ROOT/target/debug/claude-api}"
RTR="${CLAUDE_ROUTER_BIN:-$ROOT/target/debug/claude-router}"
[ -x "$BIN" ] || { echo "нет бинаря $BIN — сначала: cargo build"; exit 2; }
[ -x "$RTR" ] || { echo "нет бинаря $RTR — сначала: cargo build"; exit 2; }

DATA="$(mktemp -d)"; export SUB_CFG_DIR="$DATA"
umask 077
export SRV_LOG="$DATA/hits.log"
MOCKP=${MOCKP:-9121}; SRVP=${SRVP:-9122}; RTRP=${RTRP:-9123}
cleanup() { kill "${RTRPID:-}" "${SRV:-}" "${MOCK:-}" 2>/dev/null; rm -rf "$DATA"; }
trap cleanup EXIT

python3 "$HERE/mock_upstream.py" "$MOCKP" >"$DATA/mock.log" 2>&1 & MOCK=$!
for _ in $(seq 1 40); do
  kill -0 "$MOCK" 2>/dev/null && \
    curl -s -m1 -o /dev/null "http://127.0.0.1:$MOCKP/" && break
  sleep 0.1
done
if ! kill -0 "$MOCK" 2>/dev/null; then
  echo "mock upstream не поднялся" >&2; tail -n 80 "$DATA/mock.log" >&2; exit 2
fi

token_file="$DATA/token-a"
printf '%s\n' "faketokenaaaaaaaaaaaa" > "$token_file"
"$BIN" sub add-file "sub-a@test.io" --token-file "$token_file" >/dev/null 2>&1 || {
  echo "не удалось создать mock-подписку" >&2; exit 2; }
"$BIN" sub set-plan "sub-a@test.io" max20 >/dev/null 2>&1 || {
  echo "не удалось назначить plan mock-подписке" >&2; exit 2; }

ADMIN_KEY="universal-chat-smoke-admin-key"
CLAUDE_API_PROVIDER=anthropic \
CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=$SRVP CLAUDE_API_KEYS="$ADMIN_KEY" \
CLAUDE_API_BILLING=0 CLAUDE_API_POLL=0 CLAUDE_API_UPSTREAM="http://127.0.0.1:$MOCKP" \
CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
"$BIN" serve >"$DATA/srv.log" 2>&1 & SRV=$!
for i in $(seq 1 40); do curl -s -m1 "http://127.0.0.1:$SRVP/health" >/dev/null 2>&1 && break; sleep 0.25; done
if ! kill -0 "$SRV" 2>/dev/null || \
   ! curl -sf -m1 "http://127.0.0.1:$SRVP/health" >/dev/null; then
  echo "mock engine не поднялся" >&2; tail -n 80 "$DATA/srv.log" >&2; exit 2
fi

CLAUDE_ROUTER_HOST=127.0.0.1 CLAUDE_ROUTER_PORT=$RTRP \
CLAUDE_ROUTER_ANTHROPIC_ORIGIN="http://127.0.0.1:$SRVP" \
CLAUDE_ROUTER_OPENAI_ORIGIN="http://127.0.0.1:1" \
CLAUDE_ROUTER_GEMINI_ORIGIN="http://127.0.0.1:2" \
"$RTR" >"$DATA/rtr.log" 2>&1 & RTRPID=$!
for i in $(seq 1 40); do curl -s -m1 "http://127.0.0.1:$RTRP/health" >/dev/null 2>&1 && break; sleep 0.25; done
if ! kill -0 "$RTRPID" 2>/dev/null || \
   ! curl -sf -m1 "http://127.0.0.1:$RTRP/health" >/dev/null; then
  echo "router не поднялся" >&2; tail -n 80 "$DATA/rtr.log" >&2; exit 2
fi

ENGINE="http://127.0.0.1:$SRVP"
ROUTER="http://127.0.0.1:$RTRP"
FAIL=0
say() { echo "  $1"; }

# req <origin> <json-body> [extra curl args...] → код в stdout, тело в $RESP
RESP="$DATA/resp.body"
req() {
  local origin="$1" body="$2"; shift 2
  curl -sS -m10 -o "$RESP" -w '%{http_code}' "$@" "$origin/v1/chat/completions" \
    -H "content-type: application/json" -d "$body"
}
check_code() { # <code> <expected-code> <label>
  [ "$1" = "$2" ] || { say "✗ $3: HTTP $1 вместо $2"; FAIL=1; return 1; }
  return 0
}
check_body() { # <fixed-string> <label>
  grep -qF "$1" "$RESP" || {
    say "✗ $2: нет подстроки $1"; FAIL=1; return 1; }
  return 0
}

CHAT='{"model":"claude-haiku-4-5","messages":[{"role":"user","content":"hi"}]}'

echo "[1] engine: non-stream chat → chat.completion"
C=$(req "$ENGINE" "$CHAT" -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "non-stream" && {
  check_body '"object":"chat.completion"' "non-stream object"
  check_body '"content":"ok"' "non-stream content"
  check_body '"model":"claude-haiku-4-5-20251001"' "served model echo"
  check_body '"prompt_tokens":10' "usage prompt"
  check_body '"completion_tokens":2' "usage completion"
  check_body '"total_tokens":12' "usage total"
}

echo "[2] engine: namespaced model anthropic/… резолвится адаптером"
C=$(req "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "namespaced" && check_body '"object":"chat.completion"' "namespaced object"

echo "[3] engine: streaming → chat.completion.chunk + heartbeat + usage + [DONE]"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","stream":true,"stream_options":{"include_usage":true},"messages":[{"role":"user","content":"hi"}]}' -N -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "stream" && {
  check_body '"object":"chat.completion.chunk"' "chunk object"
  check_body '"role":"assistant"' "role chunk"
  check_body '"content":"mock"' "text delta 1"
  check_body '"content":" ok"' "text delta 2"
  check_body '"finish_reason":"stop"' "finish chunk"
  check_body '"delta":{}' "ping heartbeat chunk"
  check_body '"usage":{' "usage chunk"
  check_body '"prompt_tokens":10' "usage chunk prompt"
  check_body '"completion_tokens":2' "usage chunk completion"
  check_body 'data: [DONE]' "stream terminator"
}

echo "[4] engine: capability matrix — tools → 400 unsupported_parameter (OpenAI-конверт)"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","messages":[{"role":"user","content":"hi"}],"tools":[{"type":"function","function":{"name":"f","parameters":{}}}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "capability" && {
  check_body '"code":"unsupported_parameter"' "capability code"
  check_body '"param":"tools"' "capability param"
  check_body '"type":"invalid_request_error"' "capability type"
}

echo "[5] engine: неверный ключ → 401 в OpenAI-конверте (конвертация local_err)"
C=$(req "$ENGINE" "$CHAT" -H "x-api-key: definitely-wrong-key")
check_code "$C" 401 "bad key" && {
  check_body '"type":"authentication_error"' "bad key type"
  check_body '"code":"authentication_error"' "bad key code"
}

echo "[6] router: namespaced dispatch через полную цепочку"
C=$(req "$ROUTER" '{"model":"anthropic/claude-haiku-4-5","messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router namespaced" && check_body '"object":"chat.completion"' "router namespaced object"

echo "[7] router: alias dispatch через единый каталог"
C=$(req "$ROUTER" "$CHAT" -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router alias" && check_body '"object":"chat.completion"' "router alias object"

echo "[8] router: неизвестная модель → 404 model_not_found (OpenAI-конверт)"
C=$(req "$ROUTER" '{"model":"gpt-9","messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 404 "unknown model" && {
  check_body '"code":"model_not_found"' "unknown model code"
  check_body '"param":"model"' "unknown model param"
}

echo "[9] router: отсутствует model → 400 invalid_request_error"
C=$(req "$ROUTER" '{"messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "missing model" && check_body '"type":"invalid_request_error"' "missing model type"

if [ "$FAIL" = 0 ]; then
  echo "✓ SMOKE OK: universal chat lane (router dispatch + адаптер + стриминг + конверты)"
else
  echo "✗ SMOKE FAIL"
  echo "--- engine log ---"; tail -n 40 "$DATA/srv.log"
  echo "--- router log ---"; tail -n 20 "$DATA/rtr.log"
fi
exit $FAIL
