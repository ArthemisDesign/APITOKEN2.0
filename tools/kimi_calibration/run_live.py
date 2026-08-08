#!/usr/bin/env python3
"""Fail-closed live calibration runner for the KIMI subscription provider plane.

The runner spends only on an exact opaque-profile target and treats the admin-only `/kimi-subs`
immutable turn events as the sole API-dollar authority. Quota movement is read from the same
endpoint's per-window observations, keyed by exact `duration_secs`, never by window position.
Dry-run is the default; live traffic requires `--execute` plus explicit human authorization.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from collections import defaultdict
from pathlib import Path
from typing import Any


NANO_PER_USD = 1_000_000_000
# Authorized by the product owner on 2026-08-07: at most $10 of API-equivalent spend for the whole
# calibration effort. Encoded literally, as the contract requires — this is a ceiling on the run,
# not an estimate of it, and the observed spend is reported separately.
MAX_BUDGET_NANO = 10_000_000_000  # $10.00 aggregate run budget; a CLI value above this is an error.
# Separate ceiling for the tool/media probes. It is deliberately its own number: those legs exist
# because the per-request unit cost of a tool call is unproven, so they cannot be bounded the way a
# generation leg is, and letting them share the coverage budget would hide an unbounded spend
# behind a bounded one.
MAX_CAPABILITY_PROBE_USD = "0.0001"
MAX_CAPABILITY_PROBE_NANO = 100_000
DEFAULT_BUDGET_USD = "0.0001"
MIN_RECENT_TURN_LIMIT = 512
SAFE_READ_ATTEMPTS = 3
SAFE_READ_RETRY_DELAY_SECONDS = 2.0
DEFAULT_EVIDENCE_TIMEOUT_SECONDS = 180
DEFAULT_PROFILE_DELAY_SECONDS = 16.0
DEFAULT_MAX_OUTPUT_TOKENS = 256
DEFAULT_PRODUCTION_SSH_TARGET = "apitokensale"
# The KIMI blue-green plane owns stable loopback origin 8803 (slots 8804/8805); both the admin
# /kimi-subs projection and the paid Messages surface are read through it.
DEFAULT_PRODUCTION_CAPACITY_PORT = 8803
DEFAULT_PRODUCTION_API_PORT = 8803
KIMI_TARIFF_SCHEDULE_ID = "moonshot/kimi-open-platform/2026-08-03"
# A hot tariff override pins `<family>/v<version>` instead of the compiled schedule id. The
# override is seeded from the same reviewed card, so its rates are the reviewed rates — but the
# identity string legitimately differs, and rejecting it would stop the run on a naming
# difference rather than on a pricing difference. The proof that the card was actually applied
# stays where it belongs: `priced_under_expected_tariff` recomputes every money leg from the
# exact token counts, so a genuinely different rate still fails, whichever identity it carries.
KIMI_TARIFF_OVERRIDE_PIN = re.compile(r"^moonshot/kimi/(?P<model>[a-z0-9.\-]+)/v(?P<version>[1-9][0-9]*)$")


def tariff_identity_is_reviewed(tariff_schedule_id: str, served_model: str) -> bool:
    if tariff_schedule_id == KIMI_TARIFF_SCHEDULE_ID:
        return True
    matched = KIMI_TARIFF_OVERRIDE_PIN.match(tariff_schedule_id)
    return matched is not None and matched.group("model") == served_model
FRACTION_UNITS_PER_PERCENT = 1_000_000  # KIMI_FRACTION_SCALE is 100_000_000 (1% == 1e6 units).
# Thinking-off was believed to re-route k3 and k2.7-code to kimi-k2.6. Disproved live on
# 2026-08-07: `k3-256k` with effort `off` was served and priced as kimi-k3 (3000/15000 per token),
# not k2.6. The belief survived earlier runs only because k2.6 and k2.7-code share input/write/
# output rates, so the base model could not tell them apart. The requested model decides the
# tariff; effort does not change it.
EVENT_TOKEN_FIELDS = (
    "input_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "output_tokens",
    "reasoning_output_tokens",
)
EVENT_MONEY_FIELDS = (
    "api_input_nanousd",
    "api_cache_read_nanousd",
    "api_cache_write_nanousd",
    "api_output_nanousd",
    "api_total_nanousd",
)
COOLING_FIELDS = ("auth_until", "transport_until", "quota_until")


class CalibrationError(RuntimeError):
    """A calibration invariant failed and no further paid request is safe."""


class HttpCalibrationError(CalibrationError):
    def __init__(
        self,
        path: str,
        status: int,
        detail: str,
        execution_not_started: bool = False,
    ) -> None:
        super().__init__(f"{path} returned HTTP {status}: {detail}")
        self.path = path
        self.status = status
        self.detail = detail
        self.execution_not_started = execution_not_started


@dataclasses.dataclass(frozen=True)
class ModelRates:
    """Disjoint official Open Platform per-token rates in nanoUSD for one served model."""

    cached_input: int
    input: int
    cache_write: int  # Kimi publishes no write rate; a write is a miss, so this equals input.
    output: int  # Reasoning tokens are a billed subset of output, never a separate leg.


# Reviewed 2026-08-03 against platform.kimi.ai/docs/pricing/chat-k3, -chat-k27-code, -chat-k26.
# Mirrors crates/metering/src/kimi.rs; identity KIMI_TARIFF_SCHEDULE_ID must move with any epoch.
RATE_CARD: dict[str, ModelRates] = {
    "kimi-k3": ModelRates(cached_input=300, input=3_000, cache_write=3_000, output=15_000),
    "kimi-k2.7-code": ModelRates(cached_input=190, input=950, cache_write=950, output=4_000),
    "kimi-k2.7-code-highspeed": ModelRates(
        cached_input=380, input=1_900, cache_write=1_900, output=8_000
    ),
    "kimi-k2.6": ModelRates(cached_input=160, input=950, cache_write=950, output=4_000),
}


@dataclasses.dataclass(frozen=True)
class AliasSpec:
    """One reviewed subscription alias: its tariff key and accepted input context."""

    official_model: str
    context_mode: str
    input_token_limit: int


ALIAS_SPECS: dict[str, AliasSpec] = {
    "kimi-for-coding": AliasSpec("kimi-k2.7-code", "256k", 262_144),
    "kimi-for-coding-highspeed": AliasSpec("kimi-k2.7-code-highspeed", "256k", 262_144),
    "k3-256k": AliasSpec("kimi-k3", "256k", 262_144),
    "k3": AliasSpec("kimi-k3", "1m", 1_048_576),
}

DEFAULT_MODELS = (
    "kimi-k3",
    "kimi-k2.7-code",
    "kimi-k2.7-code-highspeed",
    "kimi-k2.6",
)

# Reasoning effort the plane accepts per served model family. k3 always reasons and accepts
# low/high/max; the coding family is Thinking-ON (engine default high). Every family documents
# the thinking-off re-route to kimi-k2.6.
EFFORTS_BY_MODEL: dict[str, tuple[str, ...]] = {
    "kimi-k3": ("low", "high", "max", "off"),
    "kimi-k2.7-code": ("high", "off"),
    "kimi-k2.7-code-highspeed": ("high", "off"),
    # kimi-k2.6 has no subscription alias of its own; it is served through the thinking-off
    # re-route, canonically via the cheapest coding alias.
    "kimi-k2.6": ("off",),
}

ALIASES_FOR_MODEL: dict[str, tuple[str, ...]] = {
    "kimi-k3": ("k3-256k", "k3"),
    "kimi-k2.7-code": ("kimi-for-coding",),
    "kimi-k2.7-code-highspeed": ("kimi-for-coding-highspeed",),
    "kimi-k2.6": ("kimi-for-coding",),
}


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


def optional_int(value: Any, field: str) -> int | None:
    return None if value is None else as_int(value, field)


def usd_to_nano(value: str) -> int:
    """Strict exact decimal USD to integer nanoUSD; no float, no exponent notation."""
    whole, dot, fractional = value.strip().partition(".")
    if not whole.isdigit() or (dot and not fractional.isdigit()) or len(fractional) > 9:
        raise CalibrationError(f"invalid exact USD amount: {value!r}")
    return int(whole) * NANO_PER_USD + int((fractional + "000000000")[:9])


def is_explicit_transient_stop(error: HttpCalibrationError) -> bool:
    return error.execution_not_started and error.status in {429, 503}


def validate_profile_id(value: Any) -> str:
    """Opaque roster id: 1..128 chars, ASCII alnum plus `-`/`_` (mirrors kimi-credential)."""
    if (
        not isinstance(value, str)
        or not 1 <= len(value) <= 128
        or not all(char.isascii() and (char.isalnum() or char in "-_") for char in value)
    ):
        raise CalibrationError(f"invalid exact KIMI profile id: {value!r}")
    return value


def validate_calibration_request_id(value: Any) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 36
        or any(char not in "0123456789abcdef-" for char in value)
    ):
        raise CalibrationError(f"invalid exact KIMI calibration request id: {value!r}")
    return value


def require_healthy_delivery(payload: dict[str, Any], require_empty: bool = True) -> None:
    if payload.get("enabled") is not True:
        raise CalibrationError("KIMI provider plane is not enabled")
    if payload.get("calibration_authority_available") is not True:
        raise CalibrationError("KIMI exact calibration authority is unavailable")
    delivery = payload.get("delivery")
    if not isinstance(delivery, dict):
        raise CalibrationError("KIMI response has no delivery diagnostics")
    pending = as_int(delivery.get("pending_events"), "delivery.pending_events")
    dropped = as_int(delivery.get("dropped_events"), "delivery.dropped_events")
    if dropped:
        raise CalibrationError(f"KIMI calibration delivery dropped {dropped} events")
    if delivery.get("persistence_ok") is not True:
        raise CalibrationError("KIMI calibration persistence is degraded")
    if require_empty and pending:
        raise CalibrationError(f"KIMI calibration still has {pending} pending events")


def profile_state(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Parse profiles keyed by opaque id; windows keyed by exact `duration_secs` data."""
    states: dict[str, dict[str, Any]] = {}
    for raw in payload.get("profiles", []):
        if not isinstance(raw, dict):
            continue
        profile_id = raw.get("id")
        if not isinstance(profile_id, str) or not profile_id:
            continue
        cooling = raw.get("cooling")
        cooling_until = 0
        if cooling is not None:
            if not isinstance(cooling, dict):
                raise CalibrationError(f"KIMI profile {profile_id} has invalid cooling data")
            for field in COOLING_FIELDS:
                value = cooling.get(field)
                if value is not None:
                    cooling_until = max(
                        cooling_until, as_int(value, f"profile.cooling.{field}")
                    )
        # `/kimi-subs` publishes the quota windows under `quota`. An absent or non-list value is
        # contract drift, not an empty fleet: silently degrading to zero windows would strip the
        # attribution evidence this runner exists to collect, so fail closed and name the seam.
        raw_quota = raw.get("quota")
        if not isinstance(raw_quota, list):
            raise CalibrationError(
                f"KIMI profile {profile_id} has no `quota` window list in /kimi-subs"
            )
        windows: dict[int, dict[str, Any]] = {}
        for window in raw_quota:
            if not isinstance(window, dict):
                continue
            duration = as_int(window.get("duration_secs"), "window.duration_secs")
            if duration in windows:
                raise CalibrationError(
                    f"KIMI profile {profile_id} has a duplicate window duration {duration}"
                )
            windows[duration] = {
                "used_units": as_int(window.get("used_units"), "window.used_units"),
                "limit_units": as_int(window.get("limit_units"), "window.limit_units"),
                "used_fraction_units": as_int(
                    window.get("used_fraction_units"), "window.used_fraction_units"
                ),
                "measurement_resolution_fraction_units": as_int(
                    window.get("measurement_resolution_fraction_units"),
                    "window.measurement_resolution_fraction_units",
                ),
                "resets_at": optional_int(window.get("resets_at"), "window.resets_at"),
                "observed_at": optional_int(window.get("observed_at"), "window.observed_at"),
            }
        # Auth state is not a boolean on the wire: the engine expresses an authentication
        # quarantine as the `cooling.auth_until` deadline. Keep it separate from the merged
        # cooling deadline so a dead credential reports as such instead of as ordinary cooling.
        auth_quarantined_until = 0
        if isinstance(cooling, dict) and cooling.get("auth_until") is not None:
            auth_quarantined_until = as_int(
                cooling.get("auth_until"), "profile.cooling.auth_until"
            )
        plan = raw.get("plan")
        states[profile_id] = {
            "plan": plan.strip() if isinstance(plan, str) else "",
            "live": raw.get("live") is True,
            "auth_quarantined_until": auth_quarantined_until,
            "inflight": as_int(raw.get("inflight", 0), "profile.inflight"),
            "cooling_until": cooling_until,
            "quota_observed_at": optional_int(
                raw.get("quota_observed_at"), "profile.quota_observed_at"
            ),
            "windows": windows,
        }
    return states


