#!/usr/bin/env python3
"""Fail-closed live calibration runner for a GLM Coding Plan subscription (Z.ai / bigmodel.cn).

The runner talks DIRECTLY to the provider. It never touches the engine, engine PostgreSQL,
synthetic calibration vectors or any customer traffic: it reads the provider and prints
evidence for the operator. The target is exactly one subscription, pinned by `--profile`
(opaque operator label) + `--base-url` (allowlisted `https://api.z.ai` or
`https://open.bigmodel.cn`) + the API key from the `GLM_CALIBRATION_API_KEY` environment
variable. The key is never accepted on argv, never logged and never written to the checkpoint
or the report.

Dry-run is the default and sends no paid traffic. `--execute` runs the paid matrix: the three
subscription models (`glm-5.2`, `glm-5-turbo`, `glm-4.7`) x (non-stream, incremental stream)
against the Anthropic-compatible route `POST {base}/api/anthropic/v1/messages`. Every leg is
preceded by an integer-nanoUSD worst-case bound computed from the reviewed official rate card
(the same numbers as `crates/metering/src/glm.rs`); the bound is printed before dispatch. The
free read-only quota endpoint `GET {base}/api/monitor/usage/quota/limit` (Authorization header
WITHOUT the Bearer prefix; an invalid key comes back as HTTP 200 with `code: 401` in the body)
is polled before and after every paid leg. A quota delta is attributed to the served model
only when every moved counter matches the official credits formula exactly; anything else is
recorded as `unattributed` and the paid matrix stops — ambiguity is never guessed away.

Only read-only quota polls retry on transport errors. A paid request is never retried after a
transport ambiguity: the leg is held at its full worst-case bound and is not re-sent, even on
resume. A machine-readable JSON checkpoint is written after every leg, so `--resume` continues
the same run id without repeating completed legs.
"""

from __future__ import annotations

import argparse
import dataclasses
import http.client
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path
from typing import Any


NANO_PER_USD = 1_000_000_000
# $0.05 default ceiling: the AGENTS.md admission micro-smoke scale. `--i-understand` raises the
# authorized limit to an absolute $5 ceiling, mirroring how tools/claude_calibration encodes
# its authorized limit as a hard-coded constant.
MAX_BUDGET_NANO = 50_000_000
ACKNOWLEDGED_MAX_BUDGET_NANO = 5 * NANO_PER_USD
DEFAULT_BUDGET_USD = "0.05"
SAFE_READ_ATTEMPTS = 3
SAFE_READ_RETRY_DELAY_SECONDS = 2.0
DEFAULT_QUOTA_POLL_DELAY_SECONDS = 5.0
DEFAULT_HTTP_TIMEOUT_SECONDS = 120
# Provider-side message framing tokens are not pre-countable without a tokenizer; this fixed
# per-message overhead plus the exact prompt byte length keeps the input bound conservative
# (every ASCII prompt byte covers at least one token).
INPUT_OVERHEAD_TOKENS = 32
MAX_MAX_TOKENS = 1_024

KEY_ENV = "GLM_CALIBRATION_API_KEY"
ALLOWED_BASE_URLS = ("https://api.z.ai", "https://open.bigmodel.cn")
QUOTA_PATH = "/api/monitor/usage/quota/limit"
MESSAGES_PATH = "/api/anthropic/v1/messages"
ANTHROPIC_VERSION = "2023-06-01"
ANTHROPIC_BETA = "claude-code-20250219"
USER_AGENT = "claude-cli/2.1.195 (external, sdk-cli)"

# Official GLM Coding Plan error codes (docs.z.ai/api-reference/api-code, reviewed 2026-08-03).
QUOTA_WALL_CODES = {"1308": "five-hour window", "1310": "weekly/monthly window"}
MODEL_NOT_IN_PLAN_CODE = "1311"

MODEL_ORDER = ("glm-5.2", "glm-5-turbo", "glm-4.7")
TERMINAL_LEG_STATUSES = frozenset({"ok", "unavailable", "held-ambiguous", "failed"})

CHECKPOINT_SCHEMA = "glm-live-calibration-checkpoint/v1"
REPORT_SCHEMA = "glm-live-calibration/v1"
PLAN_SCHEMA = "glm-live-calibration-plan/v1"

PROFILE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
RUN_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,79}$")


class CalibrationError(RuntimeError):
    """A fail-closed calibration invariant was not satisfied."""


class TransportFailureError(CalibrationError):
    """Transport failed. On a read-only poll it is retriable; on a paid request it is an
    ambiguity after which the request is never repeated automatically."""


class HttpCalibrationError(CalibrationError):
    """A typed provider response that is safe to classify without parsing log text."""

    def __init__(
        self, path: str, status: int, detail: str, business_code: str | None = None
    ) -> None:
        super().__init__(f"{path} returned HTTP {status}: {detail}")
        self.path = path
        self.status = status
        self.detail = detail
        self.business_code = business_code


class QuotaKeyInvalid(CalibrationError):
    """The quota endpoint answered HTTP 200 with `code: 401` in the body."""


class QuotaShapeError(CalibrationError):
    """The quota response does not match the documented credits-form wrapper; fail closed."""


class PaidLegError(CalibrationError):
    """A paid request was dispatched but its outcome cannot be proved. The leg is held at its
    full worst-case bound and never re-sent."""

    def __init__(self, reason: str, upper_bound_nano: int) -> None:
        super().__init__(reason)
        self.upper_bound_nano = upper_bound_nano


class PostSpendPollError(CalibrationError):
    """The paid leg succeeded and was charged, but the attribution poll failed afterwards."""


@dataclasses.dataclass(frozen=True)
class GlmRates:
    """Reviewed official rate card row, mirroring `crates/metering/src/glm.rs`.

    `*_nano` are nanoUSD per token (`$/M tokens * 1000`). Cache write carries the miss rate
    because GLM publishes no paid cache-write leg ("Limited-time Free" storage). Credit
    multipliers are exact rationals in tenths; `None` means the provider published no
    multipliers for the id and the credits ledger must fail closed.
    """

    input_nano: int
    cached_input_nano: int
    output_nano: int
    credit_input_tenths: int | None
    credit_cached_tenths: int | None
    credit_output_tenths: int | None
    input_token_limit: int


