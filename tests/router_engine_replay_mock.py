#!/usr/bin/env python3
"""Deterministic Anthropic-like upstream for the router→engine replay acceptance."""

from __future__ import annotations

import argparse
import json
import signal
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

OUTPUT_TEXT = "replay output"
MODEL = "claude-haiku-4-5-20251001"


class ReplayState:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.requests: list[dict[str, object]] = []
        self.failure: str | None = None

    def observe(self, request: dict[str, object]) -> None:
        with self.lock:
            self.requests.append(request)
            if request.get("stream") is True:
                messages = request.get("messages")
                if not isinstance(messages, list) or not messages:
                    self.failure = "stream replay request omitted Messages history"
                    return
                content = messages[-1].get("content") if isinstance(messages[-1], dict) else None
                if isinstance(content, list):
                    content = "".join(
                        part.get("text", "")
                        for part in content
                        if isinstance(part, dict) and part.get("type") == "text"
                    )
                if content != OUTPUT_TEXT:
                    self.failure = (
                        "stream replay prompt must equal the first visible output exactly; "
                        f"got {content!r}"
                    )


class ReplayServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int]) -> None:
        self.state = ReplayState()
        super().__init__(address, ReplayHandler)


class ReplayHandler(BaseHTTPRequestHandler):
    server: ReplayServer

    def log_message(self, *_: object) -> None:
        return

    def _send_headers(self, content_type: str, length: int) -> None:
        self.send_response(200)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(length))
        self.send_header("anthropic-ratelimit-unified-5h-utilization", "0.10000000")
        self.send_header("anthropic-ratelimit-unified-7d-utilization", "0.20000000")
        self.send_header("anthropic-ratelimit-unified-5h-reset", "1999999999")
        self.send_header("anthropic-ratelimit-unified-7d-reset", "1999999999")
        self.send_header("anthropic-ratelimit-unified-status", "allowed")
        self.end_headers()

    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
        self.send_response(404)
        self.end_headers()

    def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
        length = int(self.headers.get("content-length", "0"))
        try:
            request = json.loads(self.rfile.read(length) if length else b"{}")
        except (json.JSONDecodeError, UnicodeDecodeError):
            self.send_response(400)
            self.end_headers()
            return
        if not isinstance(request, dict):
            self.send_response(400)
            self.end_headers()
            return
        self.server.state.observe(request)
        if request.get("stream") is True:
            body = self._stream_body()
            self._send_headers("text/event-stream", len(body))
        else:
            body = json.dumps(
                {
                    "id": "msg_replay",
                    "type": "message",
                    "role": "assistant",
                    "model": MODEL,
                    "content": [{"type": "text", "text": OUTPUT_TEXT}],
                    "stop_reason": "end_turn",
                    "stop_sequence": None,
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 3,
                        "cache_read_input_tokens": 0,
                        "cache_creation_input_tokens": 0,
                    },
                },
                separators=(",", ":"),
                sort_keys=True,
            ).encode("utf-8")
            self._send_headers("application/json", len(body))
        self.wfile.write(body)

    @staticmethod
    def _stream_body() -> bytes:
        frames = [
            (
                "message_start",
                {
                    "type": "message_start",
                    "message": {
                        "id": "msg_replay",
                        "type": "message",
                        "role": "assistant",
                        "model": MODEL,
                        "content": [],
                        "stop_reason": None,
                        "stop_sequence": None,
                        "usage": {
                            "input_tokens": 11,
                            "output_tokens": 1,
                            "cache_read_input_tokens": 0,
                            "cache_creation_input_tokens": 0,
                        },
                    },
                },
            ),
            (
                "content_block_start",
                {
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""},
                },
            ),
            ("ping", {"type": "ping"}),
            (
                "content_block_delta",
                {
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": "replay "},
                },
            ),
            (
                "content_block_delta",
                {
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": "output"},
                },
            ),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            (
                "message_delta",
                {
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                    "usage": {"output_tokens": 3},
                },
            ),
            ("message_stop", {"type": "message_stop"}),
        ]
        return b"".join(
            f"event: {event}\ndata: {json.dumps(data, separators=(',', ':'), sort_keys=True)}\n\n".encode(
                "utf-8"
            )
            for event, data in frames
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ready-file", required=True)
    parser.add_argument("--result-file", required=True)
    args = parser.parse_args()
    server = ReplayServer(("127.0.0.1", 0))
    ready = Path(args.ready_file)
    result = Path(args.result_file)
    ready.write_text(str(server.server_address[1]), encoding="utf-8")

    def stop(*_: object) -> None:
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    server.serve_forever()
    with server.state.lock:
        result.write_text(
            json.dumps(
                {
                    "requests": len(server.state.requests),
                    "failure": server.state.failure,
                },
                separators=(",", ":"),
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
    server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
