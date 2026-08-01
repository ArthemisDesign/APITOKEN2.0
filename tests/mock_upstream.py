#!/usr/bin/env python3
"""Мок api.anthropic.com для smoke-валидации ротации/веера БЕЗ живых подписок.

Возвращает Anthropic-подобный 200 + unified-ratelimit заголовки (формат подтверждён живым probe:
util — доля 0..1, reset — epoch). РЕАЛИСТИЧНО растит util5h с использованием каждой подписки
(как настоящий Anthropic), чтобы `place_best` мог распределять нагрузку по свободной ёмкости.
Логирует хвост Bearer-токена каждого запроса в $SRV_LOG → видно распределение по флоту.

Запуск: SRV_LOG=/tmp/hits.log python3 mock_upstream.py [PORT]

Если в теле Messages-запроса есть ключ "thinking" (universal адаптеры инжектят
его при reasoning effort, этапы 3.4c/4.2) — отвечает thinking-блоками:
non-stream content [thinking, text] с usage.output_tokens_details.thinking_tokens,
stream — content_block_start(thinking) → 2 thinking_delta → signature_delta →
content_block_stop, затем обычный text-диалог. Триггер строго по ключу,
чтобы остальные cases не менялись.
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
        payload = self.rfile.read(length) if length else b""
        self._ratelimit_headers(util5)
        try:
            request = json.loads(payload or b"{}")
        except Exception:
            request = {}
        want_stream = bool(request.get("stream"))
        want_tools = bool(request.get("tools"))
        want_thinking = "thinking" in request
        if want_stream:
            self._sse(tools=want_tools, thinking=want_thinking)
        elif want_thinking:
            self._thinking_json()
        elif want_tools:
            self._tool_json()
        else:
            self._json()
    def _ratelimit_headers(self, util5):
        self.send_response(200)
        self.send_header("anthropic-ratelimit-unified-5h-utilization", f"{util5:.3f}")
        self.send_header("anthropic-ratelimit-unified-7d-utilization", "0.05")
        self.send_header("anthropic-ratelimit-unified-5h-reset", "9999999999")
        self.send_header("anthropic-ratelimit-unified-7d-reset", "9999999999")
        self.send_header("anthropic-ratelimit-unified-status", "allowed")
    def _json(self):
        body = json.dumps({
            "id": "msg_mock", "type": "message", "role": "assistant",
            "model": "claude-haiku-4-5-20251001",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 2,
                      "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        }).encode()
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def _tool_json(self):
        # Tool-use ответ для e2e проверки universal chat tools (3.2):
        # tool_use блок + stop_reason tool_use.
        body = json.dumps({
            "id": "msg_mock", "type": "message", "role": "assistant",
            "model": "claude-haiku-4-5-20251001",
            "content": [{"type": "tool_use", "id": "toolu_mock1", "name": "get_weather",
                         "input": {"city": "Paris"}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 12, "output_tokens": 7,
                      "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        }).encode()
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def _thinking_json(self):
        # Reasoning-ответ (4.2): thinking-блок с подписью (она наружу не
        # выставляется) + text; thinking-токены в output_tokens_details.
        body = json.dumps({
            "id": "msg_mock", "type": "message", "role": "assistant",
            "model": "claude-haiku-4-5-20251001",
            "content": [
                {"type": "thinking", "thinking": "mock think full", "signature": "sig_mock"},
                {"type": "text", "text": "ok"},
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 7,
                      "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
                      "output_tokens_details": {"thinking_tokens": 5}},
        }).encode()
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def _sse(self, tools=False, thinking=False):
        # Мини-диалог Messages SSE: message_start → ping → 2 text-дельты →
        # message_delta(stop) → message_stop. Для e2e проверки universal chat
        # streaming (tests/universal_chat_smoke.sh). При tools=True — tool_use
        # диалог: content_block_start(tool_use) → 2 input_json_delta →
        # message_delta(tool_use). При thinking=True — reasoning-диалог (4.2):
        # thinking-блок (2 thinking_delta + signature_delta, которая должна
        # дропаться) → обычный text-блок следующим index.
        model = "claude-haiku-4-5-20251001"
        if thinking:
            frames = [
                ("message_start", {"type": "message_start", "message": {
                    "id": "msg_mock", "type": "message", "role": "assistant",
                    "model": model, "content": [], "stop_reason": None,
                    "usage": {"input_tokens": 10, "output_tokens": 1,
                              "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}),
                ("content_block_start", {"type": "content_block_start", "index": 0,
                    "content_block": {"type": "thinking", "thinking": ""}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "thinking_delta", "thinking": "mock think 1"}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "thinking_delta", "thinking": " mock think 2"}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "signature_delta", "signature": "sig_mock"}}),
                ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                ("content_block_start", {"type": "content_block_start", "index": 1,
                    "content_block": {"type": "text", "text": ""}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 1,
                    "delta": {"type": "text_delta", "text": "mock"}}),
                ("content_block_stop", {"type": "content_block_stop", "index": 1}),
                ("message_delta", {"type": "message_delta",
                    "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                    "usage": {"output_tokens": 7,
                              "output_tokens_details": {"thinking_tokens": 5}}}),
                ("message_stop", {"type": "message_stop"}),
            ]
        elif tools:
            frames = [
                ("message_start", {"type": "message_start", "message": {
                    "id": "msg_mock", "type": "message", "role": "assistant",
                    "model": model, "content": [], "stop_reason": None,
                    "usage": {"input_tokens": 12, "output_tokens": 1,
                              "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}),
                ("content_block_start", {"type": "content_block_start", "index": 0,
                    "content_block": {"type": "tool_use", "id": "toolu_mock1",
                                      "name": "get_weather", "input": {}}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "{\"city\":"}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "\"Paris\"}"}}),
                ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                ("message_delta", {"type": "message_delta",
                    "delta": {"stop_reason": "tool_use", "stop_sequence": None},
                    "usage": {"output_tokens": 7}}),
                ("message_stop", {"type": "message_stop"}),
            ]
        else:
            frames = [
            ("message_start", {"type": "message_start", "message": {
                "id": "msg_mock", "type": "message", "role": "assistant",
                "model": model, "content": [], "stop_reason": None,
                "usage": {"input_tokens": 10, "output_tokens": 1,
                          "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}),
            ("content_block_start", {"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
            ("ping", {"type": "ping"}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "mock"}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": " ok"}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("message_delta", {"type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                "usage": {"output_tokens": 2}}),
            ("message_stop", {"type": "message_stop"}),
        ]
        body = b"".join(
            b"event: %s\ndata: %s\n\n" % (ev.encode(), json.dumps(data).encode())
            for ev, data in frames
        )
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def do_POST(self): self._hit()
    def do_GET(self):
        if self.path.startswith("/v1/models"):
            # Каталог плоскости для единого /v1/models router'а (alias-dispatch
            # universal chat). Прочие GET (detect_plan) — прежний 403.
            body = json.dumps({"data": [
                {"type": "model", "id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5",
                 "created_at": "2026-01-02T00:00:00Z"},
                {"type": "model", "id": "claude-opus-4-8", "display_name": "Claude Opus 4.8",
                 "created_at": "2026-01-01T00:00:00Z"},
            ], "has_more": False, "first_id": "claude-haiku-4-5", "last_id": "claude-opus-4-8"}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(403); self.end_headers()  # detect_plan → noscope-подобно

if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9099
    open(LOG, "w").close()
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
