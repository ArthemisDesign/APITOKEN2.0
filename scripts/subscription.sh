#!/usr/bin/env bash
# subscription.sh — реестр Claude-подписок и их авторизация через прокси.
# ИДЕНТИФИКАТОР подписки = email аккаунта (ничего придумывать не надо). Каталог
# профиля выводится из email: me@x.com → ~/.claude-me-x-com. Реестр (НЕ в репо)
# ключуется по email. При `activate` активная подписка проецируется в два плоских
# файла, которые читают box_docker.sh и движок:
#   active_profile  — каталог профиля (его .credentials.json сидится в коробки)
#   active_proxy    — http://user:pass@host:port (идёт в коробку как HTTPS_PROXY)
#
# Метод логина — docs/CLAUDE_AUTH_PROFILES.md §6 (claude auth login + FIFO,
# code#state). Прокси задаётся ПРИ авторизации → аккаунт логинится и работает с
# одного IP. Формат прокси на входе: ip:port:user:pass | ip:port | http(s)://…
#
# Команды (везде <email> = идентификатор):
#   add        <email> <proxy>            интерактивно: auth-start + ввод code#state + finish
#   auth-start <email> [proxy]            запустить логин, напечатать OAuth-URL (proxy опускается
#                                         при relogin — берётся из реестра)
#   auth-finish <email> "<code#state>"    докормить код, проверить, записать активной
#   relogin    <email>                    = auth-start с сохранённым proxy (интерактивно)
#   activate   <email>                    сделать активной (пишет проекции)
#   set-proxy  <email> <proxy>            сменить прокси подписки
#   refresh    [email]                    прогрев профиля (держит токен живым); дефолт — активная
#   ping       <email>                    проверить живость (через её прокси)
#   list                                  список подписок
set -uo pipefail

die()  { echo "SUB_FAIL: $*" >&2; exit 1; }
warn() { echo "SUB_WARN: $*" >&2; }
ok()   { echo "SUB_OK: $*"; }

CFG="${SUB_CFG_DIR:-$HOME/.config/personal-agents}"
REG="$CFG/subscriptions.json"          # JSON-ЗЕРКАЛО (фолбэк/совместимость), НЕ источник истины
SUBS_DB="${SUBS_DB:-$CFG/subscriptions.db}"   # ИСТОЧНИК ИСТИНЫ реестра — общая SQLite
ACTIVE_PROFILE_F="$CFG/active_profile"
ACTIVE_PROXY_F="$CFG/active_proxy"
CLAUDE="${CLAUDE_BIN:-$HOME/.local/bin/claude}"
PING_MODEL="${SUB_PING_MODEL:-claude-haiku-4-5-20251001}"
mkdir -p "$CFG"

# email → фс-безопасный слаг (для имени каталога/файлов): me@x.com → me-x-com
_slug() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9' '-' | sed 's/-\{1,\}/-/g; s/^-//; s/-$//'; }
_dir_of()  { echo "$HOME/.claude-$(_slug "$1")"; }
_fifo_of() { echo "/tmp/claude-auth-$(_slug "$1").fifo"; }
_log_of()  { echo "/tmp/claude-auth-$(_slug "$1").log"; }

# ip:port:user:pass | ip:port | http(s)://… → http-URL (или пусто)
proxy_url() {
  local raw="${1:-}"; [ -n "$raw" ] || { echo ""; return 0; }
  case "$raw" in http://*|https://*) echo "$raw"; return 0;; esac
  local ip port user pass; IFS=':' read -r ip port user pass <<<"$raw"
  if [ -n "${user:-}" ] && [ -n "${pass:-}" ]; then echo "http://$user:$pass@$ip:$port"
  elif [ -n "${port:-}" ]; then echo "http://$ip:$port"
  else echo ""; fi
}

# РЕЕСТР ПОДПИСОК — ИСТОЧНИК ИСТИНЫ: общая SQLite (subscriptions.db). JSON рядом —
# ЗЕРКАЛО (перезаписывается на каждую мутацию) для обратной совместимости и фолбэка.
# Всё через python3 (sqlite3 в stdlib — CLI sqlite3 на хосте нет). Ключ = email.
_reg() {
  python3 - "$SUBS_DB" "$REG" "$ACTIVE_PROFILE_F" "$ACTIVE_PROXY_F" "$@" <<'PY'
import json, sys, os, time, sqlite3
db_p, reg_p, apf, axf, op = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]
args = sys.argv[6:]

