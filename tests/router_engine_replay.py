#!/usr/bin/env python3
"""Keyless deterministic replay through real router and engine binaries."""

from __future__ import annotations

import argparse
import copy
import difflib
import http.client
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "tests" / "fixtures" / "router-engine-replay-v1.json"
ENGINE_BIN = Path(os.environ.get("CLAUDE_API_BIN", ROOT / "target" / "debug" / "claude-api"))
ROUTER_BIN = Path(os.environ.get("CLAUDE_ROUTER_BIN", ROOT / "target" / "debug" / "claude-router"))
ADMIN_KEY = "router-engine-replay-admin-key-000000000000"
MODEL = "anthropic/claude-haiku-4-5"
TOKEN = "faketoken-router-replay-only"
ID_PREFIXES = ("resp_", "msg_", "rs_", "fc_")


def fail(message: str) -> "None":
    raise AssertionError(message)


def free_port() -> int:
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_http(port: int, path: str, process: subprocess.Popen[bytes], log: Path) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            fail(f"process exited before readiness ({process.returncode}):\n{log.read_text(errors='replace')[-8000:]}")
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=0.5)
            connection.request("GET", path)
            response = connection.getresponse()
            response.read()
            connection.close()
            if response.status == 200:
                return
        except OSError:
            pass
        time.sleep(0.1)
    fail(f"readiness timeout for port {port}:\n{log.read_text(errors='replace')[-8000:]}")


def request(port: int, body: dict[str, Any]) -> tuple[int, str, bytes]:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    payload = json.dumps(body, separators=(",", ":")).encode("utf-8")
    connection.request(
        "POST",
        "/v1/responses",
        body=payload,
        headers={"content-type": "application/json", "x-api-key": ADMIN_KEY},
    )
    response = connection.getresponse()
    data = response.read()
    content_type = response.getheader("content-type", "")
    connection.close()
    return response.status, content_type, data


def visible_text(response: dict[str, Any]) -> str:
    pieces: list[str] = []
    for item in response.get("output", []):
        if not isinstance(item, dict) or item.get("type") != "message":
            continue
        for part in item.get("content", []):
            if isinstance(part, dict) and part.get("type") == "output_text" and isinstance(part.get("text"), str):
                pieces.append(part["text"])
    return "".join(pieces)


def parse_sse(data: bytes) -> list[dict[str, Any]]:
    text = data.decode("utf-8")
    frames: list[dict[str, Any]] = []
    for raw in text.replace("\r\n", "\n").split("\n\n"):
        if not raw.strip():
            continue
        event: str | None = None
        data_lines: list[str] = []
        comment: str | None = None
        for line in raw.splitlines():
            if line.startswith("event:"):
                event = line[6:].strip()
            elif line.startswith("data:"):
                data_lines.append(line[5:].strip())
            elif line.startswith(":"):
                comment = line[1:].strip()
        if comment is not None and not data_lines:
            frames.append({"comment": comment})
            continue
        if event is None or not data_lines:
            fail(f"malformed SSE frame: {raw!r}")
        payload = json.loads("\n".join(data_lines))
        if not isinstance(payload, dict) or payload.get("type") != event:
            fail(f"event/data type mismatch: {event!r} {payload!r}")
        frames.append({"event": event, "data": payload})
    return frames


def normalize(value: Any) -> Any:
    ids: dict[str, str] = {}
    counters: dict[str, int] = {}

    def walk(item: Any, key: str | None = None) -> Any:
        if isinstance(item, dict):
            return {name: walk(child, name) for name, child in sorted(item.items())}
        if isinstance(item, list):
            return [walk(child, key) for child in item]
        if key == "created_at" and isinstance(item, (int, float)):
            return "<created_at>"
        if key in {"id", "response_id", "item_id"} and isinstance(item, str):
            for prefix in ID_PREFIXES:
                if item.startswith(prefix):
                    if item not in ids:
                        counters[prefix] = counters.get(prefix, 0) + 1
                        ids[item] = f"{prefix}<id{counters[prefix]}>"
                    return ids[item]
        return item

    return walk(copy.deepcopy(value))


def guard_non_stream(response: dict[str, Any]) -> str:
    if response.get("object") != "response" or response.get("status") != "completed":
        fail(f"non-stream response is not completed: {response!r}")
    text = visible_text(response)
    if not text:
        fail("non-stream response has no visible output")
    if any(isinstance(item, dict) and item.get("type") == "error" for item in response.get("output", [])):
        fail("non-stream response is error-only")
    usage = response.get("usage")
    if not isinstance(usage, dict):
        fail("non-stream response omitted terminal usage")
    inputs, outputs, total = usage.get("input_tokens"), usage.get("output_tokens"), usage.get("total_tokens")
    if not all(isinstance(number, int) and number > 0 for number in (inputs, outputs, total)):
        fail(f"non-stream terminal usage is not positive: {usage!r}")
    if inputs + outputs != total:
        fail(f"non-stream usage arithmetic is invalid: {usage!r}")
    return text


