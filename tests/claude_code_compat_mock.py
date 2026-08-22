#!/usr/bin/env python3
"""Credential-blind loopback Anthropic Messages mock for exact Claude Code acceptance."""

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, default=0)
parser.add_argument("--ready-file", required=True)
parser.add_argument("--evidence-file", required=True)
args = parser.parse_args()
ready_file = Path(args.ready_file)
evidence_file = Path(args.evidence_file)
EXPECTED_KEY = "claude-code-compat-placeholder"


def event_stream(frames) -> bytes:
    return b"".join(
        f"event: {event}\ndata: {json.dumps(data, separators=(',', ':'))}\n\n".encode()
        for event, data in frames
    )


def message_sse(structured: bool) -> bytes:
    start = ("message_start", {"type": "message_start", "message": {
        "id": "msg_compat", "type": "message", "role": "assistant", "content": [],
        "model": "claude-sonnet-4-6", "stop_reason": None, "stop_sequence": None,
        "usage": {"input_tokens": 1, "cache_creation_input_tokens": 0,
                  "cache_read_input_tokens": 0, "output_tokens": 0}}})
    if structured:
        return event_stream([
            start,
            ("content_block_start", {"type": "content_block_start", "index": 0,
                                     "content_block": {"type": "tool_use", "id": "toolu_structured",
                                                       "name": "StructuredOutput", "input": {}}}),
            ("content_block_delta", {"type": "content_block_delta", "index": 0,
                                     "delta": {"type": "input_json_delta",
                                               "partial_json": '{"ok":true}'}}),
            ("content_block_stop", {"type": "content_block_stop", "index": 0}),
            ("message_delta", {"type": "message_delta",
                               "delta": {"stop_reason": "tool_use", "stop_sequence": None},
                               "usage": {"output_tokens": 1}}),
            ("message_stop", {"type": "message_stop"}),
        ])
    return event_stream([
        start,
        ("ping", {"type": "ping"}),
        ("content_block_start", {"type": "content_block_start", "index": 0,
                                 "content_block": {"type": "text", "text": ""}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0,
                                 "delta": {"type": "text_delta", "text": "OK"}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        ("message_delta", {"type": "message_delta",
                           "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                           "usage": {"output_tokens": 1}}),
        ("message_stop", {"type": "message_stop"}),
    ])


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        return

    def record(self, body: bytes = b""):
        headers = {}
        for name, value in self.headers.items():
            lowered = name.lower()
            if lowered in {"authorization", "x-api-key"}:
                value = "<redacted>"
            headers.setdefault(lowered, []).append(value)
        parsed = None
        if body:
            try:
                parsed = json.loads(body)
            except Exception:
                parsed = "<invalid-json>"
        entry = {"method": self.command, "path": self.path, "headers": headers, "body": parsed}
        with evidence_file.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, ensure_ascii=False, separators=(",", ":")) + "\n")
        return parsed

    def send_bytes(self, status: int, content_type: str, body: bytes, **headers):
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        for name, value in headers.items():
            self.send_header(name.replace("_", "-"), value)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)
            self.wfile.flush()

    def do_HEAD(self):
        self.record()
        self.send_bytes(200, "application/json", b"")

    def credential_ok(self):
        return self.headers.get("x-api-key") == EXPECTED_KEY and not self.headers.get("authorization")

    def do_GET(self):
        self.record()
        if not self.credential_ok():
            self.send_bytes(401, "application/json", b'{"error":"wrong test credential"}')
            return
        if self.path == "/v1/models?limit=1000":
            body = json.dumps({"data": [{"id": "claude-sonnet-4-6",
                                         "display_name": "Claude Sonnet 4.6"}]}).encode()
            self.send_bytes(200, "application/json", body)
        else:
            self.send_bytes(404, "application/json", b'{"error":"not found"}')

    def do_POST(self):
        length = int(self.headers.get("content-length", "0") or 0)
        body = self.rfile.read(length) if length else b""
        parsed = self.record(body)
        if not self.credential_ok():
            self.send_bytes(401, "application/json", b'{"error":"wrong test credential"}')
            return
        if self.path.startswith("/v1/messages/count_tokens"):
            self.send_bytes(200, "application/json", b'{"input_tokens":1}',
                            request_id="req_compat_count")
            return
        if self.path.startswith("/v1/messages"):
            tools = parsed.get("tools", []) if isinstance(parsed, dict) else []
            structured = any(isinstance(tool, dict) and tool.get("name") == "StructuredOutput"
                             for tool in tools)
            self.send_bytes(200, "text/event-stream", message_sse(structured),
                            request_id="req_compat_messages", cache_control="no-cache")
            return
        self.send_bytes(404, "application/json", b'{"error":"not found"}')


server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
ready_file.write_text(str(server.server_address[1]), encoding="ascii")
server.serve_forever()
