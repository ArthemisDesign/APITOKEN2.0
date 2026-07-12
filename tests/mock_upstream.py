#!/usr/bin/env python3
"""Мок api.anthropic.com для smoke-валидации ротации/веера БЕЗ живых подписок.

Возвращает Anthropic-подобный 200 + unified-ratelimit заголовки (формат подтверждён живым probe:
util — доля 0..1, reset — epoch). РЕАЛИСТИЧНО растит util5h с использованием каждой подписки
(как настоящий Anthropic), чтобы `place_best` мог распределять нагрузку по свободной ёмкости.
Логирует хвост Bearer-токена каждого запроса в $SRV_LOG → видно распределение по флоту.

Запуск: SRV_LOG=/tmp/hits.log python3 mock_upstream.py [PORT]
"""
import json, os, sys, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = os.environ.get("SRV_LOG", "/tmp/mock_hits.log")
_hits, _lock = {}, threading.Lock()

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _hit(self):
        auth = self.headers.get("authorization", "")
        tok = auth.replace("Bearer ", "")[-6:] if auth else "NONE"
        with _lock:
            _hits[tok] = _hits.get(tok, 0) + 1
            n = _hits[tok]
        with open(LOG, "a") as f:
            f.write(tok + "\n")
        util5 = min(0.90, 0.05 + n * 0.03)   # util растёт с нагрузкой
        length = int(self.headers.get("content-length", 0) or 0)
        if length:
            self.rfile.read(length)
        body = json.dumps({
            "id": "msg_mock", "type": "message", "role": "assistant",
            "model": "claude-haiku-4-5-20251001",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 2,
                      "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        }).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("anthropic-ratelimit-unified-5h-utilization", f"{util5:.3f}")
        self.send_header("anthropic-ratelimit-unified-7d-utilization", "0.05")
        self.send_header("anthropic-ratelimit-unified-5h-reset", "9999999999")
        self.send_header("anthropic-ratelimit-unified-7d-reset", "9999999999")
        self.send_header("anthropic-ratelimit-unified-status", "allowed")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def do_POST(self): self._hit()
    def do_GET(self):
        self.send_response(403); self.end_headers()  # detect_plan → noscope-подобно

if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9099
    open(LOG, "w").close()
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
