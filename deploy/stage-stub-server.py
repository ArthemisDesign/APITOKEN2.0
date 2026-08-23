#!/usr/bin/env python3
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json, os

HOST = "10.254.32.2"
PORT = int(os.environ.get("STAGE_STUB_PORT", "3901"))
ROLE = os.environ.get("STAGE_STUB_ROLE", "mock-upstream")

class Handler(BaseHTTPRequestHandler):
    def reply(self, code, body):
        payload = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers(); self.wfile.write(payload)
    def do_GET(self):
        if self.path in ("/health", "/ready"):
            self.reply(200, {"ok": True, "role": ROLE})
        elif self.path == "/metrics":
            payload = b"stage_safe_sink_up 1\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; version=0.0.4")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers(); self.wfile.write(payload)
        else: self.reply(404, {"error": "not_found"})
    def do_POST(self):
        length = min(int(self.headers.get("content-length", "0")), 1048576)
        if length: self.rfile.read(length)
        self.reply(202, {"accepted": True, "sink": ROLE, "external_side_effect": False})
    def log_message(self, fmt, *args):
        print(f"stage-stub role={ROLE} {fmt % args}", flush=True)

ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
