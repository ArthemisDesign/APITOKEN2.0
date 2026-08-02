#!/usr/bin/env python3
"""Bounded live calibration runner for the pooled Claude backend.

The runner exercises real `/v1/messages` traffic, but treats backend `/capacity` evidence as the
only spend authority. It never estimates the consumed budget from the customer balance. Every
mutating request is preceded by a free `/v1/messages/count_tokens` call and an integer-nanoUSD
worst-case guard; after the response, the exact usage must appear in one unambiguous backend
aggregate before another request is allowed.

No request is sent without `--execute`. API/control credentials are read from the environment and
are never included in the report.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import shlex
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable


NANO_PER_USD = 1_000_000_000
DEFAULT_BUDGET_NANO = 40 * NANO_PER_USD
DEFAULT_TIMEOUT_SECONDS = 180
DEFAULT_PROFILE_DELAY_SECONDS = 16.0
MIN_CACHE_WORDS = 2_048
ANTHROPIC_BETAS = (
    "web-search-2025-03-05,extended-cache-ttl-2025-04-11,fast-mode-2026-02-01"
)

TOKEN_FIELDS = (
    "input_tokens",
    "cache_read_tokens",
    "cache_write_5m_tokens",
    "cache_write_1h_tokens",
    "output_tokens",
    "search_queries",
)
MONEY_FIELDS = (
    "api_input_nanousd",
    "api_cache_read_nanousd",
    "api_cache_write_5m_nanousd",
    "api_cache_write_1h_nanousd",
    "api_output_nanousd",
    "api_search_nanousd",
    "api_total_nanousd",
)
ROW_ID_FIELDS = (
    "email",
    "model",
    "service_tier",
    "inference_geo",
    "tariff_schedule_id",
)


class CalibrationError(RuntimeError):
    """A fail-closed calibration invariant was not satisfied."""


class HttpCalibrationError(CalibrationError):
    """A typed provider response that is safe to classify without parsing log text."""

    def __init__(self, path: str, status: int, detail: str) -> None:
        super().__init__(f"{path} returned HTTP {status}: {detail}")
        self.path = path
        self.status = status
        self.detail = detail


@dataclasses.dataclass(frozen=True)
class TokenRates:
    input_nano: int
    cache_read_nano: int
    cache_write_5m_nano: int
    cache_write_1h_nano: int
    output_nano: int
    web_search_nano: int

    def max_input_nano(self, cache_ttl: str | None) -> int:
        if cache_ttl == "1h":
            return self.cache_write_1h_nano
        if cache_ttl == "5m":
            return self.cache_write_5m_nano
        return self.input_nano


@dataclasses.dataclass(frozen=True)
class Leg:
    name: str
    model: str
    tier: str
    kind: str
    cache_ttl: str | None = None
    cache_id: str | None = None
    cache_phase: str | None = None
    prompt_words: int = 64
    max_tokens: int = 32


@dataclasses.dataclass
class ProfileBudget:
    limit_nano: int
    spent_nano: dict[str, int]

    @classmethod
    def for_profiles(cls, profiles: Iterable[str], limit_nano: int) -> "ProfileBudget":
        return cls(limit_nano=limit_nano, spent_nano={profile: 0 for profile in profiles})

    def require_room_for_any_routing(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        blocked = [
            profile
            for profile, spent in self.spent_nano.items()
            if spent + upper_bound_nano > self.limit_nano
        ]
        if blocked:
            raise CalibrationError(
                "budget guard stopped before dispatch; insufficient room on: "
                + ", ".join(sorted(blocked))
            )

    def require_room_for_profile(self, profile: str, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if profile not in self.spent_nano:
            raise CalibrationError(f"unknown exact-routing profile: {profile}")
        if self.spent_nano[profile] + upper_bound_nano > self.limit_nano:
            raise CalibrationError(
                f"budget guard stopped before dispatch; insufficient room on: {profile}"
            )

    def charge(self, profile: str, actual_nano: int) -> None:
        if profile not in self.spent_nano:
            raise CalibrationError(f"turn was attributed to an unexpected profile: {profile}")
        if actual_nano <= 0:
            raise CalibrationError("backend evidence reported a non-positive turn cost")
        total = self.spent_nano[profile] + actual_nano
        if total > self.limit_nano:
            raise CalibrationError(f"backend evidence exceeded budget for {profile}")
        self.spent_nano[profile] = total


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


def require_healthy_delivery(payload: dict[str, Any], require_empty: bool) -> None:
    delivery = payload.get("calibration_delivery")
    if not isinstance(delivery, dict):
        raise CalibrationError("capacity response has no calibration_delivery authority")
    pending = as_int(delivery.get("pending_events"), "calibration_delivery.pending_events")
    dropped = as_int(delivery.get("dropped_events"), "calibration_delivery.dropped_events")
    if dropped:
        raise CalibrationError(f"calibration delivery dropped {dropped} events")
    if delivery.get("persistence_ok") is not True:
        raise CalibrationError("calibration persistence is degraded")
    if require_empty and pending:
        raise CalibrationError(f"calibration delivery still has {pending} pending events")


def row_key(row: dict[str, Any]) -> tuple[str, ...]:
    return tuple(str(row.get(field, "")) for field in ROW_ID_FIELDS)


def evidence_rows(payload: dict[str, Any]) -> dict[tuple[str, ...], dict[str, int]]:
    rows: dict[tuple[str, ...], dict[str, int]] = {}
    for raw in payload.get("calibration_evidence", []):
        if not isinstance(raw, dict):
            raise CalibrationError("calibration_evidence contains a non-object")
        key = row_key(raw)
        if not all(key):
            raise CalibrationError(f"calibration evidence has an incomplete identity: {key!r}")
        parsed = {
            field: as_int(raw.get(field, 0), f"calibration_evidence.{field}")
            for field in TOKEN_FIELDS + MONEY_FIELDS
        }
        rows[key] = parsed
    return rows


def row_deltas(
    before: dict[tuple[str, ...], dict[str, int]],
    after: dict[tuple[str, ...], dict[str, int]],
) -> dict[tuple[str, ...], dict[str, int]]:
    deltas: dict[tuple[str, ...], dict[str, int]] = {}
    for key in set(before) | set(after):
        old = before.get(key, {})
        new = after.get(key, {})
        delta = {
            field: new.get(field, 0) - old.get(field, 0)
            for field in TOKEN_FIELDS + MONEY_FIELDS
        }
        if any(value < 0 for value in delta.values()):
            raise CalibrationError(f"calibration aggregate moved backwards: {key!r}")
        if any(delta.values()):
            deltas[key] = delta
    return deltas


def usage_from_response(payload: dict[str, Any]) -> dict[str, int]:
    usage = payload.get("usage")
    if not isinstance(usage, dict):
        raise CalibrationError("successful response has no authoritative usage object")
    cache = usage.get("cache_creation")
    cache = cache if isinstance(cache, dict) else {}
    cache_5m = as_int(cache.get("ephemeral_5m_input_tokens", 0), "usage.cache_creation.5m")
    cache_1h = as_int(cache.get("ephemeral_1h_input_tokens", 0), "usage.cache_creation.1h")
    cache_total = as_int(
        usage.get("cache_creation_input_tokens", 0), "usage.cache_creation_input_tokens"
    )
    if cache_5m + cache_1h == 0 and cache_total > 0:
        cache_5m = cache_total
    server_tools = usage.get("server_tool_use")
    server_tools = server_tools if isinstance(server_tools, dict) else {}
    parsed = {
        "input_tokens": as_int(usage.get("input_tokens", 0), "usage.input_tokens"),
        "cache_read_tokens": as_int(
            usage.get("cache_read_input_tokens", 0), "usage.cache_read_input_tokens"
        ),
        "cache_write_5m_tokens": cache_5m,
        "cache_write_1h_tokens": cache_1h,
        "output_tokens": as_int(usage.get("output_tokens", 0), "usage.output_tokens"),
        "search_queries": as_int(
            server_tools.get("web_search_requests", 0),
            "usage.server_tool_use.web_search_requests",
        ),
    }
    if not any(parsed.values()):
        raise CalibrationError("authoritative usage is empty")
    return parsed


def response_service_tier(payload: dict[str, Any]) -> str:
    usage = payload.get("usage")
    speed = usage.get("speed") if isinstance(usage, dict) else None
    return "fast" if isinstance(speed, str) and speed.lower() == "fast" else "standard"


def attribute_exact_turn(
    before: dict[tuple[str, ...], dict[str, int]],
    after: dict[tuple[str, ...], dict[str, int]],
    usage: dict[str, int],
    served_model: str,
    tier: str,
) -> tuple[str, dict[str, int], tuple[str, ...]] | None:
    matches = []
    for key, delta in row_deltas(before, after).items():
        email, model, service_tier, _, _ = key
        if model != served_model or service_tier != tier:
            continue
        if all(delta[field] == usage[field] for field in TOKEN_FIELDS):
            matches.append((email, delta, key))
    if len(matches) > 1:
        raise CalibrationError("turn evidence is ambiguous across multiple profiles")
    return matches[0] if matches else None


def profile_state(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    states: dict[str, dict[str, Any]] = {}
    for raw in payload.get("per_sub", []):
        if not isinstance(raw, dict):
            continue
        email = str(raw.get("email", ""))
        if not email:
            continue
        windows = {
            str(window.get("window_kind")): window
            for window in raw.get("windows", [])
            if isinstance(window, dict)
        }
        states[email] = {
            "routable": bool(raw.get("routable")),
            "dead": bool(raw.get("dead")),
            "cooling": bool(raw.get("cooling")),
            "plan": str(raw.get("plan", "")),
            "used_5h": windows.get("5h", {}).get("used_fraction_units"),
            "used_7d": windows.get("7d", {}).get("used_fraction_units"),
        }
    return states


def fraction_delta(before: dict[str, Any], after: dict[str, Any], field: str) -> int | None:
    left, right = before.get(field), after.get(field)
    if left is None or right is None:
        return None
    delta = int(right) - int(left)
    return delta if delta >= 0 else None


def canonical_rate_id(model: str, available: set[str]) -> str | None:
    if model in available:
        return model
    aliases = (
        ("claude-opus-5", "claude-opus-5"),
        ("claude-opus-5", "claude-opus-4-8"),
        ("claude-fable-5", "claude-fable-5"),
        ("claude-opus-4-8", "claude-opus-4-8"),
        ("claude-opus-4-7", "claude-opus-4-7"),
        ("claude-opus-4-6", "claude-opus-4-8"),
        ("claude-opus-4-5", "claude-opus-4-8"),
        ("claude-sonnet-5", "claude-sonnet-5"),
        ("claude-sonnet-4-6", "claude-sonnet-4-6"),
        ("claude-sonnet-4-5", "claude-sonnet-4-6"),
        ("claude-haiku-4-5", "claude-haiku-4-5"),
    )
    for prefix, canonical in aliases:
        if model.startswith(prefix) and canonical in available:
            return canonical
    return None


def rate_catalog(payload: dict[str, Any]) -> tuple[dict[tuple[str, str], TokenRates], TokenRates]:
    catalog: dict[tuple[str, str], TokenRates] = {}
    all_rates: list[TokenRates] = []
    for model in payload.get("conversion_models", []):
        if not isinstance(model, dict):
            continue
        model_id = str(model.get("id", ""))
        web = as_int(model.get("web_search_nanousd_per_request", 0), "web search rate")
        for tier in model.get("tiers", []):
            if not isinstance(tier, dict):
                continue
            tier_id = str(tier.get("id", ""))
            rates = TokenRates(
                input_nano=as_int(tier.get("input_nanousd_per_token"), "input rate"),
                cache_read_nano=as_int(
                    tier.get("cache_read_nanousd_per_token"), "cache read rate"
                ),
                cache_write_5m_nano=as_int(
                    tier.get("cache_write_5m_nanousd_per_token"), "cache write 5m rate"
                ),
                cache_write_1h_nano=as_int(
                    tier.get("cache_write_1h_nanousd_per_token"), "cache write 1h rate"
                ),
                output_nano=as_int(tier.get("output_nanousd_per_token"), "output rate"),
                web_search_nano=web,
            )
            if model_id and tier_id:
                catalog[(model_id, tier_id)] = rates
                all_rates.append(rates)
    if not all_rates:
        raise CalibrationError("capacity response has no Claude conversion rates")
    ceiling = TokenRates(
        input_nano=max(rate.input_nano for rate in all_rates),
        cache_read_nano=max(rate.cache_read_nano for rate in all_rates),
        cache_write_5m_nano=max(rate.cache_write_5m_nano for rate in all_rates),
        cache_write_1h_nano=max(rate.cache_write_1h_nano for rate in all_rates),
        output_nano=max(rate.output_nano for rate in all_rates),
        web_search_nano=max(rate.web_search_nano for rate in all_rates),
    )
    return catalog, ceiling


def rates_for_model(
    catalog: dict[tuple[str, str], TokenRates], ceiling: TokenRates, model: str, tier: str
) -> TokenRates:
    canonical = canonical_rate_id(model, {key[0] for key in catalog})
    return catalog.get((canonical or "", tier), ceiling)


def request_upper_bound_nano(
    input_tokens: int, max_tokens: int, web_uses: int, rates: TokenRates, cache_ttl: str | None
) -> int:
    if input_tokens <= 0 or max_tokens < 0 or web_uses < 0:
        raise CalibrationError("invalid count_tokens result or request limits")
    return (
        input_tokens * rates.max_input_nano(cache_ttl)
        + max_tokens * rates.output_nano
        + web_uses * rates.web_search_nano
    )


def filler(words: int, salt: str) -> str:
    if words <= 0:
        return salt
    return f"{salt}\n" + (" calibration" * words).strip()


def body_for_leg(leg: Leg, run_id: str) -> dict[str, Any]:
    body: dict[str, Any] = {
        "model": leg.model,
        "max_tokens": leg.max_tokens,
        "messages": [
            {
                "role": "user",
                "content": (
                    f"Calibration run {run_id}, leg {leg.name}. "
                    "Reply with exactly CALIBRATION_OK. "
                    + (filler(leg.prompt_words, leg.name) if leg.kind == "fresh" else "")
                ),
            }
        ],
    }
    if leg.tier == "fast":
        body["speed"] = "fast"
    if leg.kind == "cache":
        if leg.cache_ttl not in {"5m", "1h"} or not leg.cache_id:
            raise CalibrationError(f"invalid cache leg: {leg}")
        cached = filler(
            max(leg.prompt_words, MIN_CACHE_WORDS), f"{run_id}:{leg.cache_id}"
        )
        body["system"] = [
            {
                "type": "text",
                "text": cached,
                "cache_control": {"type": "ephemeral", "ttl": leg.cache_ttl},
            }
        ]
        body["messages"][0]["content"] = (
            f"Calibration cache {leg.cache_id} phase {leg.cache_phase}. Reply CALIBRATION_OK."
        )
    elif leg.kind == "web":
        body["tools"] = [
            {"type": "web_search_20250305", "name": "web_search", "max_uses": 1}
        ]
        body["messages"][0]["content"] = (
            "Use web search exactly once to find the current UTC date, then answer with the date."
        )
    return body


def count_body(body: dict[str, Any]) -> dict[str, Any]:
    allowed = {"model", "messages", "system", "tools", "tool_choice", "thinking"}
    counted = {key: value for key, value in body.items() if key in allowed}
    tools = counted.get("tools")
    if isinstance(tools, list):
        local_tools = [
            tool
            for tool in tools
            if not (
                isinstance(tool, dict)
                and "web_search" in str(tool.get("type", ""))
            )
        ]
        if local_tools:
            counted["tools"] = local_tools
        else:
            counted.pop("tools", None)
            counted.pop("tool_choice", None)
    return counted


def guarded_input_tokens(body: dict[str, Any], counted_input_tokens: int) -> int:
    """Cover server-tool schema overhead that Anthropic refuses to count for us.

    UTF-8 bytes are a conservative token upper bound, so adding the complete compact JSON payload
    cannot under-reserve even if the provider injects the server tool into its effective prompt.
    """

    server_tools = [
        tool
        for tool in body.get("tools", [])
        if isinstance(tool, dict) and "web_search" in str(tool.get("type", ""))
    ]
    if not server_tools:
        return counted_input_tokens
    schema_bytes = len(json.dumps(server_tools, separators=(",", ":")).encode())
    return counted_input_tokens + schema_bytes


def supported_tiers(
    model: str, catalog: dict[tuple[str, str], TokenRates]
) -> tuple[str, ...]:
    canonical = canonical_rate_id(model, {key[0] for key in catalog})
    if canonical is None or (canonical, "standard") not in catalog:
        raise CalibrationError(f"advertised model has no audited conversion rate: {model}")
    fast_model = model == "claude-opus-5" or model.startswith("claude-opus-4-8")
    return (
        ("standard", "fast")
        if fast_model and (canonical, "fast") in catalog
        else ("standard",)
    )


def build_coverage_legs(
    models: list[str],
    prompt_words: int,
    catalog: dict[tuple[str, str], TokenRates],
) -> list[Leg]:
    legs: list[Leg] = []
    for model in models:
        for tier in supported_tiers(model, catalog):
            legs.append(
                Leg(
                    name=f"fresh:{model}:{tier}",
                    model=model,
                    tier=tier,
                    kind="fresh",
                    prompt_words=max(64, prompt_words // 8),
                    max_tokens=32,
                )
            )
            for ttl in ("5m", "1h"):
                cache_id = f"coverage-{model}-{ttl}-{tier}"
                for phase in ("write", "read"):
                    legs.append(
                        Leg(
                            name=f"cache-{ttl}-{phase}:{model}:{tier}",
                            model=model,
                            tier=tier,
                            kind="cache",
                            cache_ttl=ttl,
                            cache_id=cache_id,
                            cache_phase=phase,
                            prompt_words=max(prompt_words, MIN_CACHE_WORDS),
                            max_tokens=16,
                        )
                    )
            legs.append(
                Leg(
                    name=f"web:{model}:{tier}",
                    model=model,
                    tier=tier,
                    kind="web",
                    prompt_words=0,
                    max_tokens=128,
                )
            )
    return legs


def verify_leg_usage(leg: Leg, usage: dict[str, int]) -> None:
    if usage["output_tokens"] <= 0:
        raise CalibrationError(f"{leg.name}: output token class was not observed")
    if leg.kind == "fresh" and usage["input_tokens"] <= 0:
        raise CalibrationError(f"{leg.name}: fresh input token class was not observed")
    if leg.kind == "cache" and leg.cache_phase == "write":
        field = "cache_write_1h_tokens" if leg.cache_ttl == "1h" else "cache_write_5m_tokens"
        if usage[field] <= 0:
            raise CalibrationError(f"{leg.name}: expected {field}, got {usage}")
    if leg.kind == "cache" and leg.cache_phase == "read":
        if usage["cache_read_tokens"] <= 0:
            raise CalibrationError(f"{leg.name}: cache read was not observed")
    if leg.kind == "web" and usage["search_queries"] <= 0:
        raise CalibrationError(f"{leg.name}: Web Search usage was not observed")


class JsonHttpClient:
    def __init__(self, api_url: str, api_key: str, timeout: int) -> None:
        self.api_url = api_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def request(
        self,
        path: str,
        method: str = "GET",
        body: dict[str, Any] | None = None,
        session_id: str | None = None,
        target_profile: str | None = None,
    ) -> dict[str, Any]:
        data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
        headers = {
            "x-api-key": self.api_key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
            "accept": "application/json",
            "anthropic-beta": ANTHROPIC_BETAS,
        }
        if session_id:
            headers["x-session-id"] = session_id
        if target_profile:
            headers["x-apitoken-calibration-profile"] = target_profile.removesuffix("…")
        request = urllib.request.Request(
            f"{self.api_url}{path}", data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            detail = error.read(800).decode(errors="replace")
            raise HttpCalibrationError(path, error.code, detail) from error
        except urllib.error.URLError as error:
            raise CalibrationError(f"{path} failed: {error}") from error
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload


class ProductionSshJsonHttpClient:
    """Call the stable loopback router with a remote-only forwarding-admin credential.

    The credential is expanded only by the production shell and never crosses SSH or enters the
    report. The admin-only calibration header makes each paid-equivalent turn exact-profile and is
    stripped before the request reaches Anthropic.
    """

    def __init__(self, timeout: int) -> None:
        self.timeout = timeout

    def request(
        self,
        path: str,
        method: str = "GET",
        body: dict[str, Any] | None = None,
        session_id: str | None = None,
        target_profile: str | None = None,
    ) -> dict[str, Any]:
        if method not in {"GET", "POST"} or not path.startswith("/v1/"):
            raise CalibrationError(f"unsupported production SSH request: {method} {path}")
        if any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/_-?=&." for char in path):
            raise CalibrationError(f"unsafe production SSH path: {path!r}")
        headers = [
            "anthropic-version: 2023-06-01",
            "content-type: application/json",
            "accept: application/json",
            f"anthropic-beta: {ANTHROPIC_BETAS}",
        ]
        if session_id:
            try:
                uuid.UUID(session_id)
            except ValueError as error:
                raise CalibrationError("invalid calibration session UUID") from error
            headers.append(f"x-session-id: {session_id}")
        if target_profile:
            hint = target_profile.removesuffix("…")
            if not (1 <= len(hint) <= 4) or not all(
                char.isascii() and (char.isalnum() or char in "._-") for char in hint
            ):
                raise CalibrationError(f"invalid bounded profile hint: {target_profile!r}")
            headers.append(f"x-apitoken-calibration-profile: {hint}")
        header_args = " ".join(f"-H {shlex.quote(header)}" for header in headers)
        data_arg = "--data-binary @-" if body is not None else ""
        remote = (
            "set -a && . /srv/claude-api/data/server.env && set +a && "
            "calibration_key=${CLAUDE_API_KEYS%%,*} && test -n \"$calibration_key\" && "
            f"curl -sS --max-time {self.timeout} -w '\\n%{{http_code}}' "
            f"-X {method} -H \"x-api-key: $calibration_key\" {header_args} {data_arg} "
            f"{shlex.quote('http://127.0.0.1:8790' + path)}"
        )
        data = b"" if body is None else json.dumps(body, separators=(",", ":")).encode()
        try:
            result = subprocess.run(
                ["ssh", "apitokensale", remote],
                input=data,
                capture_output=True,
                timeout=self.timeout + 30,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise CalibrationError(f"{path} timed out over production SSH") from error
        if result.returncode != 0:
            detail = result.stderr.decode(errors="replace")[-800:]
            raise CalibrationError(
                f"{path} production SSH transport failed ({result.returncode}): {detail}"
            )
        raw, separator, status_raw = result.stdout.rpartition(b"\n")
        if not separator or not status_raw.isdigit():
            raise CalibrationError(f"{path} production SSH response has no HTTP status")
        status = int(status_raw)
        if status >= 400:
            detail = raw[:800].decode(errors="replace")
            raise HttpCalibrationError(path, status, detail)
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload


class CapacityReader:
    def __init__(self, command: str | None, url: str | None, panel_key: str | None, timeout: int):
        self.command = shlex.split(command) if command else None
        self.url = url
        self.panel_key = panel_key
        self.timeout = timeout
        if not self.command and not self.url:
            raise CalibrationError("set --capacity-command or --capacity-url")
        if self.url and not self.panel_key:
            raise CalibrationError("CLAUDE_API_PANEL_KEY is required with --capacity-url")

    def read(self) -> dict[str, Any]:
        if self.command:
            result = subprocess.run(
                self.command,
                check=False,
                capture_output=True,
                timeout=self.timeout,
            )
            if result.returncode != 0:
                detail = result.stderr.decode(errors="replace")[-500:]
                raise CalibrationError(f"capacity command failed ({result.returncode}): {detail}")
            raw = result.stdout
        else:
            request = urllib.request.Request(
                str(self.url), headers={"x-api-key": str(self.panel_key)}, method="GET"
            )
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    raw = response.read()
            except urllib.error.URLError as error:
                raise CalibrationError(f"capacity read failed: {error}") from error
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError("capacity authority returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError("capacity authority returned a non-object")
        return payload


class Runner:
    def __init__(
        self,
        api: JsonHttpClient,
        capacity: CapacityReader,
        budget: ProfileBudget,
        catalog: dict[tuple[str, str], TokenRates],
        ceiling: TokenRates,
        evidence_timeout: int,
        profile_delay: float,
        run_id: str,
        exact_profile_routing: bool = False,
    ) -> None:
        self.api = api
        self.capacity = capacity
        self.budget = budget
        self.catalog = catalog
        self.ceiling = ceiling
        self.evidence_timeout = evidence_timeout
        self.profile_delay = profile_delay
        self.run_id = run_id
        self.exact_profile_routing = exact_profile_routing
        self.last_profile_turn: dict[str, float] = defaultdict(float)
        self.records: list[dict[str, Any]] = []

    def wait_profile_delay(self, expected_profile: str | None) -> None:
        if not expected_profile:
            return
        remaining = self.last_profile_turn[expected_profile] + self.profile_delay - time.monotonic()
        if remaining > 0:
            time.sleep(remaining)

    def execute_leg(
        self, leg: Leg, session_id: str, expected_profile: str | None = None
    ) -> tuple[str, dict[str, Any]]:
        self.wait_profile_delay(expected_profile)
        before_payload = self.capacity.read()
        require_healthy_delivery(before_payload, require_empty=True)
        before_states = profile_state(before_payload)
        before_rows = evidence_rows(before_payload)
        if expected_profile:
            state = before_states.get(expected_profile)
            if not state or not state["routable"]:
                raise CalibrationError(f"{expected_profile} became non-routable before {leg.name}")

        body = body_for_leg(leg, self.run_id)
        count = self.api.request(
            "/v1/messages/count_tokens",
            method="POST",
            body=count_body(body),
            session_id=session_id,
            target_profile=expected_profile if self.exact_profile_routing else None,
        )
        input_tokens = as_int(count.get("input_tokens"), "count_tokens.input_tokens")
        guarded_tokens = guarded_input_tokens(body, input_tokens)
        rates = rates_for_model(self.catalog, self.ceiling, leg.model, leg.tier)
        web_uses = sum(
            as_int(tool.get("max_uses", 0), "tool.max_uses")
            for tool in body.get("tools", [])
            if isinstance(tool, dict) and "web_search" in str(tool.get("type", ""))
        )
        upper = request_upper_bound_nano(
            guarded_tokens, leg.max_tokens, web_uses, rates, leg.cache_ttl
        )
        if self.exact_profile_routing:
            if not expected_profile:
                raise CalibrationError("exact-profile routing requires an expected profile")
            self.budget.require_room_for_profile(expected_profile, upper)
        else:
            self.budget.require_room_for_any_routing(upper)

        response = self.api.request(
            "/v1/messages",
            method="POST",
            body=body,
            session_id=session_id,
            target_profile=expected_profile if self.exact_profile_routing else None,
        )
        usage = usage_from_response(response)
        actual_tier = response_service_tier(response)
        served_model = str(response.get("model") or leg.model)
        deadline = time.monotonic() + self.evidence_timeout
        match = None
        after_payload = None
        while time.monotonic() < deadline:
            after_payload = self.capacity.read()
            # The just-finished turn may briefly be pending, but any writer failure or dropped
            # event is terminal immediately. Do not burn the rest of the evidence timeout while
            # the durable FIFO already reports that exact attribution cannot advance.
            require_healthy_delivery(after_payload, require_empty=False)
            match = attribute_exact_turn(
                before_rows, evidence_rows(after_payload), usage, served_model, actual_tier
            )
            if match:
                break
            time.sleep(2.5)
        if not match or after_payload is None:
            raise CalibrationError(
                f"{leg.name}: exact backend evidence did not become attributable within timeout"
            )
        require_healthy_delivery(after_payload, require_empty=True)
        profile, delta, _ = match
        if expected_profile and profile != expected_profile:
            raise CalibrationError(
                f"{leg.name}: affinity rebound from {expected_profile} to {profile}; stopped"
            )
        if delta["api_total_nanousd"] > upper:
            raise CalibrationError(
                f"{leg.name}: actual backend cost exceeds pre-request upper bound"
            )
        self.budget.charge(profile, delta["api_total_nanousd"])
        self.last_profile_turn[profile] = time.monotonic()
        coverage_error = None
        try:
            verify_leg_usage(leg, usage)
            if actual_tier != leg.tier:
                raise CalibrationError(
                    f"{leg.name}: requested {leg.tier}, provider served {actual_tier}"
                )
        except CalibrationError as error:
            coverage_error = str(error)

        after_states = profile_state(after_payload)
        before_state = before_states.get(profile, {})
        after_state = after_states.get(profile, {})
        record = {
            "leg": leg.name,
            "kind": leg.kind,
            "requested_model": leg.model,
            "served_model": served_model,
            "requested_tier": leg.tier,
            "tier": actual_tier,
            "profile": profile,
            "session_hash": hashlib.sha256(session_id.encode()).hexdigest()[:12],
            "counted_input_tokens": input_tokens,
            "guarded_input_tokens": guarded_tokens,
            "upper_bound_nano": str(upper),
            "actual_nano": str(delta["api_total_nanousd"]),
            "profile_test_spend_nano": str(self.budget.spent_nano[profile]),
            "usage": usage,
            "coverage_ok": coverage_error is None,
            "coverage_error": coverage_error,
            "fraction_delta_5h": fraction_delta(before_state, after_state, "used_5h"),
            "fraction_delta_7d": fraction_delta(before_state, after_state, "used_7d"),
        }
        self.records.append(record)
        suffix = "" if coverage_error is None else f"; COVERAGE MISS: {coverage_error}"
        print(
            f"{profile} {leg.name}: ${delta['api_total_nanousd'] / NANO_PER_USD:.6f}; "
            f"total=${self.budget.spent_nano[profile] / NANO_PER_USD:.6f}{suffix}",
            flush=True,
        )
        return profile, record


def model_profitability(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], dict[str, int]] = defaultdict(
        lambda: {"spend": 0, "fraction_5h": 0, "fraction_7d": 0, "turns": 0}
    )
    for record in records:
        key = (record["served_model"], record["tier"])
        row = grouped[key]
        row["spend"] += int(record["actual_nano"])
        row["fraction_5h"] += int(record["fraction_delta_5h"] or 0)
        row["fraction_7d"] += int(record["fraction_delta_7d"] or 0)
        row["turns"] += 1
    output = []
    for (model, tier), row in grouped.items():
        per_percent = (
            row["spend"] * 1_000_000 // row["fraction_5h"]
            if row["fraction_5h"] > 0
            else None
        )
        output.append(
            {
                "model": model,
                "tier": tier,
                "turns": row["turns"],
                "api_spend_nano": str(row["spend"]),
                "observed_fraction_units_5h": row["fraction_5h"],
                "observed_fraction_units_7d": row["fraction_7d"],
                "api_nano_per_1pct_5h": None if per_percent is None else str(per_percent),
            }
        )
    return sorted(
        output,
        key=lambda row: int(row["api_nano_per_1pct_5h"] or -1),
        reverse=True,
    )


def coverage_failure(records: list[dict[str, Any]]) -> str | None:
    misses = [str(record["leg"]) for record in records if not record.get("coverage_ok", False)]
    if not misses:
        return None
    preview = ", ".join(misses[:8])
    suffix = "" if len(misses) <= 8 else f", and {len(misses) - 8} more"
    noun = "leg" if len(misses) == 1 else "legs"
    return f"token-class coverage incomplete for {len(misses)} {noun}: {preview}{suffix}"


def discover_models(api: JsonHttpClient) -> list[str]:
    payload = api.request("/v1/models")
    models = []
    for entry in payload.get("data", []):
        model = entry.get("id") if isinstance(entry, dict) else None
        if isinstance(model, str) and model.startswith("claude-"):
            models.append(model)
    if not models:
        raise CalibrationError("/v1/models returned no Claude models")
    return sorted(set(models))


def usd_to_nano(value: str) -> int:
    whole, dot, fractional = value.strip().partition(".")
    if not whole.isdigit() or (dot and not fractional.isdigit()):
        raise CalibrationError(f"invalid USD amount: {value!r}")
    fraction = (fractional + "000000000")[:9]
    return int(whole) * NANO_PER_USD + int(fraction)


def remote_capacity_command() -> str:
    return (
        "ssh apitokensale "
        "'set -a; . /srv/claude-api/data/server.env; set +a; "
        'curl -fsS -H "x-api-key: $CLAUDE_API_PANEL_KEY" '
        "http://127.0.0.1:8790/capacity'"
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="required to send live requests")
    parser.add_argument("--api-url", default="https://api.apitoken.sale")
    parser.add_argument("--api-key-env", default="APITOKEN_API_KEY")
    parser.add_argument("--capacity-command", default=os.getenv("CLAUDE_CALIBRATION_CAPACITY_COMMAND"))
    parser.add_argument("--capacity-url")
    parser.add_argument("--panel-key-env", default="CLAUDE_API_PANEL_KEY")
    parser.add_argument("--budget-usd", default="40")
    parser.add_argument("--prompt-words", type=int, default=4_096)
    parser.add_argument("--fill-model", default="claude-fable-5")
    parser.add_argument("--fill-tier", choices=("standard", "fast"), default="standard")
    parser.add_argument("--fill-leg-usd", default="2")
    parser.add_argument("--max-fill-turns", type=int, default=80)
    parser.add_argument("--no-fill", action="store_true")
    parser.add_argument("--evidence-timeout", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--profile-delay", type=float, default=DEFAULT_PROFILE_DELAY_SECONDS)
    parser.add_argument("--http-timeout", type=int, default=240)
    parser.add_argument("--report", default="/tmp/claude-calibration-report.json")
    parser.add_argument("--models", nargs="*")
    parser.add_argument(
        "--production-capacity-over-ssh",
        action="store_true",
        help="use the standard secret-safe read-only SSH capacity command",
    )
    parser.add_argument(
        "--production-api-over-ssh",
        action="store_true",
        help="send admin-only exact-profile live turns through the production loopback slot",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if not args.execute:
        print("Refusing live traffic without --execute.", file=sys.stderr)
        return 2
    api_key = os.getenv(args.api_key_env, "")
    if not args.production_api_over_ssh and not api_key:
        raise CalibrationError(f"missing API key environment variable: {args.api_key_env}")
    capacity_command = (
        remote_capacity_command()
        if args.production_capacity_over_ssh
        else args.capacity_command
    )
    capacity = CapacityReader(
        capacity_command,
        args.capacity_url,
        os.getenv(args.panel_key_env),
        args.http_timeout,
    )
    api = (
        ProductionSshJsonHttpClient(args.http_timeout)
        if args.production_api_over_ssh
        else JsonHttpClient(args.api_url, api_key, args.http_timeout)
    )
    baseline = capacity.read()
    states = profile_state(baseline)
    profiles = sorted(
        email for email, state in states.items() if state["routable"] and not state["dead"]
    )
    if not profiles:
        raise CalibrationError("no healthy routable Claude profiles")
    unknown_plan = [
        email for email in profiles if states[email]["plan"] not in {"pro", "max5", "max20"}
    ]
    if unknown_plan:
        raise CalibrationError(
            "routable profiles still have no authoritative paid plan: "
            + ", ".join(unknown_plan)
        )
    require_healthy_delivery(baseline, require_empty=True)

    catalog, ceiling = rate_catalog(baseline)
    models = args.models or discover_models(api)
    budget_nano = usd_to_nano(args.budget_usd)
    if budget_nano <= 0 or budget_nano > DEFAULT_BUDGET_NANO:
        raise CalibrationError("--budget-usd must be positive and no greater than 40")
    run_id = f"claude-cal-{int(time.time())}-{uuid.uuid4().hex[:8]}"
    budget = ProfileBudget.for_profiles(profiles, budget_nano)
    runner = Runner(
        api,
        capacity,
        budget,
        catalog,
        ceiling,
        args.evidence_timeout,
        args.profile_delay,
        run_id,
        exact_profile_routing=args.production_api_over_ssh,
    )

    sessions: dict[str, str] = {}
    unavailable_capabilities: list[dict[str, Any]] = []
    unavailable_pairs: set[tuple[str, str, str]] = set()
    profile_stops: dict[str, str] = {}
    coverage_stops: set[str] = set()
    failure: str | None = None
    try:
        discovery_model = next((model for model in models if "haiku" in model), models[-1])
        if args.production_api_over_ssh:
            for attempt, profile in enumerate(profiles):
                session = str(uuid.uuid4())
                leg = Leg(
                    name=f"discovery:{attempt}",
                    model=discovery_model,
                    tier="standard",
                    kind="fresh",
                    prompt_words=16,
                    max_tokens=8,
                )
                runner.execute_leg(leg, session, expected_profile=profile)
                sessions[profile] = session
        else:
            for attempt in range(max(12, len(profiles) * 6)):
                if len(sessions) == len(profiles):
                    break
                session = str(uuid.uuid4())
                leg = Leg(
                    name=f"discovery:{attempt}",
                    model=discovery_model,
                    tier="standard",
                    kind="fresh",
                    prompt_words=16,
                    max_tokens=8,
                )
                profile, _ = runner.execute_leg(leg, session)
                sessions.setdefault(profile, session)
        missing = sorted(set(profiles) - set(sessions))
        if missing:
            raise CalibrationError(f"could not establish sticky sessions for: {', '.join(missing)}")

        coverage = build_coverage_legs(models, args.prompt_words, catalog)
        for leg in coverage:
            for profile in profiles:
                pair = (profile, leg.model, leg.tier)
                if profile in profile_stops or pair in unavailable_pairs:
                    continue
                try:
                    _, record = runner.execute_leg(
                        leg, sessions[profile], expected_profile=profile
                    )
                    if record["tier"] != leg.tier:
                        unavailable_pairs.add(pair)
                        unavailable_capabilities.append(
                            {
                                "profile": profile,
                                "model": leg.model,
                                "tier": leg.tier,
                                "reason": record["coverage_error"],
                            }
                        )
                except HttpCalibrationError as error:
                    if leg.tier == "fast" and error.status in {400, 403, 429}:
                        unavailable_pairs.add(pair)
                        unavailable_capabilities.append(
                            {
                                "profile": profile,
                                "model": leg.model,
                                "tier": leg.tier,
                                "http_status": error.status,
                                "reason": error.detail[:300],
                            }
                        )
                        print(
                            f"{profile} {leg.model}: fast unavailable (HTTP {error.status})",
                            flush=True,
                        )
                        continue
                    if error.status == 429:
                        profile_stops[profile] = str(error)
                        coverage_stops.add(profile)
                        print(f"{profile}: provider quota wall reached", flush=True)
                        continue
                    raise
                except CalibrationError as error:
                    if "became non-routable" in str(error):
                        profile_stops[profile] = str(error)
                        coverage_stops.add(profile)
                        print(f"{profile}: provider made profile non-routable", flush=True)
                        continue
                    raise

        if not args.no_fill:
            if args.fill_model not in models:
                raise CalibrationError(f"fill model is not advertised: {args.fill_model}")
            target_leg_nano = usd_to_nano(args.fill_leg_usd)
            if args.fill_tier not in supported_tiers(args.fill_model, catalog):
                raise CalibrationError(
                    f"fill tier is not available for {args.fill_model}: {args.fill_tier}"
                )
            fill_rates = rates_for_model(catalog, ceiling, args.fill_model, args.fill_tier)
            words = max(
                MIN_CACHE_WORDS,
                min(120_000, target_leg_nano // max(fill_rates.cache_write_1h_nano, 1)),
            )
            fill_done = set(profile_stops)
            for index in range(args.max_fill_turns):
                made_progress = False
                for profile in profiles:
                    if profile in fill_done:
                        continue
                    leg = Leg(
                        name=f"fill-cache-1h:{index}",
                        model=args.fill_model,
                        tier=args.fill_tier,
                        kind="cache",
                        cache_ttl="1h",
                        cache_id=f"{run_id}-fill-{profile}-{index}",
                        cache_phase="write",
                        prompt_words=words,
                        max_tokens=8,
                    )
                    try:
                        runner.execute_leg(leg, sessions[profile], expected_profile=profile)
                        made_progress = True
                    except CalibrationError as error:
                        if "budget guard stopped before dispatch" in str(error):
                            fill_done.add(profile)
                            continue
                        if isinstance(error, HttpCalibrationError) and error.status == 429:
                            profile_stops[profile] = str(error)
                            fill_done.add(profile)
                            continue
                        if "became non-routable" in str(error):
                            profile_stops[profile] = str(error)
                            fill_done.add(profile)
                            continue
                        raise
                if len(fill_done) == len(profiles) or not made_progress:
                    break
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        failure = str(error)

    # Usage-class misses are not safety faults, so finish the remaining matrix and preserve all
    # useful evidence. They still make the run formally incomplete: a green report must prove every
    # requested input/cache/output/search leg, not merely receive successful HTTP responses.
    failure = failure or coverage_failure(runner.records)
    if coverage_stops:
        failure = failure or (
            "coverage stopped at the provider quota wall for: "
            + ", ".join(sorted(coverage_stops))
        )

    try:
        final = capacity.read()
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        final = baseline
        failure = failure or f"final capacity read failed: {error}"
    report = {
        "schema": "claude-live-calibration/v1",
        "run_id": run_id,
        "complete": failure is None,
        "failure": failure,
        "budget_nano_per_profile": str(budget_nano),
        "models": models,
        "profiles": profiles,
        "unavailable_capabilities": unavailable_capabilities,
        "profile_stops": profile_stops,
        "spent_nano_per_profile": {
            profile: str(spent) for profile, spent in sorted(budget.spent_nano.items())
        },
        "records": runner.records,
        "model_profitability": model_profitability(runner.records),
        "final_capacity": final,
    }
    report_path = Path(args.report)
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"report: {report_path}")
    if failure:
        raise CalibrationError(f"{failure}; partial report: {report_path}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        print(f"calibration stopped safely: {error}", file=sys.stderr)
        sys.exit(1)
