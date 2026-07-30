#!/usr/bin/env bash
# Smoke-валидация ротации/веера по N подпискам через МОК-апстрим — БЕЗ живых аккаунтов и квоты.
# Проверяет ГЛАВНЫЙ инвариант: distinct-сессии распределяются по всему флоту (нет кластеров),
# а одна сессия липнет к одной персоне (cache-first пин). Ничего не тарифицирует, `claude` не зовёт.
#
# Запуск:  cargo build && bash tests/rotation_fanout_smoke.sh
# Требует: python3, curl, собранный debug-бинарь target/debug/claude-api.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BIN="${CLAUDE_API_BIN:-$ROOT/target/debug/claude-api}"
[ -x "$BIN" ] || { echo "нет бинаря $BIN — сначала: cargo build"; exit 2; }

DATA="$(mktemp -d)"; export SUB_CFG_DIR="$DATA"
umask 077
export SRV_LOG="$DATA/hits.log"
MOCKP=${MOCKP:-9099}; SRVP=${SRVP:-9797}
cleanup() { kill "${SRV:-}" "${MOCK:-}" 2>/dev/null; rm -rf "$DATA"; }
trap cleanup EXIT

python3 "$HERE/mock_upstream.py" "$MOCKP" >"$DATA/mock.log" 2>&1 & MOCK=$!
for _ in $(seq 1 40); do
  kill -0 "$MOCK" 2>/dev/null && \
    curl -s -m1 -o /dev/null "http://127.0.0.1:$MOCKP/" && break
  sleep 0.1
done
if ! kill -0 "$MOCK" 2>/dev/null || \
   ! curl -sS -m1 -o /dev/null "http://127.0.0.1:$MOCKP/"; then
  echo "mock upstream не поднялся" >&2
  tail -n 80 "$DATA/mock.log" >&2
  exit 2
fi

N=5
ADMIN_KEY="rotation-smoke-admin-key-only"
for s in a b c d e; do
  token_file="$DATA/token-$s"
  printf '%s\n' "faketoken${s}${s}${s}${s}${s}${s}" > "$token_file"
  "$BIN" sub add-file "sub-$s@test.io" --token-file "$token_file" >/dev/null 2>&1 || {
    echo "не удалось создать mock-подписку sub-$s@test.io" >&2
    exit 2
  }
  "$BIN" sub set-plan "sub-$s@test.io" max20 >/dev/null 2>&1 || {
    echo "не удалось назначить plan mock-подписке sub-$s@test.io" >&2
    exit 2
  }
done

CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=$SRVP CLAUDE_API_KEYS="$ADMIN_KEY" \
CLAUDE_API_BILLING=0 CLAUDE_API_POLL=0 CLAUDE_API_UPSTREAM="http://127.0.0.1:$MOCKP" \
CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1 \
"$BIN" serve >"$DATA/srv.log" 2>&1 & SRV=$!
for i in $(seq 1 40); do curl -s -m1 "http://127.0.0.1:$SRVP/health" >/dev/null 2>&1 && break; sleep 0.25; done
if ! kill -0 "$SRV" 2>/dev/null || \
   ! curl -sf -m1 "http://127.0.0.1:$SRVP/health" >/dev/null; then
  echo "mock engine не поднялся" >&2
  tail -n 80 "$DATA/srv.log" >&2
  exit 2
fi

req() { curl -sS -m5 -o /dev/null -w "%{http_code}" "http://127.0.0.1:$SRVP/v1/messages" \
  -H "x-api-key: $ADMIN_KEY" -H "anthropic-version: 2023-06-01" -H "content-type: application/json" \
  -d "{\"model\":\"claude-haiku-4-5-20251001\",\"max_tokens\":8,\"system\":\"$1\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}"; }

FAIL=0

probe_code=$(req "smoke-probe")
if [ "$probe_code" != "200" ]; then
  echo "mock request вернул HTTP $probe_code вместо 200" >&2
  tail -n 80 "$DATA/srv.log" >&2
  exit 2
fi

# 1) 50 РАЗНЫХ сессий → ровный веер по флоту
: > "$SRV_LOG"; for n in $(seq 1 50); do req "distinct-session-$n" >/dev/null; done
D=$(sort "$SRV_LOG" | uniq | wc -l | tr -d ' ')
TOP=$(sort "$SRV_LOG" | uniq -c | sort -rn | head -1 | awk '{print $1}')
echo "50 distinct → подписок $D/$N, макс доля $TOP/50"; sort "$SRV_LOG" | uniq -c | sort -rn | sed 's/^/    /'
[ "$D" -ge 4 ] && [ "$TOP" -le 18 ] || { echo "✗ веер слабый/кластер"; FAIL=1; }

# 2) 60 ПАРАЛЛЕЛЬНЫХ (наплыв) → конверт MAX_INFLIGHT раскидывает
: > "$SRV_LOG"; pids=()
for n in $(seq 1 60); do req "burst-$n" >/dev/null & pids+=($!); done
wait "${pids[@]}"
BD=$(sort "$SRV_LOG" | uniq | wc -l | tr -d ' ')
BTOP=$(sort "$SRV_LOG" | uniq -c | sort -rn | head -1 | awk '{print $1}')
echo "60 параллельных → подписок $BD/$N, макс доля $BTOP/60"
[ "$BD" -ge 4 ] && [ "$BTOP" -le 20 ] || { echo "✗ наплыв кластеризуется"; FAIL=1; }

# 3) ПИН: 12 одинаковых сессий → одна персона
: > "$SRV_LOG"; for n in $(seq 1 12); do req "same-pinned-session" >/dev/null; done
P=$(sort "$SRV_LOG" | uniq | wc -l | tr -d ' ')
echo "12 одинаковых → подписок $P (ожидаем 1)"
[ "$P" = "1" ] || { echo "✗ пин сломан"; FAIL=1; }

if [ "$FAIL" = 0 ]; then echo "✓ SMOKE OK: ровный веер + пин на тёплый кэш"; else echo "✗ SMOKE FAIL"; fi
exit $FAIL