# Колонки строки подписки (порядок фиксирован — совпадает со схемой в движке db.rs).
COLS = ["email","kind","profile_dir","token_file","proxy","plan","status",
        "source","fleet","seller","seller_id","seller_nick","refresher","added_ts","added"]
DEFAULTS = {"kind":"token","profile_dir":"","token_file":"","proxy":"","plan":"",
            "status":"active","source":"","fleet":"prod","seller":"","seller_id":"",
            "seller_nick":"","refresher":1,"added_ts":0,"added":""}

def connect():
    os.makedirs(os.path.dirname(db_p), exist_ok=True)
    c = sqlite3.connect(db_p, timeout=10)
    c.row_factory = sqlite3.Row
    c.execute("PRAGMA journal_mode=WAL"); c.execute("PRAGMA busy_timeout=5000")
    c.execute("""CREATE TABLE IF NOT EXISTS subs(
        email TEXT PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'token',
        profile_dir TEXT NOT NULL DEFAULT '', token_file TEXT NOT NULL DEFAULT '',
        proxy TEXT NOT NULL DEFAULT '', plan TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'active', source TEXT NOT NULL DEFAULT '',
        fleet TEXT NOT NULL DEFAULT 'prod', seller TEXT NOT NULL DEFAULT '',
        seller_id TEXT NOT NULL DEFAULT '', seller_nick TEXT NOT NULL DEFAULT '',
        refresher INTEGER NOT NULL DEFAULT 1,
        added_ts INTEGER NOT NULL DEFAULT 0, added TEXT NOT NULL DEFAULT '')""")
    c.execute("CREATE TABLE IF NOT EXISTS subs_meta(k TEXT PRIMARY KEY, v TEXT NOT NULL DEFAULT '')")
    # мягкая миграция старых БД: доливаем недостающие колонки (duplicate — гасим)
    for col in COLS:
        try: c.execute(f"ALTER TABLE subs ADD COLUMN {col} TEXT")
        except sqlite3.OperationalError: pass
    migrate_from_json(c)
    return c

def migrate_from_json(c):
    """Одноразовый импорт старого subscriptions.json → таблицу (если таблица пуста)."""
    n = c.execute("SELECT COUNT(*) FROM subs").fetchone()[0]
    if n: return
    try:
        with open(reg_p) as f: d = json.load(f)
    except Exception:
        return
    subs = d.get("subs", {})
    if not subs: return
    for email, s in subs.items():
        row = dict(DEFAULTS); row.update({k: s.get(k) for k in COLS if s.get(k) is not None})
        row["email"] = email
        row["refresher"] = 1 if s.get("refresher", True) else 0
        upsert_row(c, row)
    if d.get("active"):
        c.execute("INSERT OR REPLACE INTO subs_meta(k,v) VALUES('active',?)", (d["active"],))
    c.commit()

def upsert_row(c, row):
    row = {**DEFAULTS, **row}
    cols = ",".join(COLS); ph = ",".join("?" for _ in COLS)
    sets = ",".join(f"{k}=excluded.{k}" for k in COLS if k != "email")
    c.execute(f"INSERT INTO subs({cols}) VALUES({ph}) "
              f"ON CONFLICT(email) DO UPDATE SET {sets}",
              [row.get(k) for k in COLS])

def get_row(c, email):
    r = c.execute("SELECT * FROM subs WHERE email=?", (email,)).fetchone()
    return dict(r) if r else None

def stamp(row):
    if not row.get("added_ts"):
        ts = int(time.time()); row["added_ts"] = ts
        row["added"] = time.strftime("%Y-%m-%d %H:%M", time.gmtime(ts))

def active(c):
    r = c.execute("SELECT v FROM subs_meta WHERE k='active'").fetchone()
    return r[0] if r else ""

def set_active(c, email):
    c.execute("INSERT OR REPLACE INTO subs_meta(k,v) VALUES('active',?)", (email,))

def mirror_json(c):
    """Перезаписать JSON-зеркало из БД (атомарно). Форма = как исторический реестр."""
    subs = {}
    for r in c.execute("SELECT * FROM subs").fetchall():
        d = dict(r); d["refresher"] = bool(d.get("refresher", 1)); subs[d["email"]] = d
    out = {"active": active(c), "subs": subs}
    os.makedirs(os.path.dirname(reg_p), exist_ok=True)
    t = reg_p + ".tmp"; json.dump(out, open(t, "w"), ensure_ascii=False, indent=2); os.replace(t, reg_p)

