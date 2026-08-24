#!/usr/bin/env python3
"""Fixed-path staging client for one capped production Anthropic account."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import http.client, json, os

HOST = "10.254.32.2"
PORT = 9081
UPSTREAM_HOST = "10.254.32.1"
UPSTREAM_PORT = 9081
KEY_FILE = "/etc/apitoken-staging/stage-live.key"
MAX_BODY = 1_048_576
MAX_TOKENS = 64

class Handler(BaseHTTPRequestHandler):
    def reply(self, code, body):
        payload = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(code); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload))); self.end_headers(); self.wfile.write(payload)
    def do_GET(self):
        if self.path == "/ready": self.reply(200, {"ok": True, "mode": "budgeted-live-endpoint"})
        else: self.reply(404, {"error": "not_found"})
    def do_POST(self):
        if self.path != "/v1/messages": return self.reply(404, {"error": "not_found"})
        length = int(self.headers.get("content-length", "0"))
        if length < 2 or length > MAX_BODY: return self.reply(413, {"error": "body_limit"})
        try: body = json.loads(self.rfile.read(length))
        except Exception: return self.reply(400, {"error": "invalid_json"})
        if set(body) - {"model","max_tokens","messages","system","temperature","stream"}:
            return self.reply(400, {"error": "unsupported_field"})
        if body.get("model") not in {"claude-sonnet-4-6", "claude-sonnet-5"} or not isinstance(body.get("messages"), list):
            return self.reply(400, {"error": "invalid_request"})
        if body.get("stream", False) is not False: return self.reply(400, {"error": "stream_disabled"})
        if not isinstance(body.get("max_tokens"), int) or not 1 <= body["max_tokens"] <= MAX_TOKENS:
            return self.reply(400, {"error": "max_tokens_cap"})
        key = open(KEY_FILE).read().strip()
        encoded = json.dumps(body, separators=(",", ":")).encode()
        try:
            conn = http.client.HTTPConnection(UPSTREAM_HOST, UPSTREAM_PORT, timeout=30)
            conn.request("POST", "/v1/messages", encoded, {"content-type":"application/json","x-api-key":key,"anthropic-version":"2023-06-01"})
            response = conn.getresponse(); payload = response.read(min(int(response.getheader("content-length") or MAX_BODY), MAX_BODY))
            self.send_response(response.status); self.send_header("content-type", response.getheader("content-type") or "application/json")
            self.send_header("content-length", str(len(payload))); self.end_headers(); self.wfile.write(payload)
        except Exception: self.reply(502, {"error": "live_endpoint_unavailable"})
    def log_message(self, fmt, *args): print(f"stage-live-proxy {fmt % args}", flush=True)

ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
