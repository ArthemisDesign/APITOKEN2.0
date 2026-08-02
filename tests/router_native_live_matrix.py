#!/usr/bin/env python3
"""Credential-safe production acceptance matrix for the unified native APIs.

The API key is accepted only through APITOKEN_API_KEY, removed from the process
environment before the first request, and never written to argv, files, logs, or
output.  The matrix intentionally uses tiny outputs because every successful
generation is billable.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Iterable


DEFAULT_BASE_URL = "https://router.apitoken.sale"
DEFAULT_ANTHROPIC_MODEL = "anthropic/claude-haiku-4-5-20251001"
DEFAULT_OPENAI_MODEL = "openai/gpt-5.4"
DEFAULT_GEMINI_MODEL = "gemini-2.5-flash-lite"
TIMEOUT_SECONDS = 90


class MatrixFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class Response:
    status: int
    headers: dict[str, str]
    body: bytes

    def json(self) -> Any:
        try:
            return json.loads(self.body)
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            raise MatrixFailure(f"HTTP {self.status} returned non-JSON data") from exc


def require(condition: bool, message: str) -> None:
    if not condition:
        raise MatrixFailure(message)


def nested_values(value: Any, key: str) -> Iterable[Any]:
    if isinstance(value, dict):
        for child_key, child in value.items():
            if child_key == key:
                yield child
            yield from nested_values(child, key)
    elif isinstance(value, list):
        for child in value:
            yield from nested_values(child, key)


class Client:
    def __init__(self, base_url: str, api_key: str) -> None:
        parsed = urllib.parse.urlsplit(base_url.rstrip("/"))
        require(parsed.scheme in {"http", "https"} and bool(parsed.netloc), "invalid APITOKEN_ROUTER_BASE_URL")
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key

    def request(
        self,
        method: str,
        path: str,
        payload: Any | None = None,
        *,
        headers: dict[str, str] | None = None,
    ) -> Response:
        body = None
        request_headers = {
            "accept": "application/json",
            "x-api-key": self.api_key,
        }
        if payload is not None:
            body = json.dumps(payload, separators=(",", ":")).encode()
            request_headers["content-type"] = "application/json"
        request_headers.update(headers or {})
        request = urllib.request.Request(
            f"{self.base_url}{path}",
            data=body,
            headers=request_headers,
            method=method,
        )
        try:
            with urllib.request.urlopen(request, timeout=TIMEOUT_SECONDS) as response:
                return Response(response.status, dict(response.headers.items()), response.read())
        except urllib.error.HTTPError as error:
            return Response(error.code, dict(error.headers.items()), error.read())

    def close(self) -> None:
        self.api_key = ""


def parse_sse(body: bytes) -> list[tuple[str | None, Any]]:
    frames: list[tuple[str | None, Any]] = []
    event_name: str | None = None
    data_lines: list[str] = []
    for raw_line in body.decode("utf-8", "strict").splitlines() + [""]:
        if not raw_line:
            if data_lines:
                joined = "\n".join(data_lines)
                if joined != "[DONE]":
                    try:
                        frames.append((event_name, json.loads(joined)))
                    except json.JSONDecodeError as exc:
                        raise MatrixFailure("SSE frame contains invalid JSON") from exc
            event_name = None
            data_lines = []
        elif raw_line.startswith("event:"):
            event_name = raw_line[6:].strip()
        elif raw_line.startswith("data:"):
            data_lines.append(raw_line[5:].lstrip())
    return frames


def anthropic_headers() -> dict[str, str]:
    return {"anthropic-version": "2023-06-01"}


def anthropic_matrix(client: Client, model: str) -> int:
    checks = 0
    common = {
        "model": model,
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "Reply exactly OK."}],
    }

    response = client.request("POST", "/v1/messages", common, headers=anthropic_headers())
    require(response.status == 200, f"Anthropic non-stream returned HTTP {response.status}")
    message = response.json()
    require(message.get("type") == "message" and isinstance(message.get("usage"), dict), "invalid Anthropic message envelope")
    checks += 1

    stream_body = dict(common, stream=True)
    response = client.request(
        "POST",
        "/v1/messages",
        stream_body,
        headers={**anthropic_headers(), "accept": "text/event-stream"},
    )
    require(response.status == 200, f"Anthropic stream returned HTTP {response.status}")
    frames = parse_sse(response.body)
    events = {event for event, _payload in frames}
    require("message_start" in events and "message_stop" in events, "incomplete Anthropic SSE lifecycle")
    checks += 1

    tool_request = {
        "model": model,
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "Call echo with value OK."}],
        "tools": [
            {
                "name": "echo",
                "description": "Return the supplied value.",
                "input_schema": {
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                    "required": ["value"],
                },
            }
        ],
        "tool_choice": {"type": "tool", "name": "echo"},
    }
    response = client.request("POST", "/v1/messages", tool_request, headers=anthropic_headers())
    require(response.status == 200, f"Anthropic tool call returned HTTP {response.status}")
    tool_message = response.json()
    require(any(block.get("type") == "tool_use" for block in tool_message.get("content", [])), "Anthropic tool_use missing")
    checks += 1

    count_request = {"model": model, "messages": [{"role": "user", "content": "OK"}]}
    response = client.request("POST", "/v1/messages/count_tokens", count_request, headers=anthropic_headers())
    require(response.status == 200, f"Anthropic count_tokens returned HTTP {response.status}")
    count = response.json()
    require(isinstance(count.get("input_tokens"), int) and count["input_tokens"] > 0, "invalid Anthropic token count")
    checks += 1

    response = client.request(
        "POST",
        "/v1/messages",
        {"model": model, "max_tokens": 8},
        headers=anthropic_headers(),
    )
    require(response.status == 400, f"Anthropic validation returned HTTP {response.status}")
    error = response.json().get("error", {})
    require(isinstance(error, dict) and isinstance(error.get("type"), str), "invalid Anthropic error envelope")
    checks += 1
    return checks


def openai_matrix(client: Client, model: str) -> int:
    checks = 0
    common = {
        "model": model,
        "input": "Reply exactly OK.",
        "max_output_tokens": 16,
    }

    response = client.request("POST", "/v1/responses", common)
    require(response.status == 200, f"OpenAI Responses non-stream returned HTTP {response.status}")
    standard = response.json()
    require(standard.get("object") == "response" and isinstance(standard.get("usage"), dict), "invalid OpenAI response envelope")
    require("priority" not in set(nested_values(standard.get("usage"), "service_tier")), "Standard request unexpectedly used priority")
    checks += 1

    response = client.request("POST", "/v1/responses", {**common, "service_tier": "priority"})
    require(response.status == 200, f"OpenAI Fast response returned HTTP {response.status}")
    fast = response.json()
    require("priority" in set(nested_values(fast, "service_tier")), "OpenAI Fast response lacks authoritative priority tier")
    checks += 1

    response = client.request(
        "POST",
        "/v1/responses",
        {**common, "stream": True},
        headers={"accept": "text/event-stream"},
    )
    require(response.status == 200, f"OpenAI Responses stream returned HTTP {response.status}")
    frames = parse_sse(response.body)
    event_types = {payload.get("type") for _event, payload in frames if isinstance(payload, dict)}
    require("response.created" in event_types and "response.completed" in event_types, "incomplete Responses SSE lifecycle")
    checks += 1

    tool_request = {
        "model": model,
        "input": "Call echo with value OK.",
        "max_output_tokens": 64,
        "tools": [
            {
                "type": "function",
                "name": "echo",
                "description": "Return the supplied value.",
                "parameters": {
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                    "required": ["value"],
                    "additionalProperties": False,
                },
                "strict": True,
            }
        ],
        "tool_choice": {"type": "function", "name": "echo"},
    }
    response = client.request("POST", "/v1/responses", tool_request)
    require(response.status == 200, f"OpenAI tool call returned HTTP {response.status}")
    tool_response = response.json()
    require(any(item.get("type") == "function_call" for item in tool_response.get("output", [])), "OpenAI function_call missing")
    checks += 1

    response = client.request("POST", "/v1/responses/input_tokens", {"model": model, "input": "OK"})
    require(response.status == 200, f"OpenAI input_tokens returned HTTP {response.status}")
    count = response.json()
    require(isinstance(count.get("input_tokens"), int) and count["input_tokens"] > 0, "invalid OpenAI token count")
    checks += 1

    response = client.request("POST", "/v1/responses", {"model": model, "max_output_tokens": 8})
    require(response.status == 400, f"OpenAI validation returned HTTP {response.status}")
    error = response.json().get("error", {})
    require(isinstance(error, dict) and isinstance(error.get("type"), str), "invalid OpenAI error envelope")
    checks += 1
    return checks


def gemini_matrix(client: Client, model: str) -> int:
    checks = 0
    quoted_model = urllib.parse.quote(model, safe="-._~")
    common = {
        "contents": [{"role": "user", "parts": [{"text": "Reply exactly OK."}]}],
        "generationConfig": {"maxOutputTokens": 8},
    }

    response = client.request("POST", f"/v1beta/models/{quoted_model}:generateContent", common)
    require(response.status == 200, f"Gemini generateContent returned HTTP {response.status}")
    generated = response.json()
    require(isinstance(generated.get("candidates"), list) and isinstance(generated.get("usageMetadata"), dict), "invalid Gemini response envelope")
    checks += 1

    response = client.request(
        "POST",
        f"/v1beta/models/{quoted_model}:streamGenerateContent?alt=sse",
        common,
        headers={"accept": "text/event-stream"},
    )
    require(response.status == 200, f"Gemini streamGenerateContent returned HTTP {response.status}")
    frames = parse_sse(response.body)
    require(any(isinstance(payload, dict) and payload.get("candidates") for _event, payload in frames), "Gemini SSE candidates missing")
    require(any(isinstance(payload, dict) and payload.get("usageMetadata") for _event, payload in frames), "Gemini SSE usage missing")
    checks += 1

    tool_request = {
        "contents": [{"role": "user", "parts": [{"text": "Call echo with value OK."}]}],
        "generationConfig": {"maxOutputTokens": 64},
        "tools": [
            {
                "functionDeclarations": [
                    {
                        "name": "echo",
                        "description": "Return the supplied value.",
                        "parameters": {
                            "type": "object",
                            "properties": {"value": {"type": "string"}},
                            "required": ["value"],
                        },
                    }
                ]
            }
        ],
        "toolConfig": {
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": ["echo"],
            }
        },
    }
    response = client.request("POST", f"/v1beta/models/{quoted_model}:generateContent", tool_request)
    require(response.status == 200, f"Gemini tool call returned HTTP {response.status}")
    tool_response = response.json()
    require(any(nested_values(tool_response.get("candidates", []), "functionCall")), "Gemini functionCall missing")
    checks += 1

    count_request = {"contents": [{"role": "user", "parts": [{"text": "OK"}]}]}
    response = client.request("POST", f"/v1beta/models/{quoted_model}:countTokens", count_request)
    require(response.status == 200, f"Gemini countTokens returned HTTP {response.status}")
    count = response.json()
    require(isinstance(count.get("totalTokens"), int) and count["totalTokens"] > 0, "invalid Gemini token count")
    checks += 1

    response = client.request("POST", f"/v1beta/models/{quoted_model}:generateContent", {"contents": []})
    require(response.status == 400, f"Gemini validation returned HTTP {response.status}")
    error = response.json().get("error", {})
    require(isinstance(error, dict) and isinstance(error.get("status"), str), "invalid Gemini error envelope")
    checks += 1
    return checks


def catalog_matrix(client: Client, anthropic_model: str, openai_model: str, gemini_model: str) -> int:
    checks = 0
    response = client.request("GET", "/v1/models")
    require(response.status == 200, f"aggregate catalog returned HTTP {response.status}")
    catalog = response.json()
    ids = {entry.get("id") for entry in catalog.get("data", [])}
    require({anthropic_model, openai_model}.issubset(ids), "aggregate catalog lacks selected Anthropic/OpenAI models")
    require(any(str(item).startswith("google/") and str(item).endswith(gemini_model) for item in ids), "aggregate catalog lacks selected Gemini model")
    checks += 1

    for model in (anthropic_model, openai_model):
        path = "/v1/models/" + urllib.parse.quote(model, safe="/-._~")
        response = client.request("GET", path)
        require(response.status == 200 and response.json().get("id") == model, f"catalog get failed for {model}")
        checks += 1

    response = client.request("GET", "/v1beta/models")
    require(response.status == 200, f"Gemini model list returned HTTP {response.status}")
    names = {entry.get("name") for entry in response.json().get("models", [])}
    require(f"models/{gemini_model}" in names, "Gemini model list lacks selected model")
    checks += 1

    response = client.request("GET", "/v1beta/models/" + urllib.parse.quote(gemini_model, safe="-._~"))
    require(response.status == 200 and response.json().get("name") == f"models/{gemini_model}", "Gemini model get failed")
    checks += 1
    return checks


def main() -> int:
    api_key = os.environ.pop("APITOKEN_API_KEY", "")
    if not api_key:
        print("APITOKEN_API_KEY must already be set", file=sys.stderr)
        return 2

    base_url = os.environ.get("APITOKEN_ROUTER_BASE_URL", DEFAULT_BASE_URL)
    anthropic_model = os.environ.get("APITOKEN_ANTHROPIC_MODEL", DEFAULT_ANTHROPIC_MODEL)
    openai_model = os.environ.get("APITOKEN_OPENAI_MODEL", DEFAULT_OPENAI_MODEL)
    gemini_model = os.environ.get("APITOKEN_GEMINI_MODEL", DEFAULT_GEMINI_MODEL)
    client = Client(base_url, api_key)
    api_key = ""

    total = 0
    try:
        count = catalog_matrix(client, anthropic_model, openai_model, gemini_model)
        total += count
        print(f"[PASS] catalog list/get: {count}")
        count = anthropic_matrix(client, anthropic_model)
        total += count
        print(f"[PASS] native Anthropic: {count}")
        count = openai_matrix(client, openai_model)
        total += count
        print(f"[PASS] native OpenAI Responses + Standard/Fast: {count}")
        count = gemini_matrix(client, gemini_model)
        total += count
        print(f"[PASS] native Gemini: {count}")
    except (MatrixFailure, OSError, UnicodeError) as exc:
        print(f"[FAIL] {exc}", file=sys.stderr)
        return 1
    finally:
        client.close()

    print(f"router native live matrix passed: {total} checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