c = connect()
if op == "upsert":            # login-подписка (claude auth login → .credentials.json)
    email, dir_, proxy, status = args
    row = get_row(c, email) or {"email": email, "kind": "login", "source": "addsub"}
    row.update({"email": email, "profile_dir": dir_, "proxy": proxy, "status": status})
    row.setdefault("kind", "login"); row.setdefault("source", "addsub")
    stamp(row); upsert_row(c, row); c.commit(); mirror_json(c)
elif op == "regtoken":        # токен-подписка (claude setup-token → long-lived токен)
    email, dir_, proxy, source = args[0:4]
    fleet  = args[4] if len(args) > 4 and args[4] else "prod"
    seller = args[5] if len(args) > 5 else ""
    seller_id = args[6] if len(args) > 6 else ""
    seller_nick = args[7] if len(args) > 7 else ""
    row = get_row(c, email) or {"email": email}
    row.update({"email": email, "kind": "token", "profile_dir": dir_,
                "token_file": os.path.join(dir_, "oauth_token"), "proxy": proxy,
                "status": "active", "source": source or "auth_bot", "fleet": fleet})
    # продавца/атрибуцию пишем ТОЛЬКО если переданы (не затираем при перерегистрации)
    if seller: row["seller"] = seller
    if seller_id: row["seller_id"] = seller_id
    if seller_nick: row["seller_nick"] = seller_nick
    stamp(row); upsert_row(c, row); c.commit(); mirror_json(c)
elif op == "set-proxy":
    email, proxy = args
    if not get_row(c, email): sys.exit("no such sub")
    c.execute("UPDATE subs SET proxy=? WHERE email=?", (proxy, email)); c.commit(); mirror_json(c)
elif op == "set-status":
    email, status = args
    if not get_row(c, email): sys.exit("no such sub")
    c.execute("UPDATE subs SET status=? WHERE email=?", (status, email)); c.commit(); mirror_json(c)
elif op == "set-plan":        # тип тарифа: pro | max5 | max20 (или "" — снять)
    email, plan = args
    if not get_row(c, email): sys.exit("no such sub")
    c.execute("UPDATE subs SET plan=? WHERE email=?", (plan, email)); c.commit(); mirror_json(c)
elif op == "set-fleet":       # флот подписки: dev | prod (DevStand vs Test+Main)
    email, fleet = args
    if not get_row(c, email): sys.exit("no such sub")
    c.execute("UPDATE subs SET fleet=? WHERE email=?", (fleet, email)); c.commit(); mirror_json(c)
elif op == "set-seller":      # продавец: <seller_username> [seller_id] [seller_nick]
    email = args[0]; seller = args[1] if len(args) > 1 else ""
    seller_id = args[2] if len(args) > 2 else ""
    seller_nick = args[3] if len(args) > 3 else ""
    if not get_row(c, email): sys.exit("no such sub")
    c.execute("UPDATE subs SET seller=?, seller_id=?, seller_nick=? WHERE email=?",
              (seller, seller_id, seller_nick, email))
    c.commit(); mirror_json(c)
elif op == "activate":
    email = args[0]
    row = get_row(c, email)
    if not row: sys.exit("no such sub")
    set_active(c, email); c.commit(); mirror_json(c)
    open(apf, "w").write((row.get("profile_dir") or "") + "\n")
    open(axf, "w").write((row.get("proxy") or "") + "\n")
elif op == "get":
    email, field = args; row = get_row(c, email) or {}
    print(row.get(field, "") or "")
elif op == "active":
    print(active(c) or "")
elif op == "list":
    a = active(c)
    rows = c.execute("SELECT * FROM subs ORDER BY added_ts").fetchall()
    if not rows: print("(подписок нет)"); sys.exit(0)
    for r in rows:
        s = dict(r); e = s["email"]; mark = "*" if e == a else " "
        print(f"{mark} {e}\tkind={s.get('kind','?')}\tplan={s.get('plan') or '—'}\t"
              f"fleet={s.get('fleet') or 'prod'}\tstatus={s.get('status','?')}\t"
              f"added={s.get('added') or '—'}\tsource={s.get('source') or '—'}\t"
              f"seller={s.get('seller') or '—'}\tproxy={s.get('proxy') or '—'}")
