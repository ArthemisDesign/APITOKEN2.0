#!/usr/bin/env python3
"""Credential-safe live probe for the pinned Codex app-server.

The probe delegates all credential access to the official Codex binary. It emits only account type,
public model IDs, numeric rate-limit data, a fixed canary response, usage and status. Raw stderr,
account identity and authentication fields are never printed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any


CANARY = "APITOKEN_CODEX_LIVE_OK"
MAX_FRAME_BYTES = 32 * 1024 * 1024
DEFAULT_TIMEOUT_SECONDS = 180
PREFERRED_MODELS = (
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
    "gpt-5.4",
)


class ProbeError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--model")
    parser.add_argument("--timeout-seconds", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(64 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


class AppServer:
    def __init__(self, binary: Path, work_dir: Path, timeout_seconds: int) -> None:
        if not binary.is_absolute() or not binary.is_file():
            raise ProbeError("--binary must be an existing absolute file")
        if not work_dir.is_absolute() or not work_dir.is_dir():
            raise ProbeError("--work-dir must be an existing absolute directory")
        codex_home = os.environ.get("CODEX_HOME")
        if not codex_home:
            codex_home = str(Path.home() / ".codex")

        child_env = {
            "CODEX_HOME": codex_home,
            "PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
            "NO_COLOR": "1",
            "TERM": "dumb",
        }
        for name in (
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ):
            value = os.environ.get(name)
            if value:
                child_env[name] = value

        command = [str(binary)]
        for override in (
            "include_permissions_instructions=false",
            "include_apps_instructions=false",
            "include_collaboration_mode_instructions=false",
            "include_environment_context=false",
            "skills.include_instructions=false",
            "features.plugins=false",
            "features.apps=false",
            "features.multi_agent_v2=false",
            "project_doc_max_bytes=0",
            "mcp_servers={}",
        ):
            command.extend(("--config", override))
        command.extend(("app-server", "--listen", "stdio://"))

        self.process = subprocess.Popen(
            command,
            cwd=work_dir,
            env=child_env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
            bufsize=0,
        )
        self.timeout_seconds = timeout_seconds
        self.next_id = 1
        self.frames: queue.Queue[dict[str, Any] | BaseException] = queue.Queue()
        self.stderr_bytes = 0
        self.pending_notifications: list[dict[str, Any]] = []
        threading.Thread(target=self._read_stdout, daemon=True).start()
        threading.Thread(target=self._discard_stderr, daemon=True).start()

    def _read_stdout(self) -> None:
        assert self.process.stdout is not None
        try:
            while True:
                raw = self.process.stdout.readline(MAX_FRAME_BYTES + 2)
                if not raw:
                    break
                payload = raw[:-1] if raw.endswith(b"\n") else raw
                if len(payload) > MAX_FRAME_BYTES:
                    raise ProbeError("app-server emitted an oversized JSON-RPC frame")
                frame = json.loads(raw)
                if not isinstance(frame, dict):
                    raise ProbeError("app-server emitted a non-object JSON-RPC frame")
                self.frames.put(frame)
        except BaseException as error:
            self.frames.put(error)
        finally:
            self.frames.put(ProbeError("app-server stdout closed"))

    def _discard_stderr(self) -> None:
        assert self.process.stderr is not None
        for raw in iter(lambda: self.process.stderr.read(8192), b""):
            self.stderr_bytes += len(raw)

    def send(self, payload: dict[str, Any]) -> None:
        encoded = json.dumps(payload, separators=(",", ":")).encode() + b"\n"
        if len(encoded) > MAX_FRAME_BYTES:
            raise ProbeError("outgoing JSON-RPC frame is oversized")
        if self.process.stdin is None:
            raise ProbeError("app-server stdin is unavailable")
        self.process.stdin.write(encoded)
        self.process.stdin.flush()

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + self.timeout_seconds
        while True:
            frame = self.receive(deadline)
            if frame.get("id") == request_id:
                if "error" in frame:
                    error = frame.get("error")
                    code = error.get("code") if isinstance(error, dict) else None
                    raise ProbeError(f"{method} failed with JSON-RPC code {code!r}")
                result = frame.get("result")
                if not isinstance(result, dict):
                    raise ProbeError(f"{method} returned a non-object result")
                return result
            self.pending_notifications.append(frame)

    def notify(self, method: str) -> None:
        self.send({"method": method})

    def receive(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ProbeError("timed out waiting for app-server")
        try:
            item = self.frames.get(timeout=remaining)
        except queue.Empty as error:
            raise ProbeError("timed out waiting for app-server") from error
        if isinstance(item, BaseException):
            raise ProbeError(str(item))
        return item

    def next_event(self, deadline: float) -> dict[str, Any]:
        if self.pending_notifications:
            return self.pending_notifications.pop(0)
        return self.receive(deadline)

    def close(self) -> None:
        if self.process.stdin is not None:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)


def public_rate_limits(result: dict[str, Any]) -> dict[str, Any] | None:
    limits = result.get("rateLimits")
    if not isinstance(limits, dict):
        return None

    def window(name: str) -> dict[str, Any] | None:
        value = limits.get(name)
        if not isinstance(value, dict):
            return None
        return {
            "used_percent": value.get("usedPercent"),
            "window_minutes": value.get("windowDurationMins"),
            "resets_at": value.get("resetsAt"),
        }

    return {
        "primary": window("primary"),
        "secondary": window("secondary"),
        "reached": limits.get("rateLimitReachedType") is not None
        or limits.get("spendControlReached") is True,
    }


def public_usage(value: Any) -> dict[str, int] | None:
    if not isinstance(value, dict):
        return None
    return {
        name: int(value.get(name, 0))
        for name in (
            "inputTokens",
            "cachedInputTokens",
            "cacheWriteInputTokens",
            "outputTokens",
            "reasoningOutputTokens",
            "totalTokens",
        )
        if isinstance(value.get(name, 0), int)
    }


def run_probe(server: AppServer, work_dir: Path, requested_model: str | None) -> dict[str, Any]:
    server.request(
        "initialize",
        {
            "clientInfo": {
                "name": "apitoken_openai_compat",
                "title": "API Token OpenAI-compatible gateway live probe",
                "version": "probe",
            },
            "capabilities": {
                "experimentalApi": True,
                "requestAttestation": False,
                "mcpServerOpenaiFormElicitation": False,
            },
        },
    )
    server.notify("initialized")

    account = server.request("account/read", {"refreshToken": False})
    account_type = (
        account.get("account", {}).get("type")
        if isinstance(account.get("account"), dict)
        else None
    )
    # `requiresOpenaiAuth=true` describes the active provider. It is also true for a healthy,
    # authenticated ChatGPT account; absence is represented by a missing/null account.
    if account_type != "chatgpt":
        raise ProbeError("current Codex profile is not an authenticated ChatGPT account")

    rate_limits = public_rate_limits(server.request("account/rateLimits/read", {}))

    models: list[str] = []
    seen_cursors: set[str] = set()
    cursor: str | None = None
    for _ in range(32):
        page = server.request(
            "model/list",
            {"cursor": cursor, "limit": 100, "includeHidden": False},
        )
        data = page.get("data")
        if not isinstance(data, list):
            raise ProbeError("model/list omitted data")
        for entry in data:
            model = entry.get("model") if isinstance(entry, dict) else None
            if isinstance(model, str) and model not in models:
                models.append(model)
        cursor = page.get("nextCursor")
        if cursor is None:
            break
        if not isinstance(cursor, str) or not cursor or cursor in seen_cursors:
            raise ProbeError("model/list returned an invalid pagination cursor")
        seen_cursors.add(cursor)
    else:
        raise ProbeError("model/list exceeded pagination safety limit")

    if requested_model:
        if requested_model not in models:
            raise ProbeError("requested model is not in the live app-server catalog")
        model = requested_model
    else:
        model = next((candidate for candidate in PREFERRED_MODELS if candidate in models), None)
        if model is None:
            raise ProbeError("none of the gateway's pinned models is currently available")

    thread = server.request(
        "thread/start",
        {
            "model": model,
            "cwd": str(work_dir),
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "baseInstructions": "",
            "developerInstructions": None,
            "ephemeral": True,
            "historyMode": "legacy",
            "environments": [],
            "dynamicTools": [],
            "experimentalRawEvents": True,
        },
    )
    thread_id = (
        thread.get("thread", {}).get("id")
        if isinstance(thread.get("thread"), dict)
        else None
    )
    if not isinstance(thread_id, str) or not thread_id:
        raise ProbeError("thread/start omitted thread.id")
    served_model = thread.get("model", model)
    if served_model != model:
        raise ProbeError("app-server served an unexpected model")

    turn = server.request(
        "turn/start",
        {
            "threadId": thread_id,
            "input": [
                {
                    "type": "text",
                    "text": f"Reply with exactly {CANARY} and nothing else.",
                }
            ],
        },
    )
    turn_id = (
        turn.get("turn", {}).get("id") if isinstance(turn.get("turn"), dict) else None
    )
    if not isinstance(turn_id, str) or not turn_id:
        raise ProbeError("turn/start omitted turn.id")

    deadline = time.monotonic() + server.timeout_seconds
    output_text: list[str] = []
    usage: dict[str, int] | None = None
    status: str | None = None
    while status is None:
        event = server.next_event(deadline)
        if "id" in event and "method" in event:
            server.send(
                {
                    "id": event["id"],
                    "error": {
                        "code": -32601,
                        "message": "live probe does not execute callbacks",
                    },
                }
            )
            continue
        method = event.get("method")
        params = event.get("params")
        if not isinstance(params, dict):
            continue
        event_turn = params.get("turnId")
        if isinstance(event_turn, str) and event_turn != turn_id:
            continue
        if method == "rawResponseItem/completed":
            item = params.get("item")
            if isinstance(item, dict) and item.get("type") == "message":
                content = item.get("content")
                if isinstance(content, list):
                    for part in content:
                        if (
                            isinstance(part, dict)
                            and part.get("type") == "output_text"
                            and isinstance(part.get("text"), str)
                        ):
                            output_text.append(part["text"])
        elif method == "rawResponse/completed":
            usage = public_usage(params.get("usage"))
        elif method == "turn/completed":
            completed_turn = params.get("turn")
            if isinstance(completed_turn, dict):
                status = completed_turn.get("status")
            if not isinstance(status, str):
                raise ProbeError("turn/completed omitted status")

    text = "".join(output_text).strip()
    if status != "completed":
        raise ProbeError(f"live canary turn finished with status {status!r}")
    if text != CANARY:
        raise ProbeError("live canary output did not exactly match the requested text")

    return {
        "account_type": account_type,
        "model": model,
        "available_models": models,
        "rate_limits": rate_limits,
        "turn_status": status,
        "canary": text,
        "usage": usage,
    }


def main() -> int:
    args = parse_args()
    if args.timeout_seconds < 5 or args.timeout_seconds > 3600:
        raise ProbeError("--timeout-seconds must be between 5 and 3600")
    binary = args.binary.resolve(strict=True)
    owned_temp: tempfile.TemporaryDirectory[str] | None = None
    if args.work_dir:
        work_dir = args.work_dir.resolve(strict=True)
    else:
        owned_temp = tempfile.TemporaryDirectory(prefix="apitoken-codex-probe.")
        work_dir = Path(owned_temp.name)

    version = subprocess.run(
        [str(binary), "--version"],
        check=True,
        capture_output=True,
        text=True,
        env={"CODEX_HOME": os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))},
        timeout=20,
    ).stdout.strip()
    server = AppServer(binary, work_dir, args.timeout_seconds)
    try:
        result = run_probe(server, work_dir, args.model)
        result["binary_sha256"] = sha256(binary)
        result["version"] = version
        result["stderr_bytes_discarded"] = server.stderr_bytes
        print(json.dumps(result, indent=2, sort_keys=True))
    finally:
        server.close()
        if owned_temp is not None:
            owned_temp.cleanup()
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ProbeError, OSError, subprocess.SubprocessError) as error:
        print(f"probe failed: {error}", file=sys.stderr)
        raise SystemExit(1)