def require_routable_profile(
    state: dict[str, Any] | None, profile_id: str, now: int
) -> dict[str, Any]:
    if state is None:
        raise CalibrationError(f"exact KIMI profile is absent from /kimi-subs: {profile_id}")
    if not state["live"]:
        raise CalibrationError(f"exact KIMI profile is dead: {profile_id}")
    if state["auth_quarantined_until"] > now:
        raise CalibrationError(f"exact KIMI profile is auth-quarantined: {profile_id}")
    if state["cooling_until"] > now:
        raise CalibrationError(f"exact KIMI profile is cooling: {profile_id}")
    return state


def recent_turn_events(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    limit = as_int(
        payload.get("calibration_recent_turn_limit"), "calibration_recent_turn_limit"
    )
    if limit < MIN_RECENT_TURN_LIMIT:
        raise CalibrationError(f"recent-turn window is too small: {limit}")
    raw_events = payload.get("calibration_recent_turns")
    if not isinstance(raw_events, list):
        raise CalibrationError("KIMI response has no immutable recent turns")
    events: dict[str, dict[str, Any]] = {}
    for raw in raw_events:
        if not isinstance(raw, dict):
            raise CalibrationError("recent KIMI turn is not an object")
        identity = (
            raw.get("request_id"),
            raw.get("profile_id"),
            raw.get("requested_model"),
            raw.get("served_model"),
            raw.get("plan"),
            raw.get("context_mode"),
            raw.get("reasoning_effort"),
            raw.get("tariff_schedule_id"),
        )
        if not all(isinstance(value, str) and value for value in identity):
            raise CalibrationError("recent KIMI turn has incomplete identity")
        request_id = raw["request_id"]
        if request_id in events:
            raise CalibrationError(f"duplicate immutable KIMI request id: {request_id}")
        parsed = dict(raw)
        for field in EVENT_TOKEN_FIELDS + EVENT_MONEY_FIELDS:
            if field not in raw:
                raise CalibrationError(
                    f"KIMI turn {request_id} is missing exact vector field {field}"
                )
            parsed[field] = as_int(raw[field], f"calibration_recent_turns.{field}")
        for field in ("priced_ts", "completed_at"):
            parsed[field] = as_int(raw.get(field), f"calibration_recent_turns.{field}")
        money_sum = sum(parsed[field] for field in EVENT_MONEY_FIELDS[:-1])
        if money_sum != parsed["api_total_nanousd"] or money_sum <= 0:
            raise CalibrationError(f"KIMI turn {request_id} has a broken exact cost vector")
        if parsed["reasoning_output_tokens"] > parsed["output_tokens"]:
            raise CalibrationError(f"KIMI turn {request_id} has impossible reasoning output")
        events[request_id] = parsed
    return events


def priced_under_expected_tariff(event: dict[str, Any], rates: ModelRates) -> bool:
    """Every money leg must reproduce exactly under the expected tariff.

    `kimi-k2.6` and `kimi-k2.7-code` are one provider model that differ only in the cached-input
    rate, so the served name alone cannot tell them apart. Recomputing each leg from the exact
    token counts is the only evidence that the tariff we expected is the tariff that was applied —
    and with no cache read in the turn the two remain indistinguishable, which is recorded as an
    open unknown rather than silently resolved.
    """
    legs = (
        ("input_tokens", "api_input_nanousd", rates.input),
        ("cache_read_tokens", "api_cache_read_nanousd", rates.cached_input),
        ("cache_write_tokens", "api_cache_write_nanousd", rates.cache_write),
        # Reasoning output is a subset of `output_tokens`, never an extra billed leg.
        ("output_tokens", "api_output_nanousd", rates.output),
    )
    return all(event[tokens] * rate == event[money] for tokens, money, rate in legs)


def exact_new_turn(
    before_ids: set[str],
    payload: dict[str, Any],
    request_id: str,
    profile_id: str,
    leg: "Leg",
) -> dict[str, Any] | None:
    if request_id in before_ids:
        raise CalibrationError(f"KIMI calibration request id already existed: {request_id}")
    events = recent_turn_events(payload)
    event = events.get(request_id)
    if event is None:
        return None
    # `served_model` on the wire is the provider-facing name we asked for, not the tariff key.
    if event["profile_id"] != profile_id or event["served_model"] != leg.requested_model:
        raise CalibrationError(
            f"KIMI calibration request {request_id} was rebound to "
            f"{event['profile_id']}/{event['served_model']}"
        )
    if (
        event["context_mode"] != leg.context_mode
        or event["reasoning_effort"] != leg.reasoning_effort
    ):
        raise CalibrationError(
            f"KIMI calibration request {request_id} was served as "
            f"{event['context_mode']}/{event['reasoning_effort']}"
        )
    if not priced_under_expected_tariff(event, RATE_CARD[leg.served_model]):
        raise CalibrationError(
            f"KIMI calibration request {request_id} was not priced under {leg.served_model}"
        )
    return event


def window_observation_delta(
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
    completed_at: int,
) -> dict[str, Any]:
    """Per-window quota delta; unresolved or reset-crossed is never a zero."""
    result: dict[str, Any] = {
        "status": "resolved",
        "fraction_delta": None,
        "native_delta": None,
        "before": before,
        "after": after,
    }
    if before is None or after is None or after["observed_at"] is None:
        result["status"] = "unresolved"
        return result
    if after["observed_at"] < completed_at:
        result["status"] = "unresolved"
        return result
    if before["resets_at"] is None or before["resets_at"] != after["resets_at"]:
        result["status"] = "reset-crossed"
        return result
    fraction_delta = after["used_fraction_units"] - before["used_fraction_units"]
    native_delta = after["used_units"] - before["used_units"]
    result["fraction_delta"] = fraction_delta if fraction_delta >= 0 else None
    result["native_delta"] = native_delta if native_delta >= 0 else None
    return result


@dataclasses.dataclass(frozen=True)
class Leg:
    name: str
    requested_model: str  # Exact reviewed subscription alias sent as `model`.
    served_model: str  # Served model the immutable event must report.
    context_mode: str
    reasoning_effort: str
    max_output_tokens: int = DEFAULT_MAX_OUTPUT_TOKENS
    # `None` for an ordinary generation leg; otherwise the capability this leg exists to price.
    # A capability leg is the only way to learn what a tool call or a media part costs on the
    # subscription route, and it is also the one thing the plane refuses to serve until that cost
    # is known — so it never runs on the ordinary budget and never runs by default.
    capability: str | None = None


def build_coverage_legs(models: list[str] | tuple[str, ...]) -> list[Leg]:
    legs: list[Leg] = []
    seen: set[tuple[str, str, str]] = set()
    for model in models:
        if model not in RATE_CARD:
            raise CalibrationError(f"unknown KIMI served model: {model}")
        for alias in ALIASES_FOR_MODEL[model]:
            spec = ALIAS_SPECS[alias]
            for effort in EFFORTS_BY_MODEL[model]:
                key = (alias, spec.context_mode, effort)
                if key in seen:
                    continue
                seen.add(key)
                served = spec.official_model
                legs.append(
                    Leg(f"{alias}:{spec.context_mode}:{effort}", alias, served, spec.context_mode, effort)
                )
    return legs


# One minimal probe per capability. The bodies are deliberately the smallest thing that still
# exercises the surface: a single tool the model can call at most once, and a 1x1 image part. The
# point is to learn the unit cost, not to test behaviour, so nothing here should ever grow.
CAPABILITY_PROBES: dict[str, dict[str, Any]] = {
    "tools": {
        "tools": [
            {
                "name": "calibration_probe",
                "description": "Return the string OK.",
                "input_schema": {"type": "object", "properties": {}, "required": []},
            }
        ],
        "tool_choice": {"type": "auto"},
    },
    "media": {
        "_media_part": {
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": (
                    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk"
                    "YPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
                ),
            },
        }
    },
}