def guard_stream(frames: list[dict[str, Any]], expected_text: str) -> None:
    events = [frame["event"] for frame in frames if "event" in frame]
    required = [
        "response.created",
        "response.in_progress",
        "response.output_item.added",
        "response.content_part.added",
        "response.output_text.delta",
        "response.output_text.done",
        "response.content_part.done",
        "response.output_item.done",
        "response.completed",
    ]
    position = -1
    for event in required:
        try:
            position = events.index(event, position + 1)
        except ValueError:
            fail(f"SSE lifecycle omitted or reordered {event}: {events}")
    forbidden = {"error", "response.failed"}
    if forbidden.intersection(events):
        fail(f"SSE contains terminal failure: {events}")
    payloads = [frame["data"] for frame in frames if "data" in frame]
    sequences = [payload.get("sequence_number") for payload in payloads]
    if sequences != list(range(len(sequences))):
        fail(f"SSE sequence numbers are not dense: {sequences}")
    deltas = [payload.get("delta") for payload in payloads if payload.get("type") == "response.output_text.delta"]
    if not deltas or not all(isinstance(delta, str) and delta for delta in deltas):
        fail(f"SSE has no non-empty text deltas: {deltas}")
    done = next(payload for payload in payloads if payload.get("type") == "response.output_text.done")
    completed = next(payload for payload in payloads if payload.get("type") == "response.completed")
    text = "".join(deltas)
    if text != done.get("text") or text != visible_text(completed.get("response", {})) or text != expected_text:
        fail(f"SSE text views diverge: deltas={text!r} done={done.get('text')!r}")
    usage = completed.get("response", {}).get("usage")
    if not isinstance(usage, dict) or usage.get("input_tokens", 0) <= 0 or usage.get("output_tokens", 0) <= 0:
        fail(f"SSE completed omitted terminal usage: {completed!r}")
    if not any(frame.get("comment") == "ping" for frame in frames):
        fail("SSE replay omitted the heartbeat comment")


def canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def stop(process: subprocess.Popen[bytes] | None) -> None:
    if process is None or process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)