PY
}

cmd_auth_start() {
  local email="${1:-}"; [ -n "$email" ] || die "нужен <email>"
  local raw="${2:-}" purl
  if [ -n "$raw" ]; then purl="$(proxy_url "$raw")"; else purl="$(_reg get "$email" proxy)"; fi
  local dir fifo log; dir="$(_dir_of "$email")"; fifo="$(_fifo_of "$email")"; log="$(_log_of "$email")"
  mkdir -p "$dir"; [ -f "$dir/settings.json" ] || echo '{"theme":"dark-daltonized"}' > "$dir/settings.json"
  rm -f "$fifo" "$log"; mkfifo "$fifo"; chmod 600 "$fifo"; : > "$log"
  # фон: login читает code#state из FIFO (режим <> — иначе open() блокируется)
  nohup env CLAUDE_CONFIG_DIR="$dir" ${purl:+HTTPS_PROXY="$purl" HTTP_PROXY="$purl"} \
    "$CLAUDE" auth login --claudeai --email "$email" <>"$fifo" >"$log" 2>&1 &
  disown 2>/dev/null || true
  _reg upsert "$email" "$dir" "$purl" "pending"
  # вытащить OAuth-URL из лога
  local url="" i=0
  while [ "$i" -lt 20 ]; do
    url="$(grep -oE 'https://claude\.com/[^ ]*oauth/authorize[^ ]*' "$log" 2>/dev/null | head -1)"
    [ -n "$url" ] && break
    sleep 1; i=$((i+1))
  done
  [ -n "$url" ] || die "не нашёл OAuth-URL в $log (хвост: $(tail -3 "$log" 2>/dev/null | tr '\n' ' '))"
  echo "SUB_URL: $url"
  ok "auth-start $email — открой URL, залогинься, затем: subscription.sh auth-finish $email '<code#state>'"
}

cmd_auth_finish() {
  local email="${1:-}" codestate="${2:-}"
  [ -n "$email" ] && [ -n "$codestate" ] || die "использование: auth-finish <email> '<code#state>'"
  local fifo log; fifo="$(_fifo_of "$email")"; log="$(_log_of "$email")"
  [ -p "$fifo" ] || die "нет активного auth для $email — сперва auth-start"
  printf '%s\n' "$codestate" > "$fifo"
  local i=0 done=""
  while [ "$i" -lt 25 ]; do
    grep -q 'Login successful' "$log" 2>/dev/null && { done=1; break; }
    grep -qiE 'invalid|error|not supported|failed' "$log" 2>/dev/null && break
    sleep 1; i=$((i+1))
  done
  rm -f "$fifo"
  [ -n "$done" ] || die "логин не подтверждён (хвост: $(tail -4 "$log" 2>/dev/null | tr '\n' ' '))"
  _reg upsert "$email" "$(_dir_of "$email")" "$(_reg get "$email" proxy)" "active"
  cmd_ping "$email" >/dev/null 2>&1 || die "$email залогинен, но ping не прошёл — проверь прокси/аккаунт (subscription.sh ping $email)"
  # первая подписка → сразу активная
  [ -s "$ACTIVE_PROFILE_F" ] || cmd_activate "$email" >/dev/null
  # АВТО-тариф: login-токен имеет scope user:profile → тариф подтянется сам (без ручного set-plan)
  cmd_detect_plan "$email" >/dev/null 2>&1 || true
  ok "auth-finish $email — подписка авторизована и живая (тариф: $(_reg get "$email" plan | sed 's/^$/—/'))"
}

cmd_add() {  # интерактивно (для CLI на mini)
  local email="${1:-}" raw="${2:-}"
  [ -n "$email" ] || die "использование: add <email> <proxy>"
  cmd_auth_start "$email" "$raw"
  printf 'Залогинься по URL выше, вставь сюда строку code#state и Enter: ' >&2
  local cs; read -r cs
  cmd_auth_finish "$email" "$cs"
}

cmd_activate() {
  local email="${1:-}"; [ -n "$email" ] || die "нужен <email>"
  [ -d "$(_dir_of "$email")" ] || die "нет профиля $(_dir_of "$email")"
  _reg activate "$email" || die "activate $email (нет такой подписки в реестре)"
  ok "active = $email (profile=$(cat "$ACTIVE_PROFILE_F"), proxy=$(cat "$ACTIVE_PROXY_F" 2>/dev/null || echo —))"
}

