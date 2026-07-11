#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""claude-api HTTP-сервер: пул подписок как СЕТЕВОЙ API.

Дёргает пул подписок (subscriptions.db) как настоящий API по HTTP: на каждый запрос
пул-селектор (lib/pool.py) отдаёт наименее загруженную живую подписку, запрос исполняется
через её OAuth-токен + прокси (claude CLI, как Claude Code), при 429/лимите — АВТО-РОТАЦИЯ
на следующую подписку. Фоновый поллер опрашивает лимиты (ratelimit-заголовки) для
балансировки. Учёт токенов + USD-эквивалент (lib/cost.py) в usage.log.

Зависимостей нет (stdlib http.server) — работает под systemd на сервере как есть.

Эндпоинты:
  GET  /health           — жив ли сервер, сколько подписок в пуле (без авторизации)
  GET  /pool             — статус пула (util/cooling по подпискам, без секретов)
  POST /run              — {prompt, model?, sub?, allow_full?} → {result, usage, usd, sub, model}
  POST /v1/messages      — минимальный Anthropic-совместимый: {model, messages, max_tokens?}

Авторизация: заголовок `Authorization: Bearer <key>` или `x-api-key: <key>`.
  Ключи — env CLAUDE_API_KEYS (через запятую). Если не заданы — принимаем ТОЛЬКО с 127.0.0.1.

Запуск:  SUB_CFG_DIR=/srv/claude-api/data python3 server/app.py
Env: CLAUDE_API_HOST(0.0.0.0) CLAUDE_API_PORT(8787) CLAUDE_API_KEYS CLAUDE_BIN CLAUDE_API_MODEL
     CLAUDE_API_MAX_TRIES(3) CLAUDE_API_TIMEOUT(600) CLAUDE_API_POLL(1) CLAUDE_API_USAGE_LOG