def run_scenario() -> dict[str, Any]:
    engine: subprocess.Popen[bytes] | None = None
    router: subprocess.Popen[bytes] | None = None
    mock: subprocess.Popen[bytes] | None = None
    logs: list[Any] = []
    with tempfile.TemporaryDirectory(prefix="router-engine-replay-") as raw:
        temp = Path(raw)
        os.chmod(temp, 0o700)
        try:
            config = temp / "config"
            engine_spool = temp / "engine-spool"
            router_spool = temp / "router-spool"
            for directory in (config, engine_spool, router_spool):
                directory.mkdir(mode=0o700)
            token_file = temp / "token"
            token_file.write_text(TOKEN + "\n", encoding="utf-8")
            os.chmod(token_file, 0o600)
            base_env = {
                **os.environ,
                "HOME": str(temp),
                "SUB_CFG_DIR": str(config),
                "SUBS_DB": str(config / "subscriptions.db"),
            }
            subprocess.run(
                [
                    str(ENGINE_BIN),
                    "sub",
                    "add-file",
                    "replay@example.invalid",
                    "--token-file",
                    str(token_file),
                    "--fleet",
                    "replay",
                ],
                env=base_env,
                check=True,
                stdout=subprocess.DEVNULL,
            )
            subprocess.run(
                [str(ENGINE_BIN), "sub", "set-plan", "replay@example.invalid", "max20"],
                env=base_env,
                check=True,
                stdout=subprocess.DEVNULL,
            )
            mock_ready, mock_result = temp / "mock.ready", temp / "mock.result"
            mock_log = (temp / "mock.log").open("wb")
            logs.append(mock_log)
            mock = subprocess.Popen(
                [
                    sys.executable,
                    str(ROOT / "tests" / "router_engine_replay_mock.py"),
                    "--ready-file",
                    str(mock_ready),
                    "--result-file",
                    str(mock_result),
                ],
                stdout=mock_log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not mock_ready.exists():
                if mock.poll() is not None:
                    fail("deterministic mock exited before readiness")
                time.sleep(0.05)
            if not mock_ready.exists():
                fail("deterministic mock readiness timeout")
            mock_port = int(mock_ready.read_text(encoding="utf-8"))
            engine_port, router_port = free_port(), free_port()
            engine_log_path, router_log_path = temp / "engine.log", temp / "router.log"
            engine_log, router_log = engine_log_path.open("wb"), router_log_path.open("wb")
            logs.extend((engine_log, router_log))
            engine_env = {
                **base_env,
                "SUBS_FLEET": "replay",
                "CLAUDE_API_PROVIDER": "anthropic",
                "CLAUDE_API_BODY_SPOOL_ROOT": str(engine_spool),
                "CLAUDE_API_HOST": "127.0.0.1",
                "CLAUDE_API_PORT": str(engine_port),
                "CLAUDE_API_KEYS": ADMIN_KEY,
                "CLAUDE_API_BILLING": "0",
                "CLAUDE_API_POLL": "0",
                "CLAUDE_API_GEMINI_ENABLED": "0",
                "CLAUDE_API_CODEX_ENABLED": "0",
                "CLAUDE_API_KIMI_ENABLED": "0",
                "CLAUDE_API_GLM_ENABLED": "0",
                "CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED": "0",
                "CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED": "0",
                "CLAUDE_API_UPSTREAM": f"http://127.0.0.1:{mock_port}",
                "CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM": "1",
            }
            engine = subprocess.Popen(
                [str(ENGINE_BIN), "serve"],
                env=engine_env,
                stdout=engine_log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            wait_http(engine_port, "/health", engine, engine_log_path)
            router_env = {
                **os.environ,
                "HOME": str(temp),
                "CLAUDE_ROUTER_BODY_SPOOL_ROOT": str(router_spool),
                "CLAUDE_ROUTER_HOST": "127.0.0.1",
                "CLAUDE_ROUTER_PORT": str(router_port),
                "CLAUDE_ROUTER_ANTHROPIC_ORIGIN": f"http://127.0.0.1:{engine_port}",
                "CLAUDE_ROUTER_OPENAI_ORIGIN": "http://127.0.0.1:1",
                "CLAUDE_ROUTER_GEMINI_ORIGIN": "http://127.0.0.1:2",
            }
            router = subprocess.Popen(
                [str(ROUTER_BIN)],
                env=router_env,
                stdout=router_log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            wait_http(router_port, "/health", router, router_log_path)

            status, content_type, body = request(
                router_port, {"model": MODEL, "input": "produce replay output"}
            )
            if status != 200 or "application/json" not in content_type:
                fail(f"non-stream request failed: {status} {content_type} {body!r}")
            non_stream = json.loads(body)
            first_output = guard_non_stream(non_stream)
            status, content_type, body = request(
                router_port, {"model": MODEL, "stream": True, "input": first_output}
            )
            if status != 200 or "text/event-stream" not in content_type:
                fail(f"stream request failed: {status} {content_type} {body!r}")
            frames = parse_sse(body)
            guard_stream(frames, first_output)
            transcript = normalize(
                {
                    "schema_version": 1,
                    "scenario": "router-engine-responses-replay",
                    "requests": [
                        {"model": MODEL, "stream": False, "input": "produce replay output"},
                        {"model": MODEL, "stream": True, "input": first_output},
                    ],
                    "expected": {"non_stream": non_stream, "sse": frames},
                }
            )
            stop(router)
            router = None
            stop(engine)
            engine = None
            stop(mock)
            mock = None
            for log in logs:
                log.close()
            logs.clear()
            mock_result_value = json.loads(mock_result.read_text(encoding="utf-8"))
            if mock_result_value != {"failure": None, "requests": 2}:
                fail(f"mock semantic replay guard failed: {mock_result_value!r}")
            return transcript
        finally:
            stop(router)
            stop(engine)
            stop(mock)
            for log in logs:
                log.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update-fixture", action="store_true")
    args = parser.parse_args()
    if args.update_fixture and os.environ.get("CI"):
        fail("fixture update mode is forbidden in CI")
    for binary in (ENGINE_BIN, ROUTER_BIN):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            fail(f"built binary is required: {binary}")
    fixture_before = FIXTURE.read_bytes() if FIXTURE.exists() else b""
    rendered = canonical_json(run_scenario())
    if args.update_fixture:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        temporary = FIXTURE.with_suffix(FIXTURE.suffix + ".tmp")
        temporary.write_text(rendered, encoding="utf-8")
        os.replace(temporary, FIXTURE)
        print(f"updated {FIXTURE.relative_to(ROOT)}")
    else:
        expected = FIXTURE.read_text(encoding="utf-8")
        if rendered != expected:
            diff = "".join(
                difflib.unified_diff(
                    expected.splitlines(True),
                    rendered.splitlines(True),
                    fromfile="fixture",
                    tofile="actual",
                )
            )
            fail(f"replay transcript changed; review and use --update-fixture explicitly:\n{diff}")
        if FIXTURE.read_bytes() != fixture_before:
            fail("read-only replay modified its fixture")
        print(
            "router-engine replay passed: non-stream, SSE lifecycle, terminal usage, "
            "semantic prompt echo, fixture"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