def build_capability_legs(models: list[str] | tuple[str, ...]) -> list[Leg]:
    """One probe per capability per model, on the narrowest context the alias accepts.

    These are separated from `build_coverage_legs` because they are the legs whose price is
    unknown by definition: the plane refuses tools and media precisely because no finite
    per-request unit ceiling is proved. Running them is how that changes, and it is why they
    carry their own explicit authorization rather than sharing the coverage budget.
    """
    legs: list[Leg] = []
    for model in models:
        if model not in RATE_CARD:
            raise CalibrationError(f"unknown KIMI served model: {model}")
        alias = min(
            ALIASES_FOR_MODEL[model],
            key=lambda name: (ALIAS_SPECS[name].input_token_limit, name),
        )
        spec = ALIAS_SPECS[alias]
        effort = EFFORTS_BY_MODEL[model][0]
        served = spec.official_model
        for capability in sorted(CAPABILITY_PROBES):
            legs.append(
                Leg(
                    f"{alias}:{spec.context_mode}:{effort}:{capability}",
                    alias,
                    served,
                    spec.context_mode,
                    effort,
                    capability=capability,
                )
            )
    return legs


def upper_bound_candidates(leg: Leg) -> tuple[AliasSpec, set[str]]:
    spec = ALIAS_SPECS.get(leg.requested_model)
    if spec is None:
        raise CalibrationError(f"unknown KIMI subscription alias: {leg.requested_model}")
    # Effort never changes the tariff: the requested alias decides it (live evidence 2026-08-07).
    candidates = {spec.official_model}
    return spec, candidates


