#!/usr/bin/env bash
# E2E smoke universal lanes — этапы 3.1–3.2 (chat) и 4.1–4.2 (responses)
# docs/engine/UNIFIED_ROUTER.md.
# Прогоняет ПОЛНУЮ цепочку без живых подписок и квоты (мок-апстрим):
#   клиент → claude-router (model-based dispatch) → claude-api (Anthropic
#   plane, chat→Messages и responses→Messages адаптеры) → mock upstream.
# Проверяет: non-stream и stream перевод (chunk-последовательность, usage),
# tools end-to-end (tool_calls/arguments-дельты, tool history round-trip),
# namespaced и alias dispatch в router, capability matrix (400
# unsupported_parameter), конверт ошибок (401 → OpenAI authentication_error),
# 404 неизвестной модели; для /v1/responses — Response object, Responses SSE
# (response.created/…/completed, ping comment), function_call items,
# store:true → 400 documented_limitation, replay tool-истории во входе (4.2:
# function_call/function_call_output items → 200, невалидные arguments → 400
# invalid_request) и reasoning summary (4.2): non-stream reasoning item +
# reasoning_tokens, stream response.reasoning_summary_* события с плотным
# output_index (reasoning=0, message=1) и дропом signature_delta.
# Ничего не тарифицирует, `claude` не зовёт.
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

echo "[4] engine: tools non-stream → message.tool_calls + finish tool_calls"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","messages":[{"role":"user","content":"weather?"}],"tools":[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}}],"tool_choice":"auto"}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "tools non-stream" && {
  check_body '"object":"chat.completion"' "tools object"
  check_body '"content":null' "tools content null"
  check_body '"id":"toolu_mock1"' "tool call id"
  check_body '"name":"get_weather"' "tool call name"
  check_body '"arguments":"{\"city\":\"Paris\"}"' "tool call arguments"
  check_body '"finish_reason":"tool_calls"' "tools finish"
  check_body '"prompt_tokens":12' "tools usage prompt"
}

echo "[5] engine: tools streaming → tool_calls-чанки + arguments-дельты"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","stream":true,"messages":[{"role":"user","content":"weather?"}],"tools":[{"type":"function","function":{"name":"get_weather"}}]}' -N -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "tools stream" && {
  check_body '"id":"toolu_mock1"' "tool start id"
  check_body '"arguments":""' "tool start empty arguments"
  check_body '"name":"get_weather"' "tool call name"
  check_body '"arguments":"{\"city\":"' "args delta 1"
  check_body '"arguments":"\"Paris\"}"' "args delta 2"
  check_body '"finish_reason":"tool_calls"' "tools stream finish"
  check_body 'data: [DONE]' "tools stream terminator"
}

echo "[6] engine: tool history (assistant tool_calls + tool results) → 200"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","messages":[{"role":"user","content":"weather?"},{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]},{"role":"tool","tool_call_id":"call_1","content":"sunny"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "tool history" && check_body '"content":"ok"' "tool history content"

echo "[7] engine: capability matrix — response_format → 400 unsupported_parameter (OpenAI-конверт)"
C=$(req "$ENGINE" '{"model":"claude-haiku-4-5","messages":[{"role":"user","content":"hi"}],"response_format":{"type":"json_object"}}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "capability" && {
  check_body '"code":"unsupported_parameter"' "capability code"
  check_body '"param":"response_format"' "capability param"
  check_body '"type":"invalid_request_error"' "capability type"
}

echo "[8] engine: неверный ключ → 401 в OpenAI-конверте (конвертация local_err)"
C=$(req "$ENGINE" "$CHAT" -H "x-api-key: definitely-wrong-key")
check_code "$C" 401 "bad key" && {
  check_body '"type":"authentication_error"' "bad key type"
  check_body '"code":"authentication_error"' "bad key code"
}

