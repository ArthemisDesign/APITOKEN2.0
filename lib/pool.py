"""Пул-селектор: выбор живой подписки на ход + распределение по лимитам.

Standalone-порт логики оркестратора движка (docs/POOL_SELECTION.md) для этого проекта.
Читает реестр (subscriptions.db), держит волатильное состояние подписок (утилизация окон,
cooling) в subs_live.json и на каждый ход отдаёт наименее загруженную живую подписку.

Утилизация берётся из заголовков ответа Claude (anthropic-ratelimit-unified-5h/7d-*) —
их читает poll_sub() минимальным запросом через прокси подписки. Без секретов в subs_live.json
(только email/plan/util/status/resets/cooling).

Зависимостей нет (stdlib) — работает под systemd на сервере как есть.
"""
import os, json, time, sqlite3, threading, urllib.request, urllib.error

_LOCK = threading.RLock()

# ── пути/конфиг из окружения ────────────────────────────────────────────────
def cfg_dir():   return os.environ.get("SUB_CFG_DIR") or os.path.expanduser("~/.config/claude-api")
def db_path():   return os.environ.get("SUBS_DB") or os.path.join(cfg_dir(), "subscriptions.db")
def live_path(): return os.path.join(cfg_dir(), "subs_live.json")
def my_fleet():  return (os.environ.get("SUBS_FLEET", "") or "").strip()   # пусто/all = любой флот
def poll_model():return os.environ.get("CLAUDE_API_POLL_MODEL", "claude-haiku-4-5-20251001")

# Порог утилизации: клиентские ходы — до 95%, «мозговые»/приоритетные — до 100%.
CLIENT_CAP = float(os.environ.get("CLAUDE_API_UTIL_CAP", "0.95"))
# На сколько секунд студить подписку при 429, если reset неизвестен.
COOL_DEFAULT = int(os.environ.get("CLAUDE_API_COOL_SECS", "300"))


def _now(): return int(time.time())

# ── реестр: активные подписки нужного флота ─────────────────────────────────
def load_subs(db=None):
    """Список dict-подписок (status=active, совпадающий флот, kind=token/login с профилем)."""
    db = db or db_path()
    if not os.path.exists(db):
        return []
    c = sqlite3.connect(db, timeout=10); c.row_factory = sqlite3.Row
    try:
        rows = c.execute("SELECT email,profile_dir,token_file,proxy,plan,status,fleet,kind FROM subs").fetchall()
    finally:
        c.close()
    f = my_fleet(); out = []
    for r in rows:
        d = dict(r)
        if (d.get("status") or "") != "active":
            continue
        if f and f != "all" and (d.get("fleet") or "prod") != f:
            continue
        if not (d.get("profile_dir") or ""):
            continue
        if not (d.get("token_file") or ""):
            d["token_file"] = os.path.join(d["profile_dir"], "oauth_token")
        out.append(d)
    return out


# ── волатильное состояние (утилизация/cooling) ──────────────────────────────
def load_live():
    with _LOCK:
        try:
            return json.load(open(live_path()))
        except Exception:
            return {}

def save_live(state):
    with _LOCK:
        try:
            os.makedirs(os.path.dirname(live_path()), exist_ok=True)
            tmp = live_path() + ".tmp"
            json.dump(state, open(tmp, "w"), ensure_ascii=False, indent=2)
            os.replace(tmp, live_path())
        except Exception:
            pass

def _update(email, **fields):
    with _LOCK:
        st = load_live(); d = st.get(email, {}); d.update(fields); st[email] = d
        save_live(st)

def mark_used(email):
    _update(email, last_used=_now())

def mark_cooling(email, secs=None, reason="429"):
    secs = COOL_DEFAULT if secs is None else int(secs)
    _update(email, cooling_until=_now() + max(1, secs), cooling_reason=reason)

def mark_ok(email):
    """Ход прошёл — снять cooling (если reset уже наступил, он и так истёк)."""
    with _LOCK:
        st = load_live(); d = st.get(email, {})
        if d.get("cooling_until", 0) <= _now():
            d.pop("cooling_until", None); d.pop("cooling_reason", None)
        d["last_used"] = _now(); st[email] = d; save_live(st)

def set_util(email, util5h=None, util7d=None, status=None, reset5h=None, reset7d=None):
    f = {"polled_ts": _now()}
    if util5h is not None: f["util5h"] = util5h
    if util7d is not None: f["util7d"] = util7d
    if status is not None: f["status"] = status
    if reset5h is not None: f["reset5h"] = reset5h
    if reset7d is not None: f["reset7d"] = reset7d
    _update(email, **f)


# ── выбор на ход ────────────────────────────────────────────────────────────
def _cooling(st, email): return st.get(email, {}).get("cooling_until", 0) > _now()
def _util(st, email, w): return float(st.get(email, {}).get(w, 0.0) or 0.0)