def request_upper_bound_nano(leg: Leg, rates: dict[str, ModelRates]) -> int:
    """Worst case: full accepted input context at the miss rate plus all requested output.

    Billing follows the served model, and thinking off may re-route to kimi-k2.6, so every
    plausible served rate card contributes its per-class maximum. A cache write is a miss and
    the miss rate dominates the hit rate on every card, so pricing the complete accepted input
    context at the miss rate covers every cache class split.
    """
    spec, candidates = upper_bound_candidates(leg)
    missing = sorted(candidates - set(rates))
    if missing:
        raise CalibrationError("no authoritative KIMI rate card for: " + ", ".join(missing))
    input_rate = max(rates[model].input for model in candidates)
    output_rate = max(rates[model].output for model in candidates)
    return spec.input_token_limit * input_rate + leg.max_output_tokens * output_rate


def prompt_for_leg(leg: Leg, run_id: str) -> str:
    return f"KIMI calibration {run_id} {leg.name}. Reply with exactly CALIBRATION_OK."


def body_for_leg(leg: Leg, run_id: str) -> dict[str, Any]:
    body: dict[str, Any] = {
        "model": leg.requested_model,
        "max_tokens": leg.max_output_tokens,
        "messages": [{"role": "user", "content": prompt_for_leg(leg, run_id)}],
        "reasoning_effort": leg.reasoning_effort,
    }
    if leg.capability is None:
        return body
    probe = CAPABILITY_PROBES.get(leg.capability)
    if probe is None:
        raise CalibrationError(f"unknown KIMI capability probe: {leg.capability}")
    media_part = probe.get("_media_part")
    if media_part is not None:
        body["messages"] = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt_for_leg(leg, run_id)},
                    media_part,
                ],
            }
        ]
    for field, value in probe.items():
        if field != "_media_part":
            body[field] = value
    return body


