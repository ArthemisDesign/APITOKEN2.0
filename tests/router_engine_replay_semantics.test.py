#!/usr/bin/env python3
"""Mutation tests for replay semantic guards, independent from subprocess orchestration."""

from __future__ import annotations

import importlib.util
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("router_engine_replay.py")
spec = importlib.util.spec_from_file_location("router_engine_replay", MODULE_PATH)
assert spec and spec.loader
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)
fixture = replay.json.loads(replay.FIXTURE.read_text(encoding="utf-8"))

non_stream = fixture["expected"]["non_stream"]
assert replay.guard_non_stream(non_stream) == "replay output"

missing_usage = replay.copy.deepcopy(non_stream)
missing_usage.pop("usage", None)
try:
    replay.guard_non_stream(missing_usage)
except AssertionError:
    pass
else:
    raise AssertionError("non-stream missing terminal usage passed semantic guard")

frames = fixture["expected"]["sse"]
replay.guard_stream(frames, "replay output")

missing_completed = [
    frame for frame in replay.copy.deepcopy(frames) if frame.get("event") != "response.completed"
]
try:
    replay.guard_stream(missing_completed, "replay output")
except AssertionError:
    pass
else:
    raise AssertionError("SSE missing response.completed passed semantic guard")

error_only = replay.copy.deepcopy(non_stream)
error_only["output"] = [{"type": "error"}]
try:
    replay.guard_non_stream(error_only)
except AssertionError:
    pass
else:
    raise AssertionError("error-only non-stream result passed semantic guard")

print("router-engine replay semantic mutation tests passed")