# МОСТ из auth_token_bot: регистрируем ТОКЕН-подписку (claude setup-token → long-lived
# токен) в наш реестр, чтобы коробки/движок её потребляли. <token_file> — файл с
# токеном sk-ant-…; proxy — ip:port:user:pass. Токен кладём в <profile>/oauth_token
# (его читают box_oauth_token движка и box_docker), прокси проецируем в active_proxy.
# Аргументы: <email> <token_file> [proxy] [fleet] [seller] [seller_id]
#   fleet — dev|prod (на какой стенд заходит подписка; дефолт prod = Test+Main)
#   seller / seller_id — продавец, у которого куплена (username и числовой telegram id)
cmd_register_token() {
  local email="${1:-}" token_file="${2:-}" raw="${3:-}"
  local fleet="${4:-prod}" seller="${5:-}" seller_id="${6:-}" seller_nick="${7:-}"
  [ -n "$email" ] && [ -n "$token_file" ] || die "использование: register-token <email> <token_file> [proxy] [fleet] [seller] [seller_id] [seller_nick]"
  [ -f "$token_file" ] || die "нет файла токена: $token_file"
  local tok; tok="$(tr -d '[:space:]' < "$token_file")"
  [ -n "$tok" ] || die "пустой токен в $token_file"
  case "$fleet" in dev|prod) ;; ""|*) fleet="prod" ;; esac
  local dir purl; dir="$(_dir_of "$email")"; purl="$(proxy_url "$raw")"
  mkdir -p "$dir"
  printf '%s\n' "$tok" > "$dir/oauth_token"; chmod 600 "$dir/oauth_token"
  _reg regtoken "$email" "$dir" "$purl" "${SUB_SOURCE:-auth_bot}" "$fleet" "$seller" "$seller_id" "$seller_nick" || die "regtoken $email"
  # первая подписка → сразу активная; иначе выбор активной за оператором/ротацией
  [ -s "$ACTIVE_PROFILE_F" ] || cmd_activate "$email" >/dev/null
  # АВТО-тариф: сработает, если setup-token выпущен со scope user:profile; inference-only → noscope
  cmd_detect_plan "$email" >/dev/null 2>&1 || true
  ok "register-token $email (kind=token, fleet=$fleet, продавец=${seller:-—}, source=${SUB_SOURCE:-auth_bot}, тариф: $(_reg get "$email" plan | sed 's/^$/—/'))"
}

# Флот подписки: dev (DevStand) | prod (Test+Main / ProDevStand).
cmd_set_fleet() {
  local email="${1:-}" fleet="${2:-}"
  [ -n "$email" ] && [ -n "$fleet" ] || die "использование: set-fleet <email> <dev|prod>"
  case "$fleet" in dev|prod) ;; *) die "неизвестный флот: '$fleet' (нужно dev | prod)" ;; esac
  _reg set-fleet "$email" "$fleet" || die "set-fleet $email"
  ok "fleet $email = $fleet"
}

# Продавец подписки: <email> <seller_username> [seller_id] [seller_nick].
cmd_set_seller() {
  local email="${1:-}" seller="${2:-}" seller_id="${3:-}" seller_nick="${4:-}"
  [ -n "$email" ] && [ -n "$seller" ] || die "использование: set-seller <email> <seller> [seller_id] [seller_nick]"
  _reg set-seller "$email" "$seller" "$seller_id" "$seller_nick" || die "set-seller $email"
  ok "seller $email = $seller${seller_id:+ (id $seller_id)}${seller_nick:+ / $seller_nick}"
}

# Нормализовать тариф в канон: pro | max5 | max20 (или пусто, чтобы снять).
# Принимаем: pro/про, max5/max-5/5x/5х, max20/max-20/20x/20х, «max 5x», «клод макс 20х».
_norm_plan() {
  # Понижаем регистр только ASCII (tr с кириллицей бьёт UTF-8), убираем пробелы/дефисы.
  local p; p="$(printf '%s' "${1:-}" | tr 'A-Z' 'a-z' | tr -d ' _-')"
  case "$p" in
    ""|none|снять|clear)                       echo "" ;;
    *20*)                                       echo "max20" ;;   # «max 20x», «20х», «клод макс 20х»
    *5*)                                        echo "max5" ;;    # «max 5x», «5х»
    *pro*|*про*|*Про*|*ПРО*)                    echo "pro" ;;
    *) return 1 ;;
  esac
}

