#!/usr/bin/env bash
# refresh-fingerprint.sh — держать значения прикладного слоя АКТУАЛЬНЫМИ.
#
# Снимает с ЖИВОГО claude CLI (он сам обновляется) РЕАЛЬНЫЙ исходящий /v1/messages запрос
# (UA / anthropic-version / полный anthropic-beta / identity / x-app / x-stainless-* / billing
# cc_version) и пишет их в config.env форвардинга. Так наши инжектируемые значения не протухают
# при обновлениях Claude Code (в этом и надёжность).
#
# КАК СНИМАЕМ (важно): claude шлёт запрос по HTTPS к api.anthropic.com — plain-HTTP localhost-логгер
# его НЕ видит (claude не шлёт реальный запрос на http:// base-url). Поэтому терминируем TLS через
# mitmdump (свой CA, доверенный claude'у) в direct-mode: claude → mitm(127.0.0.1) → anthropic. Даже
# если ответ 401 по IP-lock — РЕКВЕСТ (заголовки+тело) уже захвачен (нам нужен только он). Проверено:
# с IP сервера подписка отдаёт 200, захват чистый.
#
# Dormant operator utility: production timer remains disabled. When an operator invokes it, claude
# runs as agents and needs one valid pool token. An empty pool or unavailable mitm leaves values
# unchanged. The script stores config only; blue-green activation belongs to the watchdog.
set -uo pipefail
CFG_DIR="${SUB_CFG_DIR:-/srv/claude-api/data}"
DB="$CFG_DIR/subscriptions.db"
CLAUDE="${CLAUDE_BIN:-/srv/agents/.local/bin/claude}"
CONFIG_ENV="${FP_CONFIG_ENV:-$CFG_DIR/config.env}"   # FP_CONFIG_ENV — для dry-run в temp-файл
RUN_USER="${AUTH_BOT_USER:-agents}"
PORT="${FP_PORT:-19878}"

# токен из живого пула (прокси НЕ нужен: direct-mode с IP сервера работает; residential-прокси только
# усложнил бы mitm upstream-авторизацией)
TOK="$(python3 - "$DB" <<'PY'
import sqlite3, sys, os
db = sys.argv[1]
try:
    c = sqlite3.connect(db)
    cols = {r[1] for r in c.execute("PRAGMA table_info(subs)")}   # устойчиво к обеим схемам
    tok_sel = "token" if "token" in cols else "'' AS token"
    for email, tok, tf in c.execute(
            f"SELECT email, {tok_sel}, token_file FROM subs WHERE COALESCE(status,'active')='active'"):
        t = (tok or "").strip()
        if not t and tf and os.path.exists(tf):
            t = open(tf).read().strip()
        if t:
            print(t); break
except Exception:
    pass
PY
)"
[ -n "$TOK" ] || { echo "нет активной подписки с токеном — рефреш пропущен (значения без изменений)"; exit 0; }

# UA — НАДЁЖНО из версии установленного claude (не требует токена/сети); служит fallback'ом
CLI_VER="$("$CLAUDE" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"

# --- сетевой захват реального запроса через mitmdump (best-effort) ---
CAP="$(mktemp)"; : > "$CAP"
UA="" VER="" BETA="" IDENT="" XAPP="" SL_LANG="" SL_RUNTIME="" SL_RTVER="" SL_PKGVER="" SL_OS="" SL_ARCH="" CCVER=""
if ! command -v mitmdump >/dev/null 2>&1; then
    pip3 install --break-system-packages -q mitmproxy >/dev/null 2>&1 \
        || apt-get install -y mitmproxy >/dev/null 2>&1 || true
fi
if command -v mitmdump >/dev/null 2>&1; then
    ADDON="$(mktemp --suffix=.py)"
    cat > "$ADDON" <<PY
from mitmproxy import http
def request(flow: http.HTTPFlow):
    p = flow.request.path
    if flow.request.method == "POST" and "messages" in p and "count_tokens" not in p:
        with open("$CAP", "a") as f:
            f.write("### H\n")
            for k, v in flow.request.headers.items():
                f.write("%s: %s\n" % (k, v))
            f.write("### B\n%s\n### E\n" % flow.request.get_text()[:200000])
