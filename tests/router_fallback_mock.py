#!/usr/bin/env python3
"""Deterministic provider-plane mock for the router fallback load smoke.

The process owns one loopback listener and writes bounded JSON events without request bodies,
credentials, model-group IDs, or headers. Three independent instances let the smoke kill exactly
one plane and prove that only TCP ConnectionRefused is transport-safe for continuation.
"""

import argparse
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


MODELS = {
    "anthropic": ("claude-sonnet-5", "anthropic/claude-sonnet-5"),
    "openai": ("gpt-5.6-terra", "openai/gpt-5.6-terra"),
    "google": ("gemini-3.6-flash", "google/gemini-3.6-flash"),
}
POLICY_KEYS = {
    "mock-policy-filter-key": "policy_filter",
    "mock-load-key": "signed_retry",
    "mock-unsigned-key": "unsigned_stop",
    "mock-connect-key": "connect_refused",
}


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--plane", choices=tuple(MODELS), required=True)
    parser.add_argument("--ready-file", type=Path, required=True)
    parser.add_argument("--events-file", type=Path, required=True)
    return parser.parse_args()


ARGS = parse_args()
EVENT_LOCK = threading.Lock()


def record(kind, scenario, **fields):
    event = {"kind": kind, "scenario": scenario, **fields}
    encoded = json.dumps(event, separators=(",", ":"), sort_keys=True)
    with EVENT_LOCK:
        with ARGS.events_file.open("a", encoding="utf-8") as events:
            events.write(encoded + "\n")


class Handler(BaseHTTPRequestHandler):
    server_version = "RouterFallbackMock/1"

    def log_message(self, *_args):
        return

    def reply(self, status, body, headers=None):
        encoded = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(encoded)))
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self):
        if self.path == "/health":
            self.reply(200, {"status": "ok"})
            return
        native, _catalog_id = MODELS[ARGS.plane]
        if ARGS.plane == "anthropic" and self.path.startswith("/v1/models"):
            record("catalog", "discovery")
            self.reply(200, {"data": [{"id": native, "created_at": "2026-01-01T00:00:00Z", "display_name": "Mock Anthropic"}]})
            return
        if ARGS.plane == "openai" and self.path == "/v1/models":
            record("catalog", "discovery")
            self.reply(200, {"data": [{"id": native, "object": "model", "created": 1783555200}]})
            return
        if ARGS.plane == "google" and self.path.startswith("/v1beta/models"):
            record("catalog", "discovery")
            self.reply(200, {"models": [{"name": f"models/{native}", "created": 1783555200, "displayName": "Mock Gemini"}]})
            return
        self.reply(404, {"error": "not found"})

    def do_POST(self):
        length = int(self.headers.get("content-length", "0") or "0")
        if length > 1024 * 1024:
            self.reply(413, {"error": "too large"})
            return
        try:
            body = json.loads(self.rfile.read(length) if length else b"{}")
        except (json.JSONDecodeError, UnicodeDecodeError):
            self.reply(400, {"error": "invalid json"})
            return
        key = self.headers.get("x-api-key", "")
        scenario = POLICY_KEYS.get(key, "unknown")
        if self.path == "/internal/router/auth/preflight":
            self.reply(200, {"schema_version": 1, "authenticated": True})
            return
        if self.path == "/internal/router/policy/preflight":
            candidates = [candidate.get("id") for candidate in body.get("candidates", [])]
            record("preflight", scenario, candidates=candidates)
            if scenario == "policy_filter":
                allowed = [candidate for candidate in candidates if candidate.startswith("openai/")]
                mode = "strict"
            else:
                allowed = candidates
                mode = "unrestricted"
            self.reply(200, {"schema_version": 1, "mode": mode, "allowed": allowed})
            return

        record("execution", scenario, attempt=self.headers.get("x-apitoken-attempt", "none"))
        if ARGS.plane == "anthropic" and scenario == "signed_retry":
            self.reply(
                503,
                {"error": "synthetic exact not_started"},
                {"x-apitoken-execution-state": "not_started"},
            )
            return
        if ARGS.plane == "anthropic" and scenario == "unsigned_stop":
            self.reply(503, {"error": "synthetic ambiguous 503"})
            return
        if ARGS.plane == "anthropic" and scenario == "policy_filter":
            self.reply(500, {"error": "strict policy failed to filter attempt 1"})
            return
        self.reply(
            200,
            {
                "id": f"mock-{ARGS.plane}",
                "object": "response",
                "status": "completed",
                "model": body.get("model"),
            },
        )


ARGS.events_file.touch(mode=0o600, exist_ok=True)
server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
ARGS.ready_file.write_text(str(server.server_address[1]) + "\n", encoding="utf-8")
ARGS.ready_file.chmod(0o600)
server.serve_forever()