RATE_CARD = {
    "glm-5.2": GlmRates(1_400, 260, 4_400, 69, 17, 240, 1_000_000),
    "glm-5-turbo": GlmRates(1_200, 240, 4_000, 57, 15, 210, 200_000),
    "glm-4.7": GlmRates(600, 110, 2_200, 46, 12, 160, 200_000),
    # Served-only ids: the provider silently re-routes glm-5.1/glm-5 to glm-5.2. They are priced
    # so an echoed served id still resolves, but without published credit multipliers the
    # credits leg fails closed — exactly like the metering crate.
    "glm-5.1": GlmRates(1_400, 260, 4_400, None, None, None, 1_000_000),
    "glm-5": GlmRates(1_000, 200, 3_200, None, None, None, 1_000_000),
}


@dataclasses.dataclass(frozen=True)
class Leg:
    name: str
    model: str
    stream: bool
    max_tokens: int


@dataclasses.dataclass
class Budget:
    limit_nano: int
    spent_nano: int = 0
    held_nano: int = 0

    def committed_nano(self) -> int:
        return self.spent_nano + self.held_nano

    def require_room(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if self.committed_nano() + upper_bound_nano > self.limit_nano:
            raise CalibrationError(
                "budget guard stopped before dispatch: insufficient room in the run budget"
            )

    def charge(self, actual_nano: int) -> None:
        if actual_nano <= 0:
            raise CalibrationError("priced usage must be positive")
        if self.committed_nano() + actual_nano > self.limit_nano:
            raise CalibrationError("provider evidence exceeded the run budget")
        self.spent_nano += actual_nano

    def hold(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if self.committed_nano() + upper_bound_nano > self.limit_nano:
            raise CalibrationError("worst-case hold exceeded the run budget")
        self.held_nano += upper_bound_nano


def as_int(value: Any, field: str) -> int:
    if isinstance(value, bool):
        raise CalibrationError(f"{field} is boolean, expected integer")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as error:
        raise CalibrationError(f"{field} is not an integer: {value!r}") from error
    if parsed < 0:
        raise CalibrationError(f"{field} is negative")
    return parsed


def usd_to_nano(value: str) -> int:
    whole, dot, fractional = value.strip().partition(".")
    if not whole.isdigit() or (dot and not fractional.isdigit()):
        raise CalibrationError(f"invalid USD amount: {value!r}")
    fraction = (fractional + "000000000")[:9]
    return int(whole) * NANO_PER_USD + int(fraction)


def fmt_usd(nano: int) -> str:
    whole, fraction = divmod(nano, NANO_PER_USD)
    if fraction == 0:
        return f"${whole}"
    return f"${whole}.{fraction:09d}".rstrip("0")


def is_peak_sgt(unix_secs: int) -> bool:
    """Official off-peak rule: peak is Monday-Friday 14:00 inclusive to 18:00 exclusive SGT
    (UTC+8). Mirrors `glm_is_peak_utc` in crates/metering/src/glm.rs."""
    sgt = unix_secs + 8 * 3_600
    days, secs_of_day = divmod(sgt, 86_400)
    weekday = (days + 3) % 7  # 1970-01-01 was a Thursday; Monday = 0
    hour = secs_of_day // 3_600
    return weekday <= 4 and 14 <= hour < 18


def usage_from_value(raw: dict[str, Any]) -> dict[str, int]:
    """Parse one usage object. Mirrors `usage_from_value` in crates/metering/src/glm.rs:
    tolerant of absent cache fields, sums a TTL-split cache_creation object, and keeps the
    larger reasoning counter spelling."""
    split = raw.get("cache_creation")
    split_total = 0
    if isinstance(split, dict):
        for value in split.values():
            if isinstance(value, int) and not isinstance(value, bool) and value > 0:
                split_total += value
    cache_write = split_total if split_total > 0 else as_int(
        raw.get("cache_creation_input_tokens", 0), "usage.cache_creation_input_tokens"
    )
    return {
        "input_tokens": as_int(raw.get("input_tokens", 0), "usage.input_tokens"),
        "cache_read_tokens": as_int(
            raw.get("cache_read_input_tokens", 0), "usage.cache_read_input_tokens"
        ),
        "cache_write_tokens": cache_write,
        "output_tokens": as_int(raw.get("output_tokens", 0), "usage.output_tokens"),
        "reasoning_output_tokens": max(
            as_int(raw.get("reasoning_tokens", 0), "usage.reasoning_tokens"),
            as_int(raw.get("reasoning_output_tokens", 0), "usage.reasoning_output_tokens"),
        ),
    }


def validate_leg_usage(usage: dict[str, int]) -> None:
    if usage["output_tokens"] <= 0:
        raise CalibrationError("output token class was not observed")
    if (
        usage["input_tokens"] + usage["cache_read_tokens"] + usage["cache_write_tokens"]
        <= 0
    ):
        raise CalibrationError("input token class was not observed")
    if usage["reasoning_output_tokens"] > usage["output_tokens"]:
        raise CalibrationError("reasoning exceeds output; the subset invariant is broken")


def api_cost_nano(usage: dict[str, int], rates: GlmRates) -> int:
    """Exact official replacement cost in nanoUSD. Legs are disjoint; cache write carries the
    miss rate; reasoning is a non-billed subset of output."""
    if usage["reasoning_output_tokens"] > usage["output_tokens"]:
        raise CalibrationError("reasoning exceeds output; the subset invariant is broken")
    return (
        usage["input_tokens"] * rates.input_nano
        + usage["cache_read_tokens"] * rates.cached_input_nano
        + usage["cache_write_tokens"] * rates.input_nano
        + usage["output_tokens"] * rates.output_nano
    )


def credits_micro_expected(usage: dict[str, int], rates: GlmRates, off_peak: bool) -> int:
    """Official native credits in fixed-point micro-credits:
    `credits = (input x in_mult + cached x cache_mult + output x out_mult) / 10_000` with
    tenths-stored multipliers is exactly `weighted x 10` micro-credits, and off-peak is the
    exact half `weighted x 5`."""
    if (
        rates.credit_input_tenths is None
        or rates.credit_cached_tenths is None
        or rates.credit_output_tenths is None
    ):
        raise CalibrationError("served model has no published credit multipliers")
    if usage["reasoning_output_tokens"] > usage["output_tokens"]:
        raise CalibrationError("reasoning exceeds output; the subset invariant is broken")
    weighted = (
        usage["input_tokens"] * rates.credit_input_tenths
        + usage["cache_read_tokens"] * rates.credit_cached_tenths
        + usage["output_tokens"] * rates.credit_output_tenths
    )
    return weighted * (5 if off_peak else 10)


def whole_credits(micro: int) -> int:
    return (micro + 500_000) // 1_000_000


def parse_sse_events(raw_lines: list[tuple[float, bytes]]) -> list[tuple[float, dict[str, Any]]]:
    events: list[tuple[float, dict[str, Any]]] = []
    for ts, line in raw_lines:
        text = line.decode(errors="replace").strip()
        if not text.startswith("data:"):
            continue
        payload = text[len("data:"):].strip()
        if not payload or payload == "[DONE]":
            continue
        try:
            obj = json.loads(payload)
        except json.JSONDecodeError as error:
            raise CalibrationError("stream returned a malformed data frame") from error
        if not isinstance(obj, dict):
            raise CalibrationError("stream returned a non-object data frame")
        events.append((ts, obj))
    return events


def merge_stream_usage(events: list[tuple[float, dict[str, Any]]]) -> dict[str, int] | None:
    """Accumulate terminal usage from SSE events. Mirrors `merge_stream_event` in the metering
    crate: output is replaced, never summed, because the provider reports it cumulatively."""
    usage: dict[str, int] | None = None
    for _, obj in events:
        candidate = None
        message = obj.get("message")
        if isinstance(message, dict) and isinstance(message.get("usage"), dict):
            candidate = message["usage"]
        if candidate is None and isinstance(obj.get("usage"), dict):
            candidate = obj["usage"]
        if candidate is None:
            continue
        parsed = usage_from_value(candidate)
        if usage is None:
            usage = {field: 0 for field in parsed}
        for field, value in parsed.items():
            if value > 0:
                usage[field] = value
    return usage


def usage_keys_from_events(events: list[tuple[float, dict[str, Any]]]) -> list[str]:
    keys: set[str] = set()
    for _, obj in events:
        message = obj.get("message")
        if isinstance(message, dict) and isinstance(message.get("usage"), dict):
            keys.update(str(key) for key in message["usage"])
        if isinstance(obj.get("usage"), dict):
            keys.update(str(key) for key in obj["usage"])
    return sorted(keys)


def served_model_from_events(events: list[tuple[float, dict[str, Any]]]) -> str | None:
    for _, obj in events:
        message = obj.get("message")
        if isinstance(message, dict) and isinstance(message.get("model"), str):
            return message["model"]
    return None


def stream_evidence(
    events: list[tuple[float, dict[str, Any]]], first_ts: float, last_ts: float
) -> dict[str, Any]:
    types = sorted({str(obj.get("type")) for _, obj in events if obj.get("type")})
    delta_frames = 0
    text_delta_frames = 0
    for _, obj in events:
        if obj.get("type") != "content_block_delta":
            continue
        delta_frames += 1
        delta = obj.get("delta")
        if isinstance(delta, dict) and delta.get("type") == "text_delta":
            text_delta_frames += 1
    return {
        "frames": len(events),
        "event_types": types,
        "content_delta_frames": delta_frames,
        "text_delta_frames": text_delta_frames,
        "first_to_last_ms": int((last_ts - first_ts) * 1000),
        # Two or more distinct text deltas prove the provider did not buffer the whole answer
        # into a single frame.
        "incremental_evidence": text_delta_frames >= 2,
    }


def parse_quota_observation(payload: Any) -> dict[str, Any]:
    """Validate and normalize the quota endpoint wrapper `{code, msg, success, data.limits[]}`.

    HTTP 200 with `code: 401` in the body means an invalid key. A legacy/Team-plan or otherwise
    unrecognized shape fails closed instead of being silently interpreted."""
    if not isinstance(payload, dict):
        raise QuotaShapeError("quota response is not an object")
    code = payload.get("code")
    if code == 401 or code == "401":
        raise QuotaKeyInvalid("quota endpoint rejected the key (HTTP 200 with code 401 in body)")
    if payload.get("success") is not True:
        raise QuotaShapeError(
            f"quota response reported failure: code={code!r} msg={payload.get('msg')!r}"
        )
    data = payload.get("data")
    if not isinstance(data, dict):
        raise QuotaShapeError("quota response has no data object")
    raw_limits = data.get("limits")
    if raw_limits is None:
        raise QuotaShapeError("quota response has no limits list")
    if not isinstance(raw_limits, list):
        raise QuotaShapeError("quota limits is not a list")
    limits: list[dict[str, Any]] = []
    notes: list[str] = []
    for raw in raw_limits:
        if not isinstance(raw, dict):
            raise QuotaShapeError("quota limit entry is not an object")
        entry_type = raw.get("type")
        if not isinstance(entry_type, str) or not entry_type:
            raise QuotaShapeError("quota limit entry has no type")
        entry: dict[str, Any] = {"type": entry_type, "unit": raw.get("unit")}
        for field in ("number", "usage", "currentValue", "remaining", "nextResetTime"):
            value = raw.get(field)
            entry[field] = None if value is None else as_int(value, f"quota.{field}")
        entry["percentage"] = raw.get("percentage")
        details: list[dict[str, Any]] = []
        raw_details = raw.get("usageDetails")
        if raw_details is not None and not isinstance(raw_details, list):
            raise QuotaShapeError("quota usageDetails is not a list")
        for raw_detail in raw_details or []:
            if not isinstance(raw_detail, dict) or not isinstance(
                raw_detail.get("modelCode"), str
            ):
                notes.append("dropped a malformed usageDetails entry")
                continue
            counters: dict[str, int] = {}
            for key, value in raw_detail.items():
                if key == "modelCode":
                    continue
                if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                    notes.append(f"dropped a non-counter usageDetails field: {key}")
                    continue
                counters[key] = value
            details.append({"modelCode": raw_detail["modelCode"], "counters": counters})
        entry["usageDetails"] = details
        limits.append(entry)
    if not limits:
        notes.append("quota endpoint returned an empty limits list")
    return {"code": code, "msg": payload.get("msg"), "limits": limits, "notes": notes}


def quota_counters(observation: dict[str, Any]) -> dict[tuple[str, ...], int]:
    """Flat comparable counters of one quota observation, keyed by tuples so model ids with
    dots stay unambiguous."""
    counters: dict[tuple[str, ...], int] = {}
    for entry in observation["limits"]:
        entry_type = entry["type"]
        for field in ("usage", "currentValue", "remaining"):
            value = entry.get(field)
            if value is None:
                continue
            key = ("limits", entry_type, field)
            if key in counters:
                raise CalibrationError(f"duplicate quota entry identity: {entry_type}")
            counters[key] = value
        for detail in entry.get("usageDetails", []):
            for field, value in detail["counters"].items():
                key = ("details", entry_type, detail["modelCode"], field)
                if key in counters:
                    raise CalibrationError(
                        f"duplicate quota detail identity: {entry_type}/{detail['modelCode']}"
                    )
                counters[key] = value
    return counters


def counter_key_text(key: tuple[str, ...]) -> str:
    return "|".join(key)


def attribute_quota_delta(
    before: dict[str, Any],
    after: dict[str, Any],
    expected_whole_credits: int,
    served_model: str,
) -> tuple[str, dict[str, int], str]:
    """Attribute a quota delta to the served model only when every moved counter matches the
    official credits formula exactly. Anything else fails closed as `unattributed`."""
    before_counters = quota_counters(before)
    after_counters = quota_counters(after)
    if set(before_counters) != set(after_counters):
        return (
            "unattributed",
            {},
            "quota counter set changed between the before/after observations",
        )
    deltas: dict[str, int] = {}
    for key in sorted(before_counters):
        delta = after_counters[key] - before_counters[key]
        if key[-1] == "remaining":
            delta = -delta
        if delta:
            deltas[counter_key_text(key)] = delta
    if not deltas:
        if expected_whole_credits == 0:
            return (
                "below-resolution",
                {},
                "expected sub-credit movement and quota counters did not move",
            )
        return (
            "unattributed",
            {},
            f"expected {expected_whole_credits} credit(s) of movement but quota counters "
            "did not move",
        )
    for key, delta in deltas.items():
        if delta != expected_whole_credits:
            return (
                "unattributed",
                deltas,
                f"counter {key} moved by {delta}, expected exactly "
                f"{expected_whole_credits} (foreign traffic or unknown units)",
            )
        if key.startswith("details|"):
            model = key.split("|")[2]
            if model != served_model:
                return (
                    "unattributed",
                    deltas,
                    f"per-model counter moved for {model}, not the served {served_model}",
                )
    return (
        "attributed",
        deltas,
        "every moved counter matches the official credits formula for the served model",
    )


def business_code_from_detail(detail: str) -> str | None:
    try:
        payload = json.loads(detail)
    except json.JSONDecodeError:
        return None
    if not isinstance(payload, dict):
        return None
    error = payload.get("error")
    code = None
    if isinstance(error, dict):
        code = error.get("code")
    if code is None:
        code = payload.get("code")
    return str(code) if code is not None else None


def body_for_leg(leg: Leg, run_id: str) -> dict[str, Any]:
    body: dict[str, Any] = {
        "model": leg.model,
        "max_tokens": leg.max_tokens,
        "messages": [
            {
                "role": "user",
                "content": (
                    f"GLM calibration run {run_id}, leg {leg.name}. "
                    "Reply with exactly CALIBRATION_OK."
                ),
            }
        ],
    }
    if leg.stream:
        body["stream"] = True
    return body


def input_token_bound(body: dict[str, Any]) -> int:
    total = 0
    messages = body["messages"]
    for message in messages:
        total += len(str(message["content"]).encode("utf-8"))
    return total + INPUT_OVERHEAD_TOKENS * len(messages)


def worst_case_nano(leg: Leg, rates: GlmRates, input_bound: int) -> int:
    return input_bound * rates.input_nano + leg.max_tokens * rates.output_nano


def build_legs(models: list[str], max_tokens: int) -> list[Leg]:
    legs: list[Leg] = []
    for model in models:
        legs.append(Leg(f"messages:{model}", model, False, max_tokens))
        legs.append(Leg(f"messages-stream:{model}", model, True, max_tokens))
    return legs


def normalize_base_url(raw: str) -> str:
    candidate = raw.strip().rstrip("/").lower()
    if candidate not in ALLOWED_BASE_URLS:
        raise CalibrationError(
            "--base-url must be exactly https://api.z.ai or https://open.bigmodel.cn"
        )
    return candidate


def validate_profile(raw: str) -> str:
    if not PROFILE_RE.match(raw):
        raise CalibrationError(
            "--profile must be an opaque label of 1-64 chars: alnum, dot, dash, underscore"
        )
    return raw


def new_run_id() -> str:
    return f"glm-cal-{int(time.time())}-{uuid.uuid4().hex[:8]}"


class GlmClient:
    def __init__(self, base_url: str, api_key: str, timeout: int) -> None:
        self.base_url = base_url
        self.api_key = api_key
        self.timeout = timeout

    def _send(
        self, path: str, method: str, body: dict[str, Any] | None, headers: dict[str, str]
    ):
        data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=data, headers=headers, method=method
        )
        try:
            return urllib.request.urlopen(request, timeout=self.timeout)
        except urllib.error.HTTPError as error:
            raw = error.read(800).decode(errors="replace")
            detail = raw.replace(self.api_key, "***") if self.api_key else raw
            raise HttpCalibrationError(
                path, error.code, detail, business_code_from_detail(detail)
            ) from error
        except (urllib.error.URLError, OSError, http.client.HTTPException) as error:
            raise TransportFailureError(f"{path} transport failed: {error}") from error

    def _read_body(self, response: Any, path: str) -> bytes:
        try:
            with response:
                return response.read()
        except (urllib.error.URLError, OSError, http.client.HTTPException) as error:
            raise TransportFailureError(f"{path} transport failed mid-body: {error}") from error

    def quota(self) -> dict[str, Any]:
        # The quota endpoint takes the raw key in Authorization WITHOUT a Bearer prefix. It is
        # a monitor surface and deliberately carries no generation identity headers.
        response = self._send(
            QUOTA_PATH,
            "GET",
            None,
            {"authorization": self.api_key, "accept": "application/json"},
        )
        raw = self._read_body(response, QUOTA_PATH)
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise TransportFailureError("quota endpoint returned invalid JSON") from error
        return parse_quota_observation(payload)

    def quota_with_retry(self) -> dict[str, Any]:
        for attempt in range(SAFE_READ_ATTEMPTS):
            try:
                return self.quota()
            except TransportFailureError:
                if attempt + 1 == SAFE_READ_ATTEMPTS:
                    raise
                time.sleep(SAFE_READ_RETRY_DELAY_SECONDS)
        raise CalibrationError("unreachable quota retry state")

    def _generation_headers(self, stream: bool) -> dict[str, str]:
        return {
            "authorization": f"Bearer {self.api_key}",
            "user-agent": USER_AGENT,
            "anthropic-version": ANTHROPIC_VERSION,
            "anthropic-beta": ANTHROPIC_BETA,
            "content-type": "application/json",
            "accept": "text/event-stream" if stream else "application/json",
        }

    def generate(self, body: dict[str, Any]) -> dict[str, Any]:
        response = self._send(MESSAGES_PATH, "POST", body, self._generation_headers(False))
        raw = self._read_body(response, MESSAGES_PATH)
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{MESSAGES_PATH} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{MESSAGES_PATH} returned a non-object")
        return payload

    def generate_stream(
        self, body: dict[str, Any]
    ) -> tuple[list[tuple[float, dict[str, Any]]], float, float]:
        response = self._send(MESSAGES_PATH, "POST", body, self._generation_headers(True))
        raw_lines: list[tuple[float, bytes]] = []
        first_ts: float | None = None
        last_ts: float | None = None
        try:
            with response:
                while True:
                    line = response.readline()
                    if not line:
                        break
                    ts = time.monotonic()
                    if first_ts is None:
                        first_ts = ts
                    last_ts = ts
                    raw_lines.append((ts, line))
        except (urllib.error.URLError, OSError, http.client.HTTPException) as error:
            raise TransportFailureError(f"{MESSAGES_PATH} stream interrupted: {error}") from error
        if first_ts is None or last_ts is None:
            raise CalibrationError(f"{MESSAGES_PATH} stream returned no frames")
        return parse_sse_events(raw_lines), first_ts, last_ts


class Runner:
    def __init__(
        self, client: GlmClient, budget: Budget, run_id: str, poll_delay: float
    ) -> None:
        self.client = client
        self.budget = budget
        self.run_id = run_id
        self.poll_delay = poll_delay

    def execute_leg(self, leg: Leg) -> dict[str, Any]:
        rates = RATE_CARD[leg.model]
        body = body_for_leg(leg, self.run_id)
        bound = input_token_bound(body)
        upper = worst_case_nano(leg, rates, bound)
        self.budget.require_room(upper)
        print(
            f"{leg.name}: worst-case {fmt_usd(upper)} "
            f"(input<={bound} tok, output<={leg.max_tokens} tok)",
            flush=True,
        )
        before = self.client.quota_with_retry()
        off_peak = not is_peak_sgt(int(time.time()))
        try:
            if leg.stream:
                events, first_ts, last_ts = self.client.generate_stream(body)
                usage = merge_stream_usage(events)
                served = served_model_from_events(events)
                evidence: dict[str, Any] | None = stream_evidence(events, first_ts, last_ts)
                observed_keys = usage_keys_from_events(events)
            else:
                payload = self.client.generate(body)
                raw_usage = payload.get("usage")
                usage = usage_from_value(raw_usage) if isinstance(raw_usage, dict) else None
                observed_keys = (
                    sorted(str(key) for key in raw_usage) if isinstance(raw_usage, dict) else []
                )
                model = payload.get("model")
                served = model if isinstance(model, str) and model else None
                evidence = None
            if usage is None:
                raise CalibrationError("response carries no authoritative usage object")
            validate_leg_usage(usage)
            if not served:
                raise CalibrationError("response carries no served model id")
            if served not in RATE_CARD:
                raise CalibrationError(f"served model is outside the reviewed rate card: {served}")
            served_rates = RATE_CARD[served]
            actual = api_cost_nano(usage, served_rates)
            if actual > upper:
                raise CalibrationError("actual cost exceeds the pre-request upper bound")
            micro = credits_micro_expected(usage, served_rates, off_peak)
        except HttpCalibrationError:
            # A typed non-2xx rejection: the provider did not serve the request, so there is
            # nothing to hold.
            raise
        except (TransportFailureError, CalibrationError) as error:
            raise PaidLegError(str(error), upper) from error
        self.budget.charge(actual)
        expected_whole = whole_credits(micro)
        try:
            time.sleep(self.poll_delay)
            first = self.client.quota_with_retry()
            time.sleep(self.poll_delay)
            second = self.client.quota_with_retry()
        except TransportFailureError as error:
            raise PostSpendPollError(
                f"paid leg succeeded (charged {fmt_usd(actual)}) but the attribution poll "
                f"failed: {error}"
            ) from error
        settled = quota_counters(first) == quota_counters(second)
        if settled:
            attribution, deltas, reason = attribute_quota_delta(
                before, first, expected_whole, served
            )
        else:
            attribution, deltas, reason = (
                "unattributed",
                {},
                "quota counters kept moving during the observation window",
            )
        record = {
            "leg": leg.name,
            "model": leg.model,
            "served_model": served,
            "stream": leg.stream,
            "max_tokens": leg.max_tokens,
            "preflight": {"input_token_bound": bound, "worst_case_nanousd": str(upper)},
            "usage": usage,
            "usage_observed_keys": observed_keys,
            "api_nanousd": str(actual),
            "credits_micro_expected": str(micro),
            "credits_whole_expected": expected_whole,
            "off_peak": off_peak,
            "quota_before": before,
            "quota_after": first,
            "quota_settled": settled,
            "quota_deltas": deltas,
            "attribution": attribution,
            "attribution_reason": reason,
            "stream_evidence": evidence,
        }
        print(
            f"{leg.name}: served={served} api={fmt_usd(actual)} attribution={attribution}",
            flush=True,
        )
        return record


def resolve_unknowns(
    records: list[dict[str, Any]], wall_evidence: dict[str, Any] | None
) -> dict[str, Any]:
    observed_keys = {
        record["served_model"]: record["usage_observed_keys"]
        for record in records
        if record["usage_observed_keys"]
    }
    if observed_keys:
        usage_form = {
            "status": "resolved",
            "detail": "observed authoritative usage field names per served model",
            "observed_usage_keys": observed_keys,
        }
    else:
        usage_form = {
            "status": "unresolved",
            "detail": "no successful leg returned a usage object",
            "observed_usage_keys": {},
        }
    stream_records = [
        record for record in records if record["stream"] and record["stream_evidence"]
    ]
    text_deltas = sum(
        record["stream_evidence"]["text_delta_frames"] for record in stream_records
    )
    if any(record["stream_evidence"]["incremental_evidence"] for record in stream_records):
        sse = {
            "status": "resolved",
            "detail": (
                f"incremental SSE observed: {text_deltas} text delta frames across "
                f"{len(stream_records)} stream leg(s)"
            ),
        }
    elif stream_records:
        sse = {
            "status": "unresolved",
            "detail": (
                f"stream legs completed but were too short to prove incrementality "
                f"({text_deltas} text delta frames)"
            ),
        }
    else:
        sse = {"status": "unresolved", "detail": "no stream leg completed"}
    attributed = [record for record in records if record["attribution"] == "attributed"]
    below = sum(1 for record in records if record["attribution"] == "below-resolution")
    if attributed:
        units = {
            "status": "resolved",
            "detail": (
                f"{len(attributed)} leg(s) moved the quota window counters by exactly the "
                "official credits formula; units are consistent with credits"
            ),
        }
    else:
        units = {
            "status": "unresolved",
            "detail": (
                f"no leg produced an exact credit-sized quota movement ({below} leg(s) below "
                "provider resolution); raise --max-tokens with --i-understand for a larger "
                "observable delta"
            ),
        }
    if wall_evidence:
        wall = {
            "status": "resolved",
            "detail": (
                f"quota wall observed on {wall_evidence['leg']}: business code "
                f"{wall_evidence['business_code']} ({wall_evidence['window']})"
            ),
        }
    else:
        wall = {
            "status": "unresolved",
            "detail": "quota wall was not reached during the run and is never forced",
        }
    return {
        "usage_form": usage_form,
        "sse_incrementality": sse,
        "quota_units": units,
        "quota_wall_codes": wall,
    }


def fresh_state() -> dict[str, Any]:
    return {
        "spent_nano": 0,
        "held_nano": 0,
        "records": [],
        "unavailable": [],
        "unattributed": [],
        "leg_status": {},
        "quota_anchor": None,
        "wall_evidence": None,
    }


def checkpoint_payload(
    run_id: str,
    profile: str,
    base_url: str,
    budget_nano: int,
    models: list[str],
    max_tokens: int,
    state: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": CHECKPOINT_SCHEMA,
        "run_id": run_id,
        "profile": profile,
        "base_url": base_url,
        "budget_nanousd": str(budget_nano),
        "models": list(models),
        "max_tokens": max_tokens,
        "spent_nano": state["spent_nano"],
        "held_nano": state["held_nano"],
        "records": state["records"],
        "unavailable": state["unavailable"],
        "unattributed": state["unattributed"],
        "leg_status": state["leg_status"],
        "quota_anchor": state["quota_anchor"],
        "wall_evidence": state["wall_evidence"],
    }


CHECKPOINT_REQUIRED_KEYS = frozenset(checkpoint_payload("", "", "", 0, [], 0, fresh_state()))


def save_checkpoint(path: Path, payload: dict[str, Any]) -> None:
    tmp = Path(str(path) + ".tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    os.replace(tmp, path)


def load_resume(
    path: str,
    profile: str,
    base_url: str,
    budget_nano: int,
    models: list[str],
    max_tokens: int,
) -> tuple[str, dict[str, Any]]:
    try:
        payload = json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise CalibrationError(f"cannot read the resume checkpoint: {error}") from error
    if not isinstance(payload, dict) or payload.get("schema") != CHECKPOINT_SCHEMA:
        raise CalibrationError("resume checkpoint has an unrecognized schema")
    missing = sorted(CHECKPOINT_REQUIRED_KEYS - set(payload))
    if missing:
        raise CalibrationError(f"resume checkpoint is incomplete, missing: {', '.join(missing)}")
    mismatches = []
    if payload["profile"] != profile:
        mismatches.append("profile")
    if payload["base_url"] != base_url:
        mismatches.append("base_url")
    if as_int(payload["budget_nanousd"], "checkpoint budget") != budget_nano:
        mismatches.append("budget")
    if list(payload["models"]) != list(models):
        mismatches.append("models")
    if as_int(payload["max_tokens"], "checkpoint max_tokens") != max_tokens:
        mismatches.append("max_tokens")
    if mismatches:
        raise CalibrationError(
            "resume identity mismatch with the checkpoint: " + ", ".join(mismatches)
        )
    state = {
        "spent_nano": as_int(payload["spent_nano"], "checkpoint spent_nano"),
        "held_nano": as_int(payload["held_nano"], "checkpoint held_nano"),
        "records": payload["records"],
        "unavailable": payload["unavailable"],
        "unattributed": payload["unattributed"],
        "leg_status": payload["leg_status"],
        "quota_anchor": payload["quota_anchor"],
        "wall_evidence": payload["wall_evidence"],
    }
    return str(payload["run_id"]), state


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="required to send paid traffic")
    parser.add_argument("--profile", required=True, help="opaque operator label of the target subscription")
    parser.add_argument(
        "--base-url",
        required=True,
        help="exactly https://api.z.ai (international) or https://open.bigmodel.cn (CN)",
    )
    parser.add_argument("--api-key-env", default=KEY_ENV)
    parser.add_argument("--budget-usd", default=DEFAULT_BUDGET_USD)
    parser.add_argument(
        "--i-understand",
        action="store_true",
        help="acknowledge raising the hard budget cap from $0.05 to $5",
    )
    parser.add_argument("--models", nargs="*", choices=MODEL_ORDER)
    parser.add_argument("--max-tokens", type=int, default=32)
    parser.add_argument("--run-id")
    parser.add_argument("--resume", help="path to a checkpoint of an earlier incomplete run")
    parser.add_argument("--report", default="/tmp/glm-calibration-report.json")
    parser.add_argument("--checkpoint", help="default: <report>.checkpoint.json")
    parser.add_argument(
        "--quota-poll-delay", type=float, default=DEFAULT_QUOTA_POLL_DELAY_SECONDS
    )
    parser.add_argument("--http-timeout", type=int, default=DEFAULT_HTTP_TIMEOUT_SECONDS)
    return parser.parse_args(argv)


def plan_json(
    args: argparse.Namespace,
    run_id: str,
    profile: str,
    base_url: str,
    budget_nano: int,
    cap_nano: int,
    legs: list[Leg],
    api_key: str,
) -> dict[str, Any]:
    leg_plans = []
    for leg in legs:
        bound = input_token_bound(body_for_leg(leg, run_id))
        leg_plans.append(
            {
                "name": leg.name,
                "model": leg.model,
                "stream": leg.stream,
                "max_tokens": leg.max_tokens,
                "input_token_bound": bound,
                "worst_case_nanousd": str(worst_case_nano(leg, RATE_CARD[leg.model], bound)),
            }
        )
    total = sum(int(plan["worst_case_nanousd"]) for plan in leg_plans)
    plan: dict[str, Any] = {
        "schema": PLAN_SCHEMA,
        "dry_run": True,
        "run_id_preview": run_id,
        "target": {"profile": profile, "base_url": base_url},
        "key_present": bool(api_key),
        "live_possible": bool(api_key),
        "budget_nanousd": str(budget_nano),
        "budget_cap_nanousd": str(cap_nano),
        "acknowledged_cap": bool(args.i_understand),
        "legs": leg_plans,
        "total_worst_case_nanousd": str(total),
        "quota_anchor": None,
        "notes": [],
    }
    if total > budget_nano:
        plan["notes"].append(
            "the total worst case exceeds the budget: the per-leg guard will stop the matrix "
            "partway; raise --budget-usd or shrink the matrix"
        )
    if not api_key:
        plan["notes"].append(
            f"{args.api_key_env} is not set: no live leg can run — neither the free quota "
            "anchor nor the paid matrix; dry-run only prints this plan"
        )
        return plan
    client = GlmClient(base_url, api_key, args.http_timeout)
    try:
        plan["quota_anchor"] = client.quota_with_retry()
        plan["notes"].append(
            "dry-run: only the free read-only quota anchor was fetched; no paid traffic was "
            "sent. Re-run with --execute for the paid matrix"
        )
    except CalibrationError as error:
        plan["live_possible"] = False
        plan["quota_anchor_error"] = str(error)
        plan["notes"].append(
            "the free quota anchor failed; fix this before running --execute"
        )
    return plan


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    base_url = normalize_base_url(args.base_url)
    profile = validate_profile(args.profile)
    budget_nano = usd_to_nano(args.budget_usd)
    cap_nano = ACKNOWLEDGED_MAX_BUDGET_NANO if args.i_understand else MAX_BUDGET_NANO
    if budget_nano <= 0 or budget_nano > cap_nano:
        hint = "" if args.i_understand else " (pass --i-understand to authorize up to $5)"
        raise CalibrationError(
            f"--budget-usd must be positive and no greater than {fmt_usd(cap_nano)}{hint}"
        )
    if not 1 <= args.max_tokens <= MAX_MAX_TOKENS:
        raise CalibrationError(f"--max-tokens must be within 1..{MAX_MAX_TOKENS}")
    models = list(args.models) if args.models else list(MODEL_ORDER)
    legs = build_legs(models, args.max_tokens)
    api_key = os.getenv(args.api_key_env, "")
    if args.run_id and not RUN_ID_RE.match(args.run_id):
        raise CalibrationError("--run-id has unsafe characters")

    if not args.execute:
        plan = plan_json(
            args,
            args.run_id or new_run_id(),
            profile,
            base_url,
            budget_nano,
            cap_nano,
            legs,
            api_key,
        )
        if not api_key:
            print(
                f"dry-run: {args.api_key_env} is not set — live legs are impossible; "
                "printing the plan only",
                file=sys.stderr,
            )
        print(json.dumps(plan, ensure_ascii=False, indent=2))
        return 0

    if not api_key:
        raise CalibrationError(f"missing API key environment variable: {args.api_key_env}")

    checkpoint_path = Path(args.checkpoint or (args.report + ".checkpoint.json"))
    if args.resume:
        run_id, state = load_resume(
            args.resume, profile, base_url, budget_nano, models, args.max_tokens
        )
        if args.run_id and args.run_id != run_id:
            raise CalibrationError("--run-id does not match the resumed checkpoint")
    else:
        run_id = args.run_id or new_run_id()
        if checkpoint_path.exists():
            raise CalibrationError(
                f"refusing to overwrite the checkpoint of another run: {checkpoint_path}; "
                f"pass --resume {checkpoint_path} to continue it or choose another --report"
            )
        state = fresh_state()

    client = GlmClient(base_url, api_key, args.http_timeout)
    try:
        state["quota_anchor"] = client.quota_with_retry()
    except CalibrationError as error:
        raise CalibrationError(
            f"free quota anchor failed; paid traffic was not started: {error}"
        ) from error
    budget = Budget(
        budget_nano, spent_nano=state["spent_nano"], held_nano=state["held_nano"]
    )
    runner = Runner(client, budget, run_id, args.quota_poll_delay)

    def save() -> None:
        save_checkpoint(
            checkpoint_path,
            checkpoint_payload(
                run_id, profile, base_url, budget_nano, models, args.max_tokens, state
            ),
        )

    failure: str | None = None
    save()
    for leg in legs:
        if state["leg_status"].get(leg.name) in TERMINAL_LEG_STATUSES:
            continue
        try:
            record = runner.execute_leg(leg)
        except PaidLegError as error:
            budget.hold(error.upper_bound_nano)
            state["leg_status"][leg.name] = "held-ambiguous"
            failure = (
                f"{leg.name}: {error}; held the worst-case bound and the leg will not be "
                "re-sent, even on resume"
            )
        except PostSpendPollError as error:
            state["leg_status"][leg.name] = "failed"
            failure = f"{leg.name}: {error}"
        except HttpCalibrationError as error:
            if error.business_code == MODEL_NOT_IN_PLAN_CODE:
                state["unavailable"].append(
                    {
                        "model": leg.model,
                        "capability": "model-in-plan",
                        "http_status": error.status,
                        "business_code": error.business_code,
                        "reason": error.detail[:300],
                    }
                )
                for other in legs:
                    if other.model != leg.model:
                        continue
                    if state["leg_status"].get(other.name) not in TERMINAL_LEG_STATUSES:
                        state["leg_status"][other.name] = "unavailable"
                state["leg_status"][leg.name] = "unavailable"
                print(f"{leg.name}: model is not in the plan (1311); skipping it", flush=True)
            elif error.status == 401:
                state["leg_status"][leg.name] = "failed"
                failure = f"provider rejected the key during {leg.name} (HTTP 401)"
            elif error.status == 429 and error.business_code in QUOTA_WALL_CODES:
                window = QUOTA_WALL_CODES[str(error.business_code)]
                state["wall_evidence"] = {
                    "leg": leg.name,
                    "business_code": str(error.business_code),
                    "window": window,
                    "detail": error.detail[:300],
                }
                state["leg_status"][leg.name] = "failed"
                failure = (
                    f"provider quota wall ({error.business_code}, {window}) on {leg.name}"
                )
            else:
                state["leg_status"][leg.name] = "failed"
                failure = f"{leg.name}: {error}"
        except TransportFailureError as error:
            failure = f"quota endpoint unavailable before {leg.name}: {error}"
        except CalibrationError as error:
            state["leg_status"][leg.name] = "failed"
            failure = f"{leg.name}: {error}"
        else:
            state["records"].append(record)
            state["leg_status"][leg.name] = "ok"
            if record["attribution"] == "unattributed":
                state["unattributed"].append(
                    {
                        "leg": leg.name,
                        "reason": record["attribution_reason"],
                        "deltas": record["quota_deltas"],
                    }
                )
                failure = (
                    f"unattributed quota movement on {leg.name}: "
                    f"{record['attribution_reason']}; stopped fail closed"
                )
        state["spent_nano"] = budget.spent_nano
        state["held_nano"] = budget.held_nano
        save()
        if failure:
            break

    coverage = {
        model: {
            "non_stream": state["leg_status"].get(f"messages:{model}", "not-run"),
            "stream": state["leg_status"].get(f"messages-stream:{model}", "not-run"),
        }
        for model in models
    }
    complete = failure is None and all(
        state["leg_status"].get(leg.name) in {"ok", "unavailable"} for leg in legs
    )
    report = {
        "schema": REPORT_SCHEMA,
        "run_id": run_id,
        "complete": complete,
        "failure": failure,
        "target": {"profile": profile, "base_url": base_url},
        "budget_nanousd": str(budget_nano),
        "spent_nanousd": str(budget.spent_nano),
        "held_nanousd": str(budget.held_nano),
        "models": models,
        "legs": state["records"],
        "leg_status": state["leg_status"],
        "coverage": coverage,
        "unavailable_capabilities": state["unavailable"],
        "unattributed_deltas": state["unattributed"],
        "quota_anchor": state["quota_anchor"],
        "quota_wall_evidence": state["wall_evidence"],
        "unknowns": resolve_unknowns(state["records"], state["wall_evidence"]),
    }
    report_path = Path(args.report)
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"report: {report_path}")
    if failure:
        raise CalibrationError(f"{failure}; partial report: {report_path}")
    if not complete:
        raise CalibrationError(f"coverage incomplete; report: {report_path}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CalibrationError as error:
        print(f"calibration stopped safely: {error}", file=sys.stderr)
        sys.exit(1)