PY
    mitmdump -q --listen-host 127.0.0.1 --listen-port "$PORT" -s "$ADDON" >/tmp/fp_mitm.log 2>&1 &
    MPID=$!; sleep 4
    CA_SRC="$(find /root/.mitmproxy "$HOME/.mitmproxy" -name mitmproxy-ca-cert.pem 2>/dev/null | head -1)"
    CA=/tmp/fp_mitmca.pem
    if [ -n "$CA_SRC" ] && cp "$CA_SRC" "$CA" 2>/dev/null; then
        chmod 644 "$CA"
        TMPCFG="$(mktemp -d)"; echo '{"theme":"dark","hasCompletedOnboarding":true}' > "$TMPCFG/settings.json"
        chown -R "$RUN_USER":"$RUN_USER" "$TMPCFG" 2>/dev/null || true
        runuser -u "$RUN_USER" -- env CLAUDE_CODE_OAUTH_TOKEN="$TOK" CLAUDE_CONFIG_DIR="$TMPCFG" \
            HTTPS_PROXY="http://127.0.0.1:$PORT" HTTP_PROXY="http://127.0.0.1:$PORT" \
            NODE_EXTRA_CA_CERTS="$CA" SSL_CERT_FILE="$CA" \
            timeout 45 "$CLAUDE" -p "hi" --model haiku >/dev/null 2>&1 || true
        rm -rf "$TMPCFG"
    fi
    kill "$MPID" 2>/dev/null || true
    rm -f "$ADDON" "$CA" 2>/dev/null || true

    # разбор захвата (первый /v1/messages)
    if [ -s "$CAP" ]; then
        hdr() { sed -n '/^### H$/,/^### B$/p' "$CAP" | grep -iE "^$1:" | head -1 | sed -E "s/^[^:]+:[[:space:]]*//"; }
        UA="$(hdr 'User-Agent')"
        VER="$(hdr 'anthropic-version')"
        BETA="$(hdr 'anthropic-beta')"          # ВЕСЬ реальный набор (порядок=Set-итерация, важен НАБОР)
        XAPP="$(hdr 'x-app')"
        SL_LANG="$(hdr 'x-stainless-lang')"
        SL_RUNTIME="$(hdr 'x-stainless-runtime')"
        SL_RTVER="$(hdr 'x-stainless-runtime-version')"
        SL_PKGVER="$(hdr 'x-stainless-package-version')"
        SL_OS="$(hdr 'x-stainless-os')"
        SL_ARCH="$(hdr 'x-stainless-arch')"
        # тело: identity (system-блок "You are ...") и billing cc_version
        BODY="$(sed -n '/^### B$/,/^### E$/p' "$CAP")"
        IDENT="$(printf '%s' "$BODY" | grep -oE '"text":"You are [^"]*"' | head -1 | sed -E 's/^"text":"//; s/"$//')"
        # Preserve the COMPLETE value from one real request. Releases use several suffix forms
        # (`.d49`, `.a6e`, `.408`, `.0f1`); stripping one guessed form and appending another creates
        # a version that never existed.
        CCVER="$(printf '%s' "$BODY" | grep -oE 'cc_version=[^;]+' | head -1 | sed -E 's/^cc_version=//')"
    fi
fi
rm -f "$CAP" 2>/dev/null || true

# UA fallback из --version, если сетевой захват не дал
[ -n "$UA" ] || { [ -n "$CLI_VER" ] && UA="claude-cli/$CLI_VER (external, sdk-cli)"; }
[ -n "$UA" ] || { echo "не удалось определить даже UA — значения без изменений"; exit 0; }

touch "$CONFIG_ENV"
NEXT="$CONFIG_ENV.tmp.$$"
cp "$CONFIG_ENV" "$NEXT"
set_kv() {
    local key=$1 value=$2 scratch="$NEXT.row"
    grep -v "^$key=" "$NEXT" > "$scratch" 2>/dev/null || true
    printf '%s=%s\n' "$key" "$value" >> "$scratch"
    mv "$scratch" "$NEXT"
}
set_kv CLAUDE_API_UA "$UA"                                    # всегда актуально (из --version как минимум)
[ -n "$VER" ]      && set_kv CLAUDE_API_ANTHROPIC_VERSION "$VER"   # ниже — best-effort (если захват удался)
[ -n "$BETA" ]     && set_kv CLAUDE_API_BETA "$BETA"
[ -n "$IDENT" ]    && set_kv CLAUDE_API_IDENTITY "$IDENT"
[ -n "$XAPP" ]     && set_kv CLAUDE_API_X_APP "$XAPP"
[ -n "$SL_LANG" ]  && set_kv CLAUDE_API_SL_LANG "$SL_LANG"
[ -n "$SL_RUNTIME" ] && set_kv CLAUDE_API_SL_RUNTIME "$SL_RUNTIME"
[ -n "$SL_RTVER" ] && set_kv CLAUDE_API_SL_RT_VER "$SL_RTVER"
[ -n "$SL_PKGVER" ] && set_kv CLAUDE_API_SL_PKG_VER "$SL_PKGVER"
[ -n "$SL_OS" ]    && set_kv CLAUDE_API_SL_OS "$SL_OS"
[ -n "$SL_ARCH" ]  && set_kv CLAUDE_API_SL_ARCH "$SL_ARCH"
[ -n "$CCVER" ]    && set_kv CLAUDE_API_CC_VERSION "$CCVER"        # complete captured billing cc_version
mv "$NEXT" "$CONFIG_ENV"
chown "$RUN_USER":"$RUN_USER" "$CONFIG_ENV" 2>/dev/null || true
echo "актуализировано: UA='$UA' BETA='${BETA:+set}' XAPP='${XAPP:-(деф)}' SL_PKG='${SL_PKGVER:-(деф)}' SL_RTVER='${SL_RTVER:-(деф)}' CCVER='${CCVER:-(деф)}'"
echo "fingerprint сохранён; активация принадлежит reviewed blue-green rollout"