"""
import os, sys, json, time, subprocess, threading, re
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
import pool  # noqa: E402
try:
    import cost as COST  # noqa: E402
except Exception:
    COST = None

CLAUDE   = os.environ.get("CLAUDE_BIN", os.path.expanduser("~/.local/bin/claude"))
MODEL    = os.environ.get("CLAUDE_API_MODEL", "claude-opus-4-8")
MAX_TRIES = int(os.environ.get("CLAUDE_API_MAX_TRIES", "3"))
TIMEOUT  = int(os.environ.get("CLAUDE_API_TIMEOUT", "600"))
HOST     = os.environ.get("CLAUDE_API_HOST", "0.0.0.0")
PORT     = int(os.environ.get("CLAUDE_API_PORT", "8787"))
USAGE_LOG = os.environ.get("CLAUDE_API_USAGE_LOG") or os.path.join(pool.cfg_dir(), "usage.log")
API_KEYS = set(k.strip() for k in os.environ.get("CLAUDE_API_KEYS", "").split(",") if k.strip())
POLL_ON  = os.environ.get("CLAUDE_API_POLL", "1") not in ("0", "", "false", "no")

# Маркеры упора в лимит подписки в выводе claude → повод для cooling+ротации.
_LIMIT_RE = re.compile(r"(rate.?limit|429|usage limit|quota|too many requests|overloaded|"
                       r"limit reached|resets? at)", re.I)


def _read_token(sub):
    return pool.read_token(sub)


def run_one(sub, prompt, model):
    """Один ход на конкретной подписке. → (rc, stdout, stderr). Ключ=OAuth-токен, IP=прокси."""
    env = os.environ.copy()
    env["CLAUDE_CONFIG_DIR"] = sub["profile_dir"]
    tok = _read_token(sub)
    if tok:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = tok
    if sub.get("proxy"):
        env["HTTPS_PROXY"] = sub["proxy"]; env["HTTP_PROXY"] = sub["proxy"]
    try:
        p = subprocess.run(
            [CLAUDE, "-p", prompt, "--model", model, "--output-format", "json"],
            env=env, capture_output=True, text=True, timeout=TIMEOUT, stdin=subprocess.DEVNULL)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"
    except FileNotFoundError:
        return 127, "", f"claude not found: {CLAUDE}"


def _looks_limited(rc, out, err):
    if rc != 0 and _LIMIT_RE.search((err or "") + " " + (out or "")):
        return True
    # claude может вернуть rc=0 с is_error+сообщением о лимите
    try:
        v = json.loads(out)
        if isinstance(v, dict) and v.get("is_error") and _LIMIT_RE.search(json.dumps(v)):
            return True
    except Exception:
        pass
    return False


def log_usage(email, model, usage):
    if COST:
        usd, br = COST.cost_from_usage(model, usage)
    else:
        u = usage or {}
        br = dict(intok=u.get("input_tokens", 0), out=u.get("output_tokens", 0),
                  cache_read=u.get("cache_read_input_tokens", 0),
                  cache_write=u.get("cache_creation_input_tokens", 0))
        usd = 0.0
    try:
        with open(USAGE_LOG, "a") as f:
            f.write(f"{int(time.time())}\t{email}\t{model}\tin={br['intok']}\tout={br['out']}\t"
                    f"cache_r={br['cache_read']}\tcache_w={br['cache_write']}\tusd={usd:.6f}\n")
    except Exception:
        pass
    return usd, br


def run_on_pool(prompt, model, sub_pref=None, allow_full=False):
    """Ход с авто-ротацией: пул выбирает подписку, при лимите — cooling+следующая. → dict."""
    tried, last = set(), {"error": "нет подписок в пуле"}
    for attempt in range(MAX_TRIES):
        prefer = sub_pref if attempt == 0 else None
        sub = pool.pick_sub(prefer=prefer, exclude=tried, allow_full=allow_full)
        if not sub:
            break
        email = sub["email"]; tried.add(email)
        rc, out, err = run_one(sub, prompt, model)
        if _looks_limited(rc, out, err):
            pool.mark_cooling(email, reason="run429")
            last = {"error": f"{email}: лимит/429 — ротация", "sub": email}
            continue
        if rc != 0:
            last = {"error": (err or out or f"rc={rc}").strip()[:2000], "sub": email, "rc": rc}
            continue
        try:
            v = json.loads(out)
        except Exception:
            pool.mark_ok(email)
            return {"result": out, "sub": email, "model": model, "usage": {}, "usd": 0.0}
        pool.mark_ok(email)
        usage = (v.get("usage") or {}) if isinstance(v, dict) else {}
        usd, br = log_usage(email, model, usage)
        return {"result": v.get("result", "") if isinstance(v, dict) else out,
                "raw": v, "sub": email, "model": model, "usage": usage,
                "usd": round(usd, 6), "tokens": br, "attempts": attempt + 1}
    last["attempts"] = len(tried)
    return last


# ── фоновый поллер лимитов (адаптивный интервал) ────────────────────────────
def poll_interval_for(sub_state):
    u = max(float(sub_state.get("util5h", 0) or 0), float(sub_state.get("util7d", 0) or 0))
    return 12 if u >= 0.7 else (30 if u >= 0.3 else 60)   # горячие опрашиваем чаще

def poller_loop(stop):
    while not stop.is_set():
        try:
            live = pool.load_live(); now = int(time.time())
            for sub in pool.load_subs():
                e = sub["email"]; stt = live.get(e, {})
                due = now - stt.get("polled_ts", 0) >= poll_interval_for(stt)
                if due:
                    pool.poll_sub(sub)
        except Exception:
            pass
        stop.wait(6)


# ── HTTP ─────────────────────────────────────────────────────────────────────
class Handler(BaseHTTPRequestHandler):
    server_version = "claude-api/1.0"

    def _send(self, code, obj):
        body = json.dumps(obj, ensure_ascii=False, indent=2).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _authed(self):
        if not API_KEYS:                                   # ключи не заданы → только localhost
            return self.client_address[0] in ("127.0.0.1", "::1", "localhost")
        auth = self.headers.get("Authorization", "")
        key = auth[7:].strip() if auth.lower().startswith("bearer ") else self.headers.get("x-api-key", "").strip()
        return key in API_KEYS

    def _body(self):
        try:
            n = int(self.headers.get("Content-Length", 0))
            return json.loads(self.rfile.read(n) or b"{}")
        except Exception:
            return {}

    def log_message(self, *a):                             # тише в stderr (systemd journal)
        pass

    def do_GET(self):
        if self.path.split("?")[0] == "/health":
            return self._send(200, {"ok": True, "subs": len(pool.load_subs()),
                                    "model": MODEL, "auth": bool(API_KEYS)})
        if self.path.split("?")[0] == "/pool":
            if not self._authed(): return self._send(401, {"error": "unauthorized"})
            return self._send(200, {"pool": pool.pool_status(),
                                    "cap": pool.CLIENT_CAP, "poller": POLL_ON})
        return self._send(404, {"error": "not found"})

    def do_POST(self):
        path = self.path.split("?")[0]
        if not self._authed():
            return self._send(401, {"error": "unauthorized"})
        b = self._body()
        if path == "/run":
            prompt = (b.get("prompt") or "").strip()
            if not prompt:
                return self._send(400, {"error": "prompt required"})
            r = run_on_pool(prompt, b.get("model") or MODEL,
                            sub_pref=b.get("sub"), allow_full=bool(b.get("allow_full")))
            return self._send(200 if "result" in r else 502, r)
        if path == "/v1/messages":
            return self._v1_messages(b)
        return self._send(404, {"error": "not found"})

    def _v1_messages(self, b):
        """Минимальный Anthropic-совместимый: messages[] → плоский prompt → пул → content[]."""
        msgs = b.get("messages") or []
        parts = []
        sysp = b.get("system")
        if isinstance(sysp, str) and sysp.strip():
            parts.append(sysp.strip())
        for m in msgs:
            c = m.get("content", "")
            if isinstance(c, list):
                c = " ".join(x.get("text", "") for x in c if isinstance(x, dict) and x.get("type") == "text")
            role = m.get("role", "user")
            parts.append(f"{role}: {c}" if role != "user" or len(msgs) > 1 else str(c))
        prompt = "\n\n".join(p for p in parts if p).strip()
        if not prompt:
            return self._send(400, {"type": "error", "error": {"message": "messages required"}})
        r = run_on_pool(prompt, b.get("model") or MODEL, allow_full=bool(b.get("allow_full")))
        if "result" not in r:
            return self._send(502, {"type": "error", "error": {"message": r.get("error", "pool error")}})
        u = r.get("usage") or {}
        return self._send(200, {
            "id": f"msg_pool_{int(time.time())}", "type": "message", "role": "assistant",
            "model": r.get("model"), "stop_reason": "end_turn",
            "content": [{"type": "text", "text": r.get("result", "")}],
            "usage": {"input_tokens": u.get("input_tokens", 0), "output_tokens": u.get("output_tokens", 0),
                      "cache_read_input_tokens": u.get("cache_read_input_tokens", 0),
                      "cache_creation_input_tokens": u.get("cache_creation_input_tokens", 0)},
            "_pool": {"sub": r.get("sub"), "usd": r.get("usd"), "attempts": r.get("attempts")},
        })


def main():
    if not API_KEYS:
        print("⚠️  CLAUDE_API_KEYS не заданы — сервер принимает ТОЛЬКО с 127.0.0.1", file=sys.stderr)
    stop = threading.Event()
    if POLL_ON:
        threading.Thread(target=poller_loop, args=(stop,), daemon=True).start()
        print("поллер лимитов: включён", file=sys.stderr)
    srv = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"claude-api слушает http://{HOST}:{PORT}  (подписок в пуле: {len(pool.load_subs())}, "
          f"модель {MODEL}, реестр {pool.db_path()})", file=sys.stderr)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop.set(); srv.server_close()


if __name__ == "__main__":
    main()