def pick_sub(prefer=None, exclude=None, allow_full=False, db=None):
    """Наименее загруженная живая подписка (dict) или None.

    prefer     — попытаться взять именно эту (email), если она в пуле и не в exclude.
    exclude    — множество email, которые уже пробовали в этом ходе (ротация при 429).
    allow_full — пускать до 100% (приоритетные ходы), иначе клиентский потолок CLIENT_CAP.
    Порядок: сначала не-cooling и под потолком; сортировка по util7d, затем util5h, затем LRU.
    """
    exclude = set(exclude or ())
    subs = [s for s in load_subs(db) if s["email"] not in exclude]
    if not subs:
        return None
    st = load_live()
    cap = 1.0 if allow_full else CLIENT_CAP

    if prefer:
        for s in subs:
            if s["email"] == prefer:
                return s   # явное предпочтение уважаем безусловно (кроме exclude)

    avail = [s for s in subs if not _cooling(st, s["email"])]
    pool = avail or subs                                   # все стынут → берём наименее «горячую»
    ready = [s for s in pool if _util(st, s["email"], "util7d") < cap
                              and _util(st, s["email"], "util5h") < cap]
    pool = ready or pool                                   # все под потолком → берём наименее полную
    def key(s):
        e = s["email"]
        return (round(_util(st, e, "util7d"), 3), round(_util(st, e, "util5h"), 3),
                st.get(e, {}).get("last_used", 0))
    pool.sort(key=key)
    return pool[0]


# ── чтение токена ────────────────────────────────────────────────────────────
def read_token(sub):
    tf = sub.get("token_file") or ""
    if tf and os.path.exists(tf):
        try:
            return open(tf).read().strip()
        except Exception:
            return ""
    return ""


# ── опрос лимитов: минимальный запрос через прокси, читаем ratelimit-заголовки ─
def _hf(headers, name):
    try:
        v = headers.get(name)
        if v is None: return None
        v = float(v)
        return v / 100.0 if v > 1.0 else v          # util приходит процентом (42 → 0.42)
    except Exception:
        return None

def _hi(headers, name):
    try:
        v = headers.get(name); return int(float(v)) if v is not None else None
    except Exception:
        return None

def poll_sub(sub, timeout=20):
    """Минимальный POST /v1/messages через прокси подписки; читаем unified-ratelimit из
    ЗАГОЛОВКОВ (даже на ошибочном ответе — заголовки приходят и при 400/429). Пишет util в
    subs_live.json. Возвращает dict со снапшотом или None (нет токена/сети)."""
    tok = read_token(sub)
    if not tok:
        return None
    proxy = sub.get("proxy") or ""
    body = json.dumps({"model": poll_model(), "max_tokens": 1,
                       "messages": [{"role": "user", "content": "."}]}).encode()
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages", data=body, method="POST",
        headers={"authorization": f"Bearer {tok}", "anthropic-beta": "oauth-2025-04-20",
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({"https": proxy, "http": proxy}) if proxy
        else urllib.request.BaseHandler())
    headers = None; http_status = None
    try:
        resp = opener.open(req, timeout=timeout); headers = resp.headers; http_status = resp.status
        resp.read()
    except urllib.error.HTTPError as e:
        headers = e.headers; http_status = e.code            # 400/429 всё равно несут заголовки
    except Exception:
        return None
    if headers is None:
        return None
    u5 = _hf(headers, "anthropic-ratelimit-unified-5h-utilization")
    u7 = _hf(headers, "anthropic-ratelimit-unified-7d-utilization")
    status = headers.get("anthropic-ratelimit-unified-status") or headers.get("anthropic-ratelimit-unified-5h-status")
    r5 = _hi(headers, "anthropic-ratelimit-unified-5h-reset")
    r7 = _hi(headers, "anthropic-ratelimit-unified-7d-reset")
    email = sub["email"]
    set_util(email, util5h=u5, util7d=u7, status=status, reset5h=r5, reset7d=r7)
    # HTTP 429 без util-заголовков → всё равно студим до reset (или дефолт)
    if http_status == 429:
        secs = (r5 - _now()) if r5 else None
        mark_cooling(email, secs, reason="poll429")
    return {"email": email, "util5h": u5, "util7d": u7, "status": status,
            "reset5h": r5, "reset7d": r7, "http": http_status}


def pool_status(db=None):
    """Снапшот пула для /pool и /health — без секретов."""
    subs = load_subs(db); st = load_live(); now = _now(); out = []
    for s in subs:
        e = s["email"]; d = st.get(e, {})
        cu = d.get("cooling_until", 0)
        out.append({
            "email": e, "plan": s.get("plan") or "", "fleet": s.get("fleet") or "prod",
            "proxy": bool(s.get("proxy")), "util5h": d.get("util5h"), "util7d": d.get("util7d"),
            "status": d.get("status"), "cooling": cu > now,
            "cooling_left": max(0, cu - now) if cu > now else 0,
            "last_used": d.get("last_used", 0), "polled_ts": d.get("polled_ts", 0),
        })
    return out