cmd_set_plan() {
  local email="${1:-}" raw="${2:-}"
  [ -n "$email" ] || die "использование: set-plan <email> <pro|max5|max20|—>"
  local plan; plan="$(_norm_plan "$raw")" || die "неизвестный тариф: '$raw' (нужно pro | max5 | max20)"
  _reg set-plan "$email" "$plan" || die "set-plan $email"
  ok "plan $email = ${plan:-—}"
}

# АВТО-ОПРЕДЕЛЕНИЕ тарифа — ровно как это делает сам Claude Code: GET /api/oauth/profile
# с Bearer-токеном подписки (через её прокси), из ответа берём organization.rate_limit_tier
# и account.has_claude_max/has_claude_pro → канон pro|max5|max20. Работает для токенов со
# scope user:profile (все `claude auth login`). Токен только с user:inference (часть
# setup-token) профиль не отдаёт (403) — тогда молча оставляем как есть (можно set-plan руками).
# Печатает: DETECT_PLAN <email> <plan|noscope|err:...>. Пишет в реестр при успехе.
cmd_detect_plan() {
  local email="${1:-}"; [ -n "$email" ] || email="$(_reg active)"
  [ -n "$email" ] || die "нет email и нет активной подписки"
  local dir purl tok tokfile
  dir="$(_dir_of "$email")"
  tokfile="$(_reg get "$email" token_file)"; [ -n "$tokfile" ] || tokfile="$dir/oauth_token"
  [ -f "$tokfile" ] || { warn "detect-plan $email: нет токена ($tokfile)"; echo "DETECT_PLAN $email err:notoken"; return 1; }
  tok="$(tr -d '[:space:]' < "$tokfile")"
  purl="$(_reg get "$email" proxy)"
  local plan
  plan="$(SUB_TOK="$tok" SUB_PROXY="$purl" python3 - <<'PY'
import os,json,urllib.request
tok=os.environ.get("SUB_TOK",""); proxy=os.environ.get("SUB_PROXY","")
if not tok: print("err:notoken"); raise SystemExit
opener=urllib.request.build_opener(urllib.request.ProxyHandler({"https":proxy,"http":proxy}) if proxy else urllib.request.BaseHandler())
# ТОЧНО как claude: только Authorization+Content-Type, GET, без anthropic-beta.
req=urllib.request.Request("https://api.anthropic.com/api/oauth/profile",
    headers={"Authorization":f"Bearer {tok}","Content-Type":"application/json"}, method="GET")
try:
    resp=opener.open(req,timeout=20); body=resp.read().decode("utf-8","replace")
except urllib.error.HTTPError as e:
    code=e.code
    # 403 = у токена нет scope user:profile (inference-only) — тариф недоступен, не ошибка.
    print("noscope" if code==403 else f"err:http{code}"); raise SystemExit
except Exception as e:
    print(f"err:{type(e).__name__}"); raise SystemExit
try: prof=json.loads(body)
except Exception: print("err:badjson"); raise SystemExit
acc=prof.get("account") or {}; org=prof.get("organization") or {}
tier=(org.get("rate_limit_tier") or ""); otype=(org.get("organization_type") or "")
if acc.get("has_claude_max") or otype=="claude_max" or tier.startswith("default_claude_max"):
    if tier=="default_claude_max_20x": print("max20")
    elif tier=="default_claude_max_5x": print("max5")
    else: print("max5")   # редкий Max без явного tier — консервативно как max5
elif acc.get("has_claude_pro") or otype=="claude_pro" or tier=="default_claude_pro":
    print("pro")
else:
    print("err:unknown:"+(otype or tier or "empty"))
PY
)"
  case "$plan" in
    pro|max5|max20)
      _reg set-plan "$email" "$plan" || die "set-plan $email"
      echo "DETECT_PLAN $email $plan"; ok "detect-plan $email = $plan (авто из профиля)"; return 0 ;;
    noscope)
      echo "DETECT_PLAN $email noscope"
      warn "detect-plan $email: токен без scope user:profile — тариф авто-недоступен (перелогинь или set-plan вручную)"; return 0 ;;
    *)
      echo "DETECT_PLAN $email ${plan:-err:empty}"
      warn "detect-plan $email: не удалось определить (${plan:-пусто})"; return 0 ;;
  esac
}