echo "[9] router: namespaced dispatch через полную цепочку"
C=$(req "$ROUTER" '{"model":"anthropic/claude-haiku-4-5","messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router namespaced" && check_body '"object":"chat.completion"' "router namespaced object"

echo "[10] router: alias dispatch через единый каталог"
C=$(req "$ROUTER" "$CHAT" -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router alias" && check_body '"object":"chat.completion"' "router alias object"

echo "[11] router: tools через полную цепочку router→engine→mock"
C=$(req "$ROUTER" '{"model":"anthropic/claude-haiku-4-5","messages":[{"role":"user","content":"weather?"}],"tools":[{"type":"function","function":{"name":"get_weather"}}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router tools" && {
  check_body '"name":"get_weather"' "router tools name"
  check_body '"finish_reason":"tool_calls"' "router tools finish"
}

echo "[12] router: неизвестная модель → 404 model_not_found (OpenAI-конверт)"
C=$(req "$ROUTER" '{"model":"gpt-9","messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 404 "unknown model" && {
  check_body '"code":"model_not_found"' "unknown model code"
  check_body '"param":"model"' "unknown model param"
}

echo "[13] router: отсутствует model → 400 invalid_request_error"
C=$(req "$ROUTER" '{"messages":[{"role":"user","content":"hi"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "missing model" && check_body '"type":"invalid_request_error"' "missing model type"

# reqr <origin> <json-body> [extra curl args...] — то же для /v1/responses (этап 4.1)
reqr() {
  local origin="$1" body="$2"; shift 2
  curl -sS -m10 -o "$RESP" -w '%{http_code}' "$@" "$origin/v1/responses" \
    -H "content-type: application/json" -d "$body"
}

RESP_HI='{"model":"anthropic/claude-haiku-4-5","input":"hi"}'

echo "[14] engine: non-stream responses → Response object + usage"
C=$(reqr "$ENGINE" "$RESP_HI" -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses non-stream" && {
  check_body '"object":"response"' "responses object"
  check_body '"status":"completed"' "responses status"
  check_body '"text":"ok"' "responses text"
  check_body '"model":"claude-haiku-4-5-20251001"' "responses served model echo"
  check_body '"input_tokens":10' "responses usage input"
  check_body '"output_tokens":2' "responses usage output"
  check_body '"total_tokens":12' "responses usage total"
}

echo "[15] engine: streaming responses → Responses SSE + ping comment + usage в completed"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","stream":true,"input":"hi"}' -N -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses stream" && {
  check_body 'event: response.created' "responses stream created"
  check_body 'event: response.in_progress' "responses stream in_progress"
  check_body 'event: response.output_item.added' "responses stream item added"
  check_body 'event: response.content_part.added' "responses stream part added"
  check_body 'event: response.output_text.delta' "responses stream text delta event"
  check_body '"delta":"mock"' "responses stream delta 1"
  check_body '"delta":" ok"' "responses stream delta 2"
  check_body 'event: response.output_text.done' "responses stream text done"
  check_body 'event: response.output_item.done' "responses stream item done"
  check_body ': ping' "responses stream ping comment"
  check_body 'event: response.completed' "responses stream completed"
  check_body '"input_tokens":10' "responses stream usage input"
  check_body '"output_tokens":2' "responses stream usage output"
}

echo "[16] engine: responses tools non-stream → function_call item"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","input":"weather?","tools":[{"type":"function","name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses tools" && {
  check_body '"type":"function_call"' "responses tools item type"
  check_body '"call_id":"toolu_mock1"' "responses tools call_id"
  check_body '"name":"get_weather"' "responses tools name"
  check_body '"arguments":"{\"city\":\"Paris\"}"' "responses tools arguments"
  check_body '"input_tokens":12' "responses tools usage input"
}

echo "[17] engine: store:true → 400 documented_limitation (stored — только openai/*)"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","input":"hi","store":true}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "responses store" && {
  check_body '"code":"documented_limitation"' "responses store code"
  check_body '"param":"store"' "responses store param"
}

echo "[18] router: namespaced responses dispatch через полную цепочку"
C=$(reqr "$ROUTER" "$RESP_HI" -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router responses namespaced" && check_body '"object":"response"' "router responses object"

echo "[19] router: alias responses dispatch через единый каталог"
C=$(reqr "$ROUTER" '{"model":"claude-haiku-4-5","input":"hi"}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "router responses alias" && check_body '"object":"response"' "router responses alias object"

echo "[20] engine: responses replay tool-истории (function_call + function_call_output) → 200"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","input":[{"type":"message","role":"user","content":"weather?"},{"type":"function_call","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"},{"type":"function_call_output","call_id":"call_1","output":"sunny"},{"type":"message","role":"user","content":"and now?"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses replay" && check_body '"object":"response"' "responses replay object"

echo "[21] engine: невалидные arguments function_call → 400 invalid_request"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","input":[{"type":"message","role":"user","content":"hi"},{"type":"function_call","call_id":"call_1","name":"get_weather","arguments":"not json"}]}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 400 "responses bad arguments" && {
  check_body '"type":"invalid_request_error"' "bad arguments type"
  check_body '"param":"input"' "bad arguments param"
}

echo "[22] engine: Claude 4.5 reasoning hint деградирует к model default без upstream 400"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-haiku-4-5","input":"hi","reasoning":{"effort":"low"}}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses legacy reasoning hint" && {
  check_body '"object":"response"' "legacy reasoning response"
  grep -qF '"type":"reasoning"' "$RESP" && { say "✗ legacy hint неожиданно включил reasoning"; FAIL=1; }
}

echo "[23] engine: reasoning non-stream → reasoning item + message item + reasoning_tokens"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-opus-4-8","input":"hi","reasoning":{"effort":"low"}}' -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses reasoning" && {
  check_body '"type":"reasoning"' "reasoning item type"
  check_body '"id":"rs_' "reasoning item id"
  check_body '"type":"summary_text"' "summary part type"
  check_body '"text":"mock think full"' "summary text"
  check_body '"type":"message"' "message item present"
  check_body '"reasoning_tokens":5' "reasoning tokens"
  grep -qF "sig_mock" "$RESP" && { say "✗ signature утекла в non-stream ответ"; FAIL=1; }
}

echo "[24] engine: reasoning stream → reasoning_summary события + output_index reasoning=0/message=1"
C=$(reqr "$ENGINE" '{"model":"anthropic/claude-opus-4-8","stream":true,"input":"hi","reasoning":{"effort":"low"}}' -N -H "x-api-key: $ADMIN_KEY")
check_code "$C" 200 "responses reasoning stream" && {
  check_body 'event: response.reasoning_summary_part.added' "reasoning part added"
  check_body 'event: response.reasoning_summary_text.delta' "reasoning text delta"
  check_body '"delta":"mock think 1"' "reasoning delta 1"
  check_body '"delta":" mock think 2"' "reasoning delta 2"
  check_body 'event: response.reasoning_summary_text.done' "reasoning text done"
  check_body '"text":"mock think 1 mock think 2"' "reasoning done text"
  check_body 'event: response.reasoning_summary_part.done' "reasoning part done"
  check_body '"id":"rs_' "reasoning item id"
  check_body '"reasoning_tokens":5' "stream reasoning tokens"
  grep -qF "sig_mock" "$RESP" && { say "✗ signature_delta не дропнута"; FAIL=1; }
  python3 - "$RESP" <<'PY' || { say "✗ output_index reasoning=0/message=1"; FAIL=1; }
import json, sys
added = []
for frame in open(sys.argv[1]).read().split("\n\n"):
    event, data = None, None
    for line in frame.split("\n"):
        if line.startswith("event:"):
            event = line[6:].strip()
        elif line.startswith("data:"):
            data = line[5:].strip()
    if event == "response.output_item.added":
        added.append(json.loads(data))
assert [a["output_index"] for a in added] == [0, 1], added
assert added[0]["item"]["type"] == "reasoning", added
assert added[1]["item"]["type"] == "message", added
PY
}

if [ "$FAIL" = 0 ]; then
  echo "✓ SMOKE OK: universal lanes (chat + responses: router dispatch + адаптеры + стриминг + конверты)"
else
  echo "✗ SMOKE FAIL"
  echo "--- engine log ---"; tail -n 40 "$DATA/srv.log"
  echo "--- router log ---"; tail -n 20 "$DATA/rtr.log"
fi
exit $FAIL