@dataclasses.dataclass
class Budget:
    limit_nano: int
    total_nano: int = 0
    by_profile: dict[str, int] = dataclasses.field(default_factory=lambda: defaultdict(int))

    def require(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if self.total_nano + upper_bound_nano > self.limit_nano:
            raise CalibrationError("aggregate KIMI budget guard stopped before dispatch")

    def charge(self, profile_id: str, actual_nano: int, upper_bound_nano: int) -> None:
        if actual_nano <= 0 or actual_nano > upper_bound_nano:
            raise CalibrationError("KIMI backend evidence violated the preflight cost bound")
        if self.total_nano + actual_nano > self.limit_nano:
            raise CalibrationError("KIMI backend evidence exceeded the global live budget")
        self.total_nano += actual_nano
        self.by_profile[profile_id] += actual_nano


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
        target_profile: str | None = None,
        calibration_request_id: str | None = None,
    ) -> Any:
        data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
        headers = {
            "x-api-key": self.api_key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
            "accept": "application/json",
        }
        if target_profile:
            headers["x-apitoken-calibration-profile"] = validate_profile_id(target_profile)
        if calibration_request_id:
            headers["x-apitoken-calibration-request-id"] = validate_calibration_request_id(
                calibration_request_id
            )
        request = urllib.request.Request(
            f"{self.api_url}{path}", data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            raise HttpCalibrationError(
                path,
                error.code,
                error.read(800).decode(errors="replace"),
                error.headers is not None
                and error.headers.get("x-apitoken-execution-state") == "not_started",
            ) from error
        except urllib.error.URLError as error:
            raise CalibrationError(f"{path} transport failed: {error}") from error
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload


def validate_production_ssh_target(value: str) -> str:
    if (
        not value
        or len(value) > 255
        or not value[0].isalnum()
        or not all(char.isascii() and (char.isalnum() or char in ".-_:@") for char in value)
        or value.count("@") > 1
    ):
        raise CalibrationError(f"invalid production SSH target: {value!r}")
    return value


def validate_production_api_port(value: int) -> int:
    if isinstance(value, bool) or not 1 <= value <= 65_535:
        raise CalibrationError(f"invalid production API port: {value!r}")
    return value


class ProductionSshJsonHttpClient:
    """Call the stable loopback Anthropic plane with a remote-only forwarding-admin credential.

    The credential is expanded only by the production shell and never crosses SSH or enters the
    report. The admin-only calibration headers make each paid turn exact-profile/exact-request-id
    and are stripped before the request reaches Kimi. A paid `/v1/messages` request gets exactly
    one transport attempt; only read-only GETs are retried.
    """

    def __init__(
        self,
        timeout: int,
        ssh_target: str = DEFAULT_PRODUCTION_SSH_TARGET,
        api_port: int = DEFAULT_PRODUCTION_API_PORT,
    ) -> None:
        self.timeout = timeout
        self.ssh_target = validate_production_ssh_target(ssh_target)
        self.api_port = validate_production_api_port(api_port)

    def request(
        self,
        path: str,
        method: str = "GET",
        body: dict[str, Any] | None = None,
        target_profile: str | None = None,
        calibration_request_id: str | None = None,
    ) -> Any:
        if method not in {"GET", "POST"} or not path.startswith("/v1/"):
            raise CalibrationError(f"unsupported KIMI SSH request: {method} {path}")
        if any(
            char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/_-?=&.:"
            for char in path
        ):
            raise CalibrationError(f"unsafe KIMI SSH path: {path!r}")
        headers = [
            "anthropic-version: 2023-06-01",
            "content-type: application/json",
            "accept: application/json",
        ]
        if target_profile:
            headers.append(
                f"x-apitoken-calibration-profile: {validate_profile_id(target_profile)}"
            )
        if calibration_request_id:
            headers.append(
                "x-apitoken-calibration-request-id: "
                + validate_calibration_request_id(calibration_request_id)
            )
        header_args = " ".join(f"-H {shlex.quote(header)}" for header in headers)
        data_arg = "--data-binary @-" if body is not None else ""
        remote = (
            "set -a && . /srv/claude-api/data/server.env && set +a && "
            "calibration_key=${CLAUDE_API_KEYS%%,*} && test -n \"$calibration_key\" && "
            f"curl -sS --max-time {self.timeout} "
            "-w '\\n__CALIBRATION_HTTP__%{http_code}\\n"
            "%header{x-apitoken-execution-state}' "
            f"-X {method} "
            f"-H \"x-api-key: $calibration_key\" {header_args} {data_arg} "
            f"{shlex.quote(f'http://127.0.0.1:{self.api_port}' + path)}"
        )
        data = b"" if body is None else json.dumps(body, separators=(",", ":")).encode()
        safe = method == "GET"
        attempts = SAFE_READ_ATTEMPTS if safe else 1
        result = None
        for attempt in range(attempts):
            result = subprocess.run(
                ["ssh", self.ssh_target, remote],
                input=data,
                capture_output=True,
                timeout=self.timeout + 30,
                check=False,
            )
            if result.returncode == 0:
                break
            if attempt + 1 == attempts:
                raise CalibrationError(
                    f"{path} SSH transport failed: {result.stderr[-800:].decode(errors='replace')}"
                )
            time.sleep(SAFE_READ_RETRY_DELAY_SECONDS)
        if result is None:
            raise CalibrationError(f"{path} produced no SSH result")
        raw, separator, trailer = result.stdout.rpartition(b"\n__CALIBRATION_HTTP__")
        status_raw, header_separator, execution_state = trailer.partition(b"\n")
        if not separator or not header_separator or not status_raw.isdigit():
            raise CalibrationError(f"{path} SSH response has no HTTP status")
        status = int(status_raw)
        if status >= 400:
            raise HttpCalibrationError(
                path,
                status,
                raw[:800].decode(errors="replace"),
                execution_state.strip() == b"not_started",
            )
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload


class CapacityReader:
    def __init__(
        self, command: str | None, url: str | None, control_key: str | None, timeout: int
    ) -> None:
        self.command = shlex.split(command) if command else None
        self.url = url
        self.control_key = control_key
        self.timeout = timeout
        if not self.command and not self.url:
            raise CalibrationError("set --capacity-command or --capacity-url")
        if self.url and not self.control_key:
            raise CalibrationError("control key is required with --capacity-url")

    def read_once(self) -> dict[str, Any]:
        if self.command:
            result = subprocess.run(
                self.command, capture_output=True, timeout=self.timeout, check=False
            )
            if result.returncode:
                raise CalibrationError(
                    f"capacity command failed: {result.stderr[-500:].decode(errors='replace')}"
                )
            raw = result.stdout
        else:
            request = urllib.request.Request(
                self.url or "", headers={"x-api-key": self.control_key or ""}
            )
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    raw = response.read()
            except urllib.error.URLError as error:
                raise CalibrationError(f"capacity read failed: {error}") from error
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError("capacity source returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError("capacity source returned a non-object")
        return payload

    def read(self) -> dict[str, Any]:
        """Bounded retry is allowed because /kimi-subs reads are strictly read-only."""
        for attempt in range(SAFE_READ_ATTEMPTS):
            try:
                return self.read_once()
            except (CalibrationError, subprocess.TimeoutExpired):
                if attempt + 1 == SAFE_READ_ATTEMPTS:
                    raise
                time.sleep(SAFE_READ_RETRY_DELAY_SECONDS)
        raise CalibrationError("capacity source produced no result")


class Runner:
    def __init__(
        self,
        api: Any,
        capacity: CapacityReader,
        rates: dict[str, ModelRates],
        budget: Budget,
        timeout: int,
        delay: float,
        run_id: str,
    ) -> None:
        self.api = api
        self.capacity = capacity
        self.rates = rates
        self.budget = budget
        self.timeout = timeout
        self.delay = delay
        self.run_id = run_id
        self.records: list[dict[str, Any]] = []

    def execute_leg(self, leg: Leg, profile_id: str) -> dict[str, Any]:
        before = self.capacity.read()
        require_healthy_delivery(before)
        state = require_routable_profile(
            profile_state(before).get(profile_id), profile_id, int(time.time())
        )
        before_ids = set(recent_turn_events(before))
        upper = request_upper_bound_nano(leg, self.rates)
        self.budget.require(upper)
        body = body_for_leg(leg, self.run_id)
        calibration_request_id = str(uuid.uuid4())
        if calibration_request_id in before_ids:
            raise CalibrationError("generated KIMI calibration request id already exists")
        self.api.request(
            "/v1/messages",
            "POST",
            body,
            profile_id,
            calibration_request_id=calibration_request_id,
        )
        deadline = time.monotonic() + self.timeout
        event = None
        observed = before
        while time.monotonic() < deadline:
            time.sleep(2)
            observed = self.capacity.read()
            require_healthy_delivery(observed, require_empty=False)
            event = exact_new_turn(
                before_ids,
                observed,
                calibration_request_id,
                profile_id,
                leg,
            )
            if event is not None and observed.get("delivery", {}).get("pending_events") == 0:
                break
        if event is None:
            raise CalibrationError(f"{leg.name}: exact immutable KIMI event did not appear")
        if not tariff_identity_is_reviewed(event["tariff_schedule_id"], leg.served_model):
            raise CalibrationError(
                f"{leg.name}: immutable event tariff {event['tariff_schedule_id']!r} is neither "
                f"the reviewed {KIMI_TARIFF_SCHEDULE_ID!r} card nor a hot override of "
                f"{leg.served_model!r}"
            )
        actual = event["api_total_nanousd"]
        self.budget.charge(profile_id, actual, upper)
        completed_at = event["completed_at"]
        require_healthy_delivery(observed)
        if self.delay > 0:
            time.sleep(self.delay)
        quota_deadline = time.monotonic() + self.timeout
        before_windows = state["windows"]
        after_windows: dict[int, dict[str, Any]] = {}
        while True:
            observed = self.capacity.read()
            require_healthy_delivery(observed)
            after_state = profile_state(observed).get(profile_id)
            after_windows = after_state["windows"] if after_state else {}
            resolved = all(
                duration in after_windows
                and after_windows[duration]["observed_at"] is not None
                and after_windows[duration]["observed_at"] >= completed_at
                for duration in before_windows
            )
            if resolved or time.monotonic() >= quota_deadline:
                break
            time.sleep(2)
        window_records = []
        quota_snapshot_resolved = True
        for duration in sorted(before_windows):
            delta = window_observation_delta(
                before_windows[duration], after_windows.get(duration), completed_at
            )
            if delta["status"] != "resolved":
                quota_snapshot_resolved = False
            delta["duration_secs"] = duration
            window_records.append(delta)
        after_events = recent_turn_events(observed)
        concurrent_profile_request_ids = sorted(
            request_id
            for request_id, candidate in after_events.items()
            if request_id not in before_ids
            and request_id != calibration_request_id
            and candidate["profile_id"] == profile_id
        )
        profitability_eligible = (
            quota_snapshot_resolved
            and calibration_request_id in after_events
            and not concurrent_profile_request_ids
        )
        record = {
            "profile_id": profile_id,
            "plan": state["plan"],
            "leg": leg.name,
            "requested_model": leg.requested_model,
            "served_model": leg.served_model,
            "context_mode": leg.context_mode,
            "reasoning_effort": leg.reasoning_effort,
            "max_output_tokens": leg.max_output_tokens,
            "prompt_sha256_12": hashlib.sha256(
                prompt_for_leg(leg, self.run_id).encode()
            ).hexdigest()[:12],
            "request_id": event["request_id"],
            "tariff_schedule_id": event["tariff_schedule_id"],
            "upper_bound_nanousd": str(upper),
            "actual_nanousd": str(actual),
            "usage": {field: str(event[field]) for field in EVENT_TOKEN_FIELDS},
            "api_cost": {field: str(event[field]) for field in EVENT_MONEY_FIELDS},
            "windows": window_records,
            "quota_snapshot_resolved": quota_snapshot_resolved,
            "concurrent_profile_request_ids": concurrent_profile_request_ids,
            "profitability_eligible": profitability_eligible,
        }
        self.records.append(record)
        print(f"{profile_id} {leg.name}: ${actual / NANO_PER_USD:.6f}", flush=True)
        return record


def model_profitability(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """API nanoUSD per 1% of each exact-duration window; positive resolved deltas only."""
    grouped: dict[tuple[str, str, str, str, int], dict[str, int]] = defaultdict(
        lambda: {"nano": 0, "fraction": 0, "turns": 0}
    )
    for record in records:
        if (
            record.get("profitability_eligible") is not True
            or record.get("quota_snapshot_resolved") is not True
        ):
            continue
        for window in record.get("windows", []):
            delta = window.get("fraction_delta")
            if window.get("status") != "resolved" or delta is None or int(delta) <= 0:
                continue
            key = (
                record["plan"],
                record["served_model"],
                record["context_mode"],
                record["reasoning_effort"],
                int(window["duration_secs"]),
            )
            grouped[key]["nano"] += int(record["actual_nanousd"])
            grouped[key]["fraction"] += int(delta)
            grouped[key]["turns"] += 1
    rows = []
    for (plan, model, context_mode, effort, duration), value in grouped.items():
        per_one_percent = value["nano"] * FRACTION_UNITS_PER_PERCENT // value["fraction"]
        rows.append({
            "plan": plan,
            "served_model": model,
            "context_mode": context_mode,
            "reasoning_effort": effort,
            "window_duration_secs": duration,
            "turns": value["turns"],
            "api_nanousd_per_1pct_window": str(per_one_percent),
        })
    return sorted(
        rows,
        key=lambda row: int(row["api_nanousd_per_1pct_window"]),
        reverse=True,
    )


def remote_capacity_command(
    ssh_target: str = DEFAULT_PRODUCTION_SSH_TARGET,
    api_port: int = DEFAULT_PRODUCTION_CAPACITY_PORT,
) -> str:
    ssh_target = validate_production_ssh_target(ssh_target)
    api_port = validate_production_api_port(api_port)
    return (
        f"ssh {shlex.quote(ssh_target)} 'set -a; . /srv/claude-api/data/server.env; set +a; "
        'curl -fsS -H "x-api-key: $CLAUDE_API_CONTROL_KEY" '
        f"http://127.0.0.1:{api_port}/kimi-subs'"
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="required to send live requests")
    parser.add_argument("--profile", help="exact opaque KIMI profile id (required with --execute)")
    parser.add_argument("--api-url", default="https://api.apitoken.sale")
    parser.add_argument("--api-key-env", default="APITOKEN_API_KEY")
    parser.add_argument("--capacity-command", default=os.getenv("KIMI_CALIBRATION_CAPACITY_COMMAND"))
    parser.add_argument("--capacity-url")
    parser.add_argument("--control-key-env", default="CLAUDE_API_CONTROL_KEY")
    parser.add_argument("--budget-usd", default=DEFAULT_BUDGET_USD)
    parser.add_argument(
        "--capability-probe-budget-usd",
        default=None,
        help=(
            "separate explicit authorization for the tool/media probes. Absent: the capabilities "
            "are recorded as unavailable and nothing is spent on them. They never draw on "
            "--budget-usd, because their per-request unit cost is exactly what is unproven."
        ),
    )
    parser.add_argument("--models", nargs="*")
    parser.add_argument(
        "--one-m-plans",
        nargs="*",
        default=[],
        help="reviewed paid plans allowed to run 1m-context legs (empty: 1m is unavailable)",
    )
    parser.add_argument("--evidence-timeout", type=int, default=DEFAULT_EVIDENCE_TIMEOUT_SECONDS)
    parser.add_argument("--profile-delay", type=float, default=DEFAULT_PROFILE_DELAY_SECONDS)
    parser.add_argument("--http-timeout", type=int, default=240)
    parser.add_argument("--report", default="/tmp/kimi-calibration-report.json")
    parser.add_argument("--production-capacity-over-ssh", action="store_true")
    parser.add_argument("--production-api-over-ssh", action="store_true")
    parser.add_argument("--production-ssh-target", default=DEFAULT_PRODUCTION_SSH_TARGET)
    parser.add_argument(
        "--production-capacity-port",
        type=int,
        default=DEFAULT_PRODUCTION_CAPACITY_PORT,
    )
    parser.add_argument("--production-api-port", type=int, default=DEFAULT_PRODUCTION_API_PORT)
    args = parser.parse_args(argv)
    try:
        budget_nano = usd_to_nano(args.budget_usd)
        if budget_nano <= 0 or budget_nano > MAX_BUDGET_NANO:
            raise CalibrationError("--budget-usd must be positive and no greater than 10.00")
        if args.capability_probe_budget_usd is not None:
            capability_probe_nano = usd_to_nano(args.capability_probe_budget_usd)
            if (
                capability_probe_nano <= 0
                or capability_probe_nano > MAX_CAPABILITY_PROBE_NANO
            ):
                raise CalibrationError(
                    "--capability-probe-budget-usd must be positive and no greater than "
                    + MAX_CAPABILITY_PROBE_USD
                )
        if args.profile is not None:
            validate_profile_id(args.profile)
        validate_production_ssh_target(args.production_ssh_target)
        validate_production_api_port(args.production_capacity_port)
        validate_production_api_port(args.production_api_port)
    except CalibrationError as error:
        parser.error(str(error))
    if args.execute and not args.profile:
        parser.error("--profile is required with --execute")
    return args


def dry_run_plan(args: argparse.Namespace, budget_nano: int) -> dict[str, Any]:
    models = args.models or list(DEFAULT_MODELS)
    legs = []
    planned = list(build_coverage_legs(models))
    if args.capability_probe_budget_usd is not None:
        planned += build_capability_legs(models)
    for leg in planned:
        legs.append({
            "leg": leg.name,
            "requested_model": leg.requested_model,
            "served_model": leg.served_model,
            "context_mode": leg.context_mode,
            "reasoning_effort": leg.reasoning_effort,
            "max_output_tokens": leg.max_output_tokens,
            "upper_bound_nanousd": str(request_upper_bound_nano(leg, RATE_CARD)),
            "requires_reviewed_plan": leg.context_mode == "1m",
        })
    return {
        "schema": "kimi-live-calibration-plan/v1",
        "mode": "dry-run",
        "paid_requests": 0,
        "budget_nanousd_total": str(budget_nano),
        "budget_hard_cap_nanousd": str(MAX_BUDGET_NANO),
        "profile": args.profile,
        "models": models,
        "one_m_plans": args.one_m_plans,
        "legs": legs,
        "guards": [
            "exact-opaque-profile-target",
            "uuidv4-request-id-attribution",
            "healthy-authority-and-empty-fifo",
            "full-input-context-miss-rate-plus-max-output-bound",
            "served-model-rate-card-including-thinking-off-reroute",
            "single-aggregate-budget-hard-capped-at-0.0001-usd",
            "no-paid-request-retry-after-transport-ambiguity",
            "post-turn-window-observation-before-quota-deltas",
            "reset-crossed-window-is-never-a-delta",
            "secrets-from-env-or-remote-shell-only",
        ],
        "execute_requires": (
            "--execute plus explicit human authorization, a capacity source and "
            "production/admin API access"
        ),
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    budget_nano = usd_to_nano(args.budget_usd)
    if not args.execute:
        print(json.dumps(dry_run_plan(args, budget_nano), ensure_ascii=False, indent=2))
        return 0
    api_key = os.getenv(args.api_key_env, "")
    if not args.production_api_over_ssh and not api_key:
        raise CalibrationError(f"missing API key environment variable: {args.api_key_env}")
    capacity = CapacityReader(
        remote_capacity_command(args.production_ssh_target, args.production_capacity_port)
        if args.production_capacity_over_ssh
        else args.capacity_command,
        args.capacity_url,
        os.getenv(args.control_key_env),
        args.http_timeout,
    )
    api = (
        ProductionSshJsonHttpClient(
            args.http_timeout,
            args.production_ssh_target,
            args.production_api_port,
        )
        if args.production_api_over_ssh
        else JsonHttpClient(args.api_url, api_key, args.http_timeout)
    )
    baseline = capacity.read()
    require_healthy_delivery(baseline)
    state = require_routable_profile(
        profile_state(baseline).get(args.profile), args.profile, int(time.time())
    )
    if not state["plan"]:
        raise CalibrationError("exact KIMI profile has no authoritative paid plan")
    models = args.models or list(DEFAULT_MODELS)
    unknown = sorted(set(models) - set(RATE_CARD))
    if unknown:
        raise CalibrationError("models have no authoritative KIMI rate card: " + ", ".join(unknown))
    run_id = f"kimi-cal-{int(time.time())}-{uuid.uuid4().hex[:8]}"
    budget = Budget(budget_nano)
    runner = Runner(
        api,
        capacity,
        RATE_CARD,
        budget,
        args.evidence_timeout,
        args.profile_delay,
        run_id,
    )
    legs = build_coverage_legs(models)
    unavailable: list[dict[str, Any]] = []
    # Without its own authorization a capability is reported as untested, never quietly dropped.
    # `blocking=False` because an unproven tool price is the documented state of this provider,
    # not a failure of this run.
    if args.capability_probe_budget_usd is None:
        for probe in build_capability_legs(models):
            unavailable.append({
                "profile_id": args.profile,
                "model": probe.served_model,
                "capability": probe.name,
                "reason": (
                    "per-request unit cost is unproven and no --capability-probe-budget-usd "
                    "was authorized, so nothing was spent on it"
                ),
                "blocking": False,
                "skipped_before_dispatch": True,
            })
    else:
        legs = legs + build_capability_legs(models)
    stops: list[dict[str, str]] = []
    completed: set[str] = set()
    failure: str | None = None
    try:
        for leg in legs:
            if leg.context_mode == "1m" and state["plan"] not in args.one_m_plans:
                unavailable.append({
                    "profile_id": args.profile,
                    "model": leg.served_model,
                    "capability": leg.name,
                    "reason": (
                        f"1m context is not reviewed for paid plan {state['plan']!r}"
                    ),
                    "blocking": False,
                    "skipped_before_dispatch": True,
                })
                completed.add(leg.name)
                continue
            try:
                runner.execute_leg(leg, args.profile)
                completed.add(leg.name)
            except HttpCalibrationError as error:
                if error.status in {400, 403, 404}:
                    unavailable.append({
                        "profile_id": args.profile,
                        "model": leg.served_model,
                        "capability": leg.name,
                        "http_status": error.status,
                        "reason": error.detail[:300],
                        "blocking": True,
                    })
                    completed.add(leg.name)
                    raise CalibrationError(
                        f"{args.profile}/{leg.name}: required generation capability returned "
                        f"HTTP {error.status}"
                    )
                if is_explicit_transient_stop(error):
                    stops.append({
                        "scope": f"profile:{args.profile}",
                        "reason": str(error),
                    })
                    break
                raise
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        failure = str(error)
    try:
        final = capacity.read()
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        final = baseline
        failure = failure or f"final KIMI subs read failed: {error}"
    pending = [
        {
            "profile_id": args.profile,
            "model": leg.served_model,
            "capability": leg.name,
        }
        for leg in legs
        if leg.name not in completed
    ]
    blocking_unavailable = [item for item in unavailable if item.get("blocking", True)]
    complete = failure is None and not pending and not blocking_unavailable
    report = {
        "schema": "kimi-live-calibration/v1",
        "run_id": run_id,
        "complete": complete,
        "failure": failure,
        "budget_nanousd_total": str(budget_nano),
        "spent_nanousd_total": str(budget.total_nano),
        "spent_nanousd_per_profile": {
            key: str(value) for key, value in sorted(budget.by_profile.items())
        },
        "profile": args.profile,
        "plan": state["plan"],
        "models": models,
        "one_m_plans": args.one_m_plans,
        "records": runner.records,
        "unavailable_capabilities": unavailable,
        "stops": stops,
        "coverage": {
            "expected_legs": [leg.name for leg in legs],
            "completed_legs": sorted(completed),
            "pending_legs": pending,
        },
        "model_profitability": model_profitability(runner.records),
        "production_transport": {
            "capacity_over_ssh": args.production_capacity_over_ssh,
            "api_over_ssh": args.production_api_over_ssh,
            "ssh_target": args.production_ssh_target if (
                args.production_capacity_over_ssh or args.production_api_over_ssh
            ) else None,
            "capacity_port": args.production_capacity_port if args.production_capacity_over_ssh else None,
            "api_port": args.production_api_port if args.production_api_over_ssh else None,
        },
        "final_observations": final,
    }
    report_path = Path(args.report)
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"report: {report_path}")
    if not complete:
        reason = failure or f"{len(pending)} KIMI coverage legs remain after explicit stops"
        raise CalibrationError(f"{reason}; partial report: {report_path}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        print(f"KIMI calibration stopped safely: {error}", file=sys.stderr)
        sys.exit(1)
