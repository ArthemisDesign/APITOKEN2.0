#!/usr/bin/env bash
# refresh-fingerprint.sh — держать значения прикладного слоя АКТУАЛЬНЫМИ.
#
# Снимает с ЖИВОГО claude CLI (он сам обновляется) реальные UA / anthropic-version /
# oauth-бету / identity-блок и пишет их в config.env форвардинга. Так наши инжектируемые
# значения не протухают при обновлениях Claude Code (в этом и надёжность).
#
# Запускается таймером (claude-api-fingerprint.timer) под root; claude гоняем под agents.
# Нужен ВАЛИДНЫЙ токен подписки из пула (с фиктивным claude запрос не шлёт). Пул пуст —
# просто выходим, оставляя текущие значения.
set -uo pipefail
CFG_DIR="${SUB_CFG_DIR:-/srv/claude-api/data}"
DB="$CFG_DIR/subscriptions.db"
CLAUDE="${CLAUDE_BIN:-/srv/agents/.local/bin/claude}"
CONFIG_ENV="$CFG_DIR/config.env"
RUN_USER="${AUTH_BOT_USER:-agents}"
PORT="${FP_PORT:-19878}"

# токен + прокси из живого пула
creds="$(python3 - "$DB" <<'PY'
import sqlite3, sys, os
db = sys.argv[1]
try:
    c = sqlite3.connect(db)
    cols = {r[1] for r in c.execute("PRAGMA table_info(subs)")}   # устойчиво к обеим схемам
    tok_sel = "token" if "token" in cols else "'' AS token"
    for email, tok, tf, proxy in c.execute(
            f"SELECT email, {tok_sel}, token_file, proxy FROM subs WHERE COALESCE(status,'active')='active'"):
        t = (tok or "").strip()
        if not t and tf and os.path.exists(tf):
            t = open(tf).read().strip()
        if t:
            print(t + "\t" + (proxy or "")); break
except Exception:
    pass
PY
)"
TOK="${creds%%$'\t'*}"
PROXY="${creds#*$'\t'}"; [ "$PROXY" = "$creds" ] && PROXY=""
[ -n "$TOK" ] || { echo "нет активной подписки с токеном — рефреш пропущен (значения без изменений)"; exit 0; }

LOG="$(mktemp)"; TMPCFG="$(mktemp -d)"; echo '{"theme":"dark"}' > "$TMPCFG/settings.json"
chown -R "$RUN_USER":"$RUN_USER" "$TMPCFG" 2>/dev/null || true

# логгер (ловит первый запрос, печатает нужные поля)
python3 - "$PORT" > "$LOG" 2>&1 <<'PY' &
import sys, json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0)); body = self.rfile.read(n) if n else b''
        ident = ""
        try:
            for b in (json.loads(body).get('system') or []):
                t = b.get('text', '') if isinstance(b, dict) else ''
                if t.startswith('You are'): ident = t; break
        except Exception: pass
        print("UA=" + (self.headers.get('User-Agent') or ''))
        print("VER=" + (self.headers.get('anthropic-version') or ''))
        print("BETA=" + (self.headers.get('anthropic-beta') or ''))
        print("IDENT=" + ident)
        self.send_response(200); self.send_header('content-type', 'application/json'); self.end_headers()
        self.wfile.write(b'{"id":"x","type":"message","role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}')
    def do_GET(self):
        self.send_response(200); self.send_header('content-type', 'application/json'); self.end_headers()
        self.wfile.write(b'{"data":[]}')
srv = ThreadingHTTPServer(('127.0.0.1', int(sys.argv[1])), H); srv.timeout = 25; srv.handle_request()
PY
LPID=$!
sleep 1
# claude под agents → локальный логгер, БЕЗ прокси (localhost). Работает для токенов,
# валидируемых с IP сервера; для строго IP-locked захват identity/beta не выйдет — не беда,
# UA всё равно возьмём из --version, а identity/beta останутся текущими (не сломается).
runuser -u "$RUN_USER" -- env CLAUDE_CODE_OAUTH_TOKEN="$TOK" \
    ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT" CLAUDE_CONFIG_DIR="$TMPCFG" \
    timeout 25 "$CLAUDE" -p "hi" --model haiku >/dev/null 2>&1 || true
sleep 1
kill "$LPID" 2>/dev/null || true

UA="$(sed -n 's/^UA=//p' "$LOG" | head -1)"
VER="$(sed -n 's/^VER=//p' "$LOG" | head -1)"
BETA_RAW="$(sed -n 's/^BETA=//p' "$LOG" | head -1)"
IDENT="$(sed -n 's/^IDENT=//p' "$LOG" | head -1)"
rm -rf "$LOG" "$TMPCFG"
BETA="$(printf '%s' "$BETA_RAW" | tr ',' '\n' | grep -i oauth | paste -sd, -)"   # только oauth-бета

# UA — НАДЁЖНО из версии установленного claude (не требует токена/сети)
CLI_VER="$("$CLAUDE" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
[ -n "$UA" ] || { [ -n "$CLI_VER" ] && UA="claude-cli/$CLI_VER (external, sdk-cli)"; }

[ -n "$UA" ] || { echo "не удалось определить даже UA — значения без изменений"; exit 0; }

touch "$CONFIG_ENV"
set_kv() { grep -v "^$1=" "$CONFIG_ENV" > "$CONFIG_ENV.tmp" 2>/dev/null || true; echo "$1=$2" >> "$CONFIG_ENV.tmp"; mv "$CONFIG_ENV.tmp" "$CONFIG_ENV"; }
set_kv CLAUDE_API_UA "$UA"                                    # всегда актуально
[ -n "$VER" ]   && set_kv CLAUDE_API_ANTHROPIC_VERSION "$VER" # best-effort (если захват удался)
[ -n "$BETA" ]  && set_kv CLAUDE_API_BETA "$BETA"
[ -n "$IDENT" ] && set_kv CLAUDE_API_IDENTITY "$IDENT"
chown "$RUN_USER":"$RUN_USER" "$CONFIG_ENV" 2>/dev/null || true
echo "актуализировано: UA='$UA' VER='${VER:-(деф)}' BETA='${BETA:-(деф)}' IDENT='${IDENT:+снят}${IDENT:-(деф)}'"
systemctl restart claude-api 2>/dev/null && echo "claude-api перезапущен" \
    || echo "(claude-api не как сервис — применится при следующем старте)"