cmd_set_proxy() {
  local email="${1:-}" raw="${2:-}"
  [ -n "$email" ] && [ -n "$raw" ] || die "использование: set-proxy <email> <proxy>"
  _reg set-proxy "$email" "$(proxy_url "$raw")" || die "set-proxy $email"
  [ "$(_reg active)" = "$email" ] && cmd_activate "$email" >/dev/null || true   # обновить проекцию, если активная
  ok "proxy $email обновлён"
}

cmd_ping() {
  local email="${1:-}"; [ -n "$email" ] || email="$(_reg active)"
  [ -n "$email" ] || die "нет email и нет активной подписки"
  local dir purl out; dir="$(_dir_of "$email")"; purl="$(_reg get "$email" proxy)"
  out="$(CLAUDE_CONFIG_DIR="$dir" ${purl:+HTTPS_PROXY="$purl" HTTP_PROXY="$purl"} \
        timeout 45 "$CLAUDE" -p "Reply with exactly: ok" --model "$PING_MODEL" 2>&1)"
  if printf '%s' "$out" | grep -qi '^[[:space:]]*ok'; then ok "ping $email: ok"; return 0
  else warn "ping $email: ${out:-（пусто — возможно прокси висит）}"; return 1; fi
}

cmd_refresh() {
  local email="${1:-}"; [ -n "$email" ] || email="$(_reg active)"
  [ -n "$email" ] || die "нет активной подписки"
  # Оппортунистический авто-тариф: если план ещё не определён — пробуем на каждом
  # прогреве (после перелогина токен обретёт scope user:profile → план подтянется сам).
  local cur; cur="$(_reg get "$email" plan)"
  [ -z "$cur" ] && cmd_detect_plan "$email" >/dev/null 2>&1 || true
  cmd_ping "$email" >/dev/null 2>&1 && ok "refresh $email: токен освежён" || warn "refresh $email не удался"
}

cmd_list() { _reg list; echo "active: $(_reg active || echo —)"; }

usage() {
  cat >&2 <<EOF
subscription.sh <cmd>   (<email> = идентификатор подписки)
  add <email> <proxy>                интерактивная авторизация
  auth-start <email> [proxy]         запустить логин, напечатать OAuth-URL
  auth-finish <email> '<code#state>' докормить код, проверить, записать активной
  relogin <email>                    перелогин (proxy из реестра)
  register-token <email> <file> [proxy]  зарегистрировать токен-подписку (мост auth_token_bot)
  activate <email>                   сделать активной
  set-proxy <email> <proxy>          сменить прокси
  set-plan <email> <pro|max5|max20>  задать тип тарифа ВРУЧНУЮ (фолбэк-оверрайд)
  set-fleet <email> <dev|prod>       флот подписки (DevStand vs Test+Main)
  set-seller <email> <seller> [id]   продавец, у которого куплена подписка
  detect-plan [email]                АВТО-определить тариф из /api/oauth/profile (дефолт — активная)
  refresh [email]                    прогрев токена (дефолт — активная)
  ping <email>                       проверка живости через её прокси
  list                               список подписок
Формат proxy: ip:port:user:pass | ip:port | http(s)://…
EOF
  exit 1
}

# SUB_NO_MAIN=1 при `source` → только функции (для тестов), без диспетчера.
[ "${SUB_NO_MAIN:-0}" = "1" ] && return 0 2>/dev/null

sub="${1:-}"; shift || true
case "$sub" in
  add)          cmd_add "$@" ;;
  auth-start)   cmd_auth_start "$@" ;;
  auth-finish)  cmd_auth_finish "$@" ;;
  relogin)      cmd_auth_start "$@" ;;     # proxy подтянется из реестра
  activate)     cmd_activate "$@" ;;
  register-token) cmd_register_token "$@" ;;   # мост из auth_token_bot (setup-token)
  set-proxy)    cmd_set_proxy "$@" ;;
  set-plan)     cmd_set_plan "$@" ;;
  set-fleet)    cmd_set_fleet "$@" ;;
  set-seller)   cmd_set_seller "$@" ;;
  detect-plan)  cmd_detect_plan "$@" ;;
  refresh)      cmd_refresh "$@" ;;
  ping)         cmd_ping "$@" ;;
  list)         cmd_list "$@" ;;
  *)            usage ;;
esac
