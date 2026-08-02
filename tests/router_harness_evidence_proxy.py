#!/usr/bin/env python3
"""Local credential-blind proxy that records bounded router harness evidence.

The proxy forwards request/response bytes but records only protocol metadata,
model IDs, status, and service-tier evidence.  Headers, credentials, prompts,
tool arguments, and generated content are never persisted or printed.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import ssl
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Iterable


BODY_LIMIT = 64 * 1024 * 1024
HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-base-url", default="https://router.apitoken.sale")
    parser.add_argument("--label", required=True)
    parser.add_argument("--ready-file", type=Path, required=True)
    parser.add_argument("--evidence-file", type=Path, required=True)
    parser.add_argument("--api-key-env")
    return parser.parse_args()


def nested_values(value: Any, key: str) -> Iterable[Any]:
    if isinstance(value, dict):
        for child_key, child in value.items():
            if child_key == key:
                yield child
            yield from nested_values(child, key)
    elif isinstance(value, list):
        for child in value:
            yield from nested_values(child, key)


def json_or_none(body: bytes) -> Any | None:
    try:
        return json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return None


def sse_json_values(body: bytes) -> list[Any]:
    try:
        lines = body.decode("utf-8", "strict").splitlines()
    except UnicodeDecodeError:
        return []
    values: list[Any] = []
    data_lines: list[str] = []
    for line in lines + [""]:
        if not line:
            if data_lines:
                joined = "\n".join(data_lines)
                if joined != "[DONE]":
                    value = json_or_none(joined.encode())
                    if value is not None:
                        values.append(value)
            data_lines = []
        elif line.startswith("data:"):
            data_lines.append(line[5:].lstrip())
    return values


def protocol_for(path: str) -> str:
    clean_path = urllib.parse.urlsplit(path).path
    if clean_path.startswith("/v1/messages"):
        return "anthropic_messages"
    if clean_path.startswith("/v1/responses"):
        return "openai_responses"
    if clean_path.startswith("/v1/chat/completions"):
        return "openai_chat"
    if clean_path.startswith("/v1beta/models"):
        return "gemini_native"
    if clean_path.startswith("/v1/models"):
        return "catalog"
    return "other"


def model_for(path: str, request_json: Any | None) -> str | None:
    if isinstance(request_json, dict) and isinstance(request_json.get("model"), str):
        return request_json["model"][:256]
    clean_path = urllib.parse.urlsplit(path).path
    prefix = "/v1beta/models/"
    if clean_path.startswith(prefix):
        return urllib.parse.unquote(clean_path[len(prefix) :].split(":", 1)[0])[:256]
    return None


class Recorder:
    def __init__(self, path: Path, label: str) -> None:
        self.path = path
        self.label = label
        self.lock = threading.Lock()
        self.sequence = 0
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch(mode=0o600, exist_ok=True)
        path.chmod(0o600)

    def append(self, entry: dict[str, Any]) -> None:
        with self.lock:
            self.sequence += 1
            entry = {"sequence": self.sequence, "harness": self.label, **entry}
            with self.path.open("a", encoding="utf-8") as target:
                target.write(json.dumps(entry, separators=(",", ":"), sort_keys=True) + "\n")


def main() -> None:
    args = parse_args()
    injected_api_key = os.environ.pop(args.api_key_env, "") if args.api_key_env else ""
    if args.api_key_env and not injected_api_key:
        raise SystemExit(f"{args.api_key_env} must already be set")
    target = urllib.parse.urlsplit(args.target_base_url.rstrip("/"))
    if target.scheme not in {"http", "https"} or not target.hostname:
        raise SystemExit("target base URL must be absolute HTTP(S)")
    target_prefix = target.path.rstrip("/")
    recorder = Recorder(args.evidence_file, args.label)

    class Handler(BaseHTTPRequestHandler):
        server_version = "RouterHarnessEvidenceProxy/1"

        def log_message(self, *_args: Any) -> None:
            return

        def send_bytes(self, status: int, body: bytes, headers: list[tuple[str, str]]) -> None:
            self.send_response(status)
            for name, value in headers:
                lower = name.lower()
                if lower not in HOP_BY_HOP and lower not in {"content-length", "server", "date"}:
                    self.send_header(name, value)
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def handle_proxy(self) -> None:
            raw_length = self.headers.get("content-length", "0")
            try:
                length = int(raw_length or "0")
            except ValueError:
                length = -1
            if length < 0 or length > BODY_LIMIT:
                body = b'{"error":{"type":"invalid_request_error","message":"request body too large"}}'
                self.send_bytes(413, body, [("content-type", "application/json")])
                return
            request_body = self.rfile.read(length) if length else b""
            request_json = json_or_none(request_body)
            upstream_headers: dict[str, str] = {}
            for name, value in self.headers.items():
                lower = name.lower()
                if lower not in HOP_BY_HOP and lower not in {
                    "host",
                    "content-length",
                    "accept-encoding",
                    "authorization",
                    "x-api-key",
                    "x-goog-api-key",
                }:
                    upstream_headers[name] = value
            upstream_headers["accept-encoding"] = "identity"
            if injected_api_key:
                upstream_headers["x-api-key"] = injected_api_key
            if request_body:
                upstream_headers["content-length"] = str(len(request_body))

            upstream_path = target_prefix + self.path
            connection_class = http.client.HTTPSConnection if target.scheme == "https" else http.client.HTTPConnection
            connection_kwargs: dict[str, Any] = {"host": target.hostname, "port": target.port, "timeout": 180}
            if target.scheme == "https":
                connection_kwargs["context"] = ssl.create_default_context()
            connection = connection_class(**connection_kwargs)
            try:
                connection.request(self.command, upstream_path, body=request_body or None, headers=upstream_headers)
                response = connection.getresponse()
                response_body = response.read(BODY_LIMIT + 1)
                if len(response_body) > BODY_LIMIT:
                    raise RuntimeError("upstream response exceeded evidence proxy limit")
                response_headers = response.getheaders()
                status = response.status
            except Exception:
                status = 502
                response_body = b'{"error":{"type":"proxy_error","message":"evidence proxy upstream failure"}}'
                response_headers = [("content-type", "application/json")]
            finally:
                connection.close()

            response_values: list[Any] = []
            response_json = json_or_none(response_body)
            if response_json is not None:
                response_values.append(response_json)
            else:
                response_values.extend(sse_json_values(response_body))
            service_tiers = sorted(
                {
                    str(value)
                    for response_value in response_values
                    for value in nested_values(response_value, "service_tier")
                    if isinstance(value, (str, int, float, bool))
                }
            )
            event_types = sorted(
                {
                    value["type"]
                    for value in response_values
                    if isinstance(value, dict) and isinstance(value.get("type"), str)
                }
            )
            request_tiers = sorted(
                {
                    str(value)
                    for key in ("service_tier", "speed")
                    for value in nested_values(request_json, key)
                    if isinstance(value, (str, int, float, bool))
                }
            ) if request_json is not None else []
            request_tool_types = []
            if isinstance(request_json, dict) and isinstance(request_json.get("tools"), list):
                request_tool_types = [
                    tool.get("type")
                    for tool in request_json["tools"]
                    if isinstance(tool, dict) and isinstance(tool.get("type"), str)
                ][:32]
            recorder.append(
                {
                    "method": self.command,
                    "path": urllib.parse.urlsplit(self.path).path[:512],
                    "protocol": protocol_for(self.path),
                    "model": model_for(self.path, request_json),
                    "status": status,
                    "request_tiers": request_tiers,
                    "request_tool_types": request_tool_types,
                    "request_fast_header": self.headers.get("x-apitoken-service-tier"),
                    "response_service_tiers": service_tiers,
                    "response_event_types": event_types[:32],
                }
            )
            self.send_bytes(status, response_body, response_headers)

        do_DELETE = handle_proxy
        do_GET = handle_proxy
        do_PATCH = handle_proxy
        do_POST = handle_proxy
        do_PUT = handle_proxy

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    args.ready_file.parent.mkdir(parents=True, exist_ok=True)
    args.ready_file.write_text(str(server.server_address[1]) + "\n", encoding="utf-8")
    args.ready_file.chmod(0o600)
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
