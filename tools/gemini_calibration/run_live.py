#!/usr/bin/env python3
"""Fail-closed live calibration runner for the pooled native Gemini backend.

The runner spends only after an exact-profile preflight and treats `/gemini-subs` immutable turn
events as the sole API-dollar authority. Dry-run is the default; live traffic requires `--execute`.
"""

from __future__ import annotations

import argparse
import base64
import dataclasses
import json
import os
import shlex
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable


NANO_PER_USD = 1_000_000_000
MAX_BUDGET_NANO = 40 * NANO_PER_USD
MIN_RECENT_TURN_LIMIT = 512
SAFE_READ_ATTEMPTS = 3
SAFE_READ_RETRY_DELAY_SECONDS = 2.0
DEFAULT_EVIDENCE_TIMEOUT_SECONDS = 180
DEFAULT_PROFILE_DELAY_SECONDS = 16.0
DEFAULT_PRODUCTION_SSH_TARGET = "apitokensale"
DEFAULT_PRODUCTION_CAPACITY_PORT = 8794
DEFAULT_PRODUCTION_API_PORT = 8794
IMAGE_OUTPUT_TOKEN_CEILINGS = {"1K": 1_120, "2K": 1_680, "4K": 2_520}
EVENT_TOKEN_FIELDS = (
    "input_tokens",
    "audio_input_tokens",
    "cache_read_tokens",
    "cached_audio_input_tokens",
    "cache_write_5m_tokens",
    "cache_write_1h_tokens",
    "output_tokens",
    "thinking_output_tokens",
    "image_output_tokens",
    "tool_prompt_tokens",
    "search_queries",
    "grounded_search_prompts",
)
EVENT_MONEY_FIELDS = (
    "api_input_nanousd",
    "api_audio_input_nanousd",
    "api_cache_read_nanousd",
    "api_cached_audio_input_nanousd",
    "api_cache_write_5m_nanousd",
    "api_cache_write_1h_nanousd",
    "api_output_nanousd",
    "api_image_output_nanousd",
    "api_search_nanousd",
    "api_total_nanousd",
)
THINKING_TOKENS_NOT_OBSERVED = "thinking output token class was not observed"


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


class UnboundedCostError(CalibrationError):
    """A provider capability has no proved per-request money ceiling."""


@dataclasses.dataclass(frozen=True)
class GenerationResponse:
    """Parsed successful generation body retained only in memory for response proof."""

    frames: tuple[dict[str, Any], ...]
    stream: bool
    parse_error: str | None = None


@dataclasses.dataclass(frozen=True)
class ResumeState:
    run_id: str
    profiles: list[str]
    models: list[str]
    records: list[dict[str, Any]]
    unavailable: list[dict[str, Any]]
    spent_nano: int
    spent_by_profile: dict[str, int]


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
    whole, dot, fractional = value.strip().partition(".")
    if not whole.isdigit() or (dot and not fractional.isdigit()) or len(fractional) > 9:
        raise CalibrationError(f"invalid exact USD amount: {value!r}")
    return int(whole) * NANO_PER_USD + int((fractional + "000000000")[:9])


def is_explicit_transient_stop(error: HttpCalibrationError) -> bool:
    return error.execution_not_started and error.status in {429, 503}


def _string_list(value: Any, field: str) -> list[str]:
    if not isinstance(value, list) or not value:
        raise CalibrationError(f"resume report {field} must be a non-empty list")
    parsed = []
    for item in value:
        if not isinstance(item, str) or not item:
            raise CalibrationError(f"resume report {field} contains an invalid value")
        parsed.append(item)
    if len(set(parsed)) != len(parsed):
        raise CalibrationError(f"resume report {field} contains duplicates")
    return parsed


def _minimal_thinking_reclassification(payload: dict[str, Any]) -> tuple[str, str] | None:
    """Recognize one completed turn stopped only by the obsolete minimal-token assertion.

    This proof never authorizes replay of a paid request. It permits a newer runner to retain the
    exact completed record and continue only the still-pending matrix legs.
    """

    records = payload.get("records")
    unavailable = payload.get("unavailable_capabilities")
    blocking = payload.get("blocking_unavailable_capabilities")
    pending = payload.get("pending_legs")
    if (
        payload.get("schema") != "gemini-live-calibration/v2"
        or payload.get("complete") is not False
        or payload.get("resume_safe") is not False
        or not isinstance(records, list)
        or not isinstance(unavailable, list)
        or not isinstance(blocking, list)
        or not isinstance(pending, list)
        or not pending
        or len(blocking) != 1
    ):
        return None
    miss = blocking[0]
    if (
        not isinstance(miss, dict)
        or miss.get("blocking") is not True
        or miss.get("reason") != THINKING_TOKENS_NOT_OBSERVED
    ):
        return None
    profile_id, leg = miss.get("profile_id"), miss.get("capability")
    model = miss.get("model")
    if (
        not all(isinstance(value, str) and value for value in (profile_id, leg, model))
        or model != "gemini-3-flash-preview"
        or leg != f"thinking:{model}:minimal"
    ):
        return None
    matching_unavailable = [
        item
        for item in unavailable
        if isinstance(item, dict)
        and item.get("profile_id") == profile_id
        and item.get("capability") == leg
        and item.get("model") == model
        and item.get("reason") == THINKING_TOKENS_NOT_OBSERVED
        and item.get("blocking") is True
    ]
    matching_records = [
        record
        for record in records
        if isinstance(record, dict)
        and record.get("profile_id") == profile_id
        and record.get("leg") == leg
        and record.get("model") == model
    ]
    if len(matching_unavailable) != 1 or len(unavailable) != 1 or len(matching_records) != 1:
        return None
    record = matching_records[0]
    evidence = record.get("response_evidence")
    usage = record.get("usage")
    expected_failure = (
        f"{profile_id}/{leg}: paid response proof failed: {THINKING_TOKENS_NOT_OBSERVED}"
    )
    if (
        payload.get("failure") != expected_failure
        or record.get("kind") != "thinking"
        or record.get("thinking_level") != "minimal"
        or record.get("stream") is not False
        or record.get("coverage_error") != THINKING_TOKENS_NOT_OBSERVED
        or not isinstance(evidence, dict)
        or evidence.get("model_version") != model
        or evidence.get("terminal_finish") is not True
        or evidence.get("terminal_usage") is not True
        or evidence.get("usage_matches_immutable_event") is not True
        or evidence.get("response_frames") != 1
        or not isinstance(evidence.get("visible_text_chars"), int)
        or evidence["visible_text_chars"] <= 0
        or not isinstance(usage, dict)
        or usage.get("thinking_output_tokens") not in {0, "0"}
    ):
        return None
    return profile_id, leg


def load_resume_report(path: str, budget_nano: int, requested_models: list[str] | None) -> ResumeState:
    try:
        payload = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CalibrationError(f"cannot read Gemini resume report: {path}") from error
    if not isinstance(payload, dict) or payload.get("schema") not in {
        "gemini-live-calibration/v1",
        "gemini-live-calibration/v2",
    }:
        raise CalibrationError("resume report has an unsupported schema")
    if payload.get("complete") is True:
        raise CalibrationError("refusing to resume an already complete Gemini report")
    failure = payload.get("failure")
    legacy_explicit_stop = (
        isinstance(failure, str)
        and "returned HTTP 503:" in failure
        and "type.googleapis.com/google.rpc.RetryInfo" in failure
        and '"status":"UNAVAILABLE"' in failure.replace(" ", "")
    )
    legacy_stops = payload.get("profile_stops")
    legacy_checkpoint = (
        payload.get("schema") == "gemini-live-calibration/v1"
        and payload.get("resume_safe") is True
        and isinstance(legacy_stops, dict)
        and bool(legacy_stops)
        and all(
            isinstance(stop, str)
            and "returned HTTP 503:" in stop
            and "type.googleapis.com/google.rpc.RetryInfo" in stop
            and '"status":"UNAVAILABLE"' in stop.replace(" ", "")
            for stop in legacy_stops.values()
        )
    )
    proved_checkpoint = (
        payload.get("schema") == "gemini-live-calibration/v2"
        and payload.get("resume_safe") is True
        and payload.get("resume_proof") == "x-apitoken-execution-state:not_started"
    )
    minimal_reclassification = _minimal_thinking_reclassification(payload)
    if (
        not proved_checkpoint
        and not legacy_explicit_stop
        and not legacy_checkpoint
        and minimal_reclassification is None
    ):
        raise CalibrationError(
            "resume report is not proven safe; an ambiguous paid request must never be repeated"
        )
    if as_int(payload.get("budget_nanousd_total"), "resume budget") != budget_nano:
        raise CalibrationError("--budget-usd must exactly match the resumed aggregate budget")
    run_id = payload.get("run_id")
    if not isinstance(run_id, str) or not run_id.startswith("gemini-cal-") or len(run_id) > 96:
        raise CalibrationError("resume report has an invalid run id")
    profiles = _string_list(payload.get("profiles"), "profiles")
    models = _string_list(payload.get("models"), "models")
    if requested_models is not None and requested_models != models:
        raise CalibrationError("--models must be omitted or exactly match the resumed model order")

    raw_records = payload.get("records")
    raw_unavailable = payload.get("unavailable_capabilities")
    if not isinstance(raw_records, list) or not isinstance(raw_unavailable, list):
        raise CalibrationError("resume report has no completed outcome lists")
    records: list[dict[str, Any]] = []
    unavailable: list[dict[str, Any]] = []
    outcome_keys: set[tuple[str, str]] = set()
    unavailable_keys: set[tuple[str, str]] = set()
    request_ids: set[str] = set()
    calculated_by_profile: dict[str, int] = defaultdict(int)
    final_capacity = payload.get("final_capacity")
    final_events = (
        recent_turn_events(final_capacity)
        if isinstance(final_capacity, dict)
        and "calibration_recent_turn_limit" in final_capacity
        else {}
    )
    for raw in raw_records:
        if not isinstance(raw, dict):
            raise CalibrationError("resume report contains a non-object record")
        profile_id, model, leg, request_id = (
            raw.get("profile_id"), raw.get("model"), raw.get("leg"), raw.get("request_id")
        )
        if (
            profile_id not in profiles
            or model not in models
            or not isinstance(leg, str)
            or not leg
            or not isinstance(request_id, str)
            or not request_id
        ):
            raise CalibrationError("resume report record has an invalid identity")
        key = (profile_id, leg)
        if key in outcome_keys or request_id in request_ids:
            raise CalibrationError("resume report contains duplicate completed evidence")
        api_cost = raw.get("api_cost")
        usage = raw.get("usage")
        if not isinstance(api_cost, dict) or not isinstance(usage, dict):
            raise CalibrationError("resume report record has no exact usage/cost vector")
        parsed_cost = {
            field: as_int(api_cost.get(field), f"resume record api_cost.{field}")
            for field in EVENT_MONEY_FIELDS
        }
        for field in EVENT_TOKEN_FIELDS:
            as_int(usage.get(field), f"resume record usage.{field}")
        actual = as_int(raw.get("actual_nanousd"), "resume record actual_nanousd")
        if actual <= 0 or parsed_cost["api_total_nanousd"] != actual:
            raise CalibrationError("resume report record has an inconsistent actual cost")
        if sum(parsed_cost[field] for field in EVENT_MONEY_FIELDS[:-1]) != actual:
            raise CalibrationError("resume report record has a broken exact cost vector")
        schedule = raw.get("tariff_schedule_id")
        if not isinstance(schedule, str) or not schedule:
            legacy_event = final_events.get(request_id)
            schedule = legacy_event.get("tariff_schedule_id") if legacy_event else None
        if not isinstance(schedule, str) or not schedule:
            raise CalibrationError("resume report record has no exact tariff schedule identity")
        outcome_keys.add(key)
        request_ids.add(request_id)
        calculated_by_profile[profile_id] += actual
        parsed_record = dict(raw)
        parsed_record["tariff_schedule_id"] = schedule
        if minimal_reclassification == key:
            parsed_record["coverage_error"] = None
            evidence = dict(parsed_record["response_evidence"])
            evidence["minimal_zero_thinking_accepted"] = True
            parsed_record["response_evidence"] = evidence
        records.append(parsed_record)
    for raw in raw_unavailable:
        if not isinstance(raw, dict):
            raise CalibrationError("resume report contains a non-object unavailable capability")
        profile_id, model, capability = (
            raw.get("profile_id"), raw.get("model"), raw.get("capability")
        )
        if (
            profile_id not in profiles
            or model not in models
            or not isinstance(capability, str)
            or not capability
        ):
            raise CalibrationError("resume report unavailable capability has an invalid identity")
        key = (profile_id, capability)
        if key in unavailable_keys:
            raise CalibrationError("resume report contains duplicate unavailable outcomes")
        unavailable_keys.add(key)
        outcome_keys.add(key)
        if minimal_reclassification == key:
            continue
        unavailable.append(dict(raw))

    spent_nano = as_int(payload.get("spent_nanousd_total"), "resume spent total")
    if sum(calculated_by_profile.values()) != spent_nano:
        raise CalibrationError("resume report spend does not equal its immutable records")
    raw_by_profile = payload.get("spent_nanousd_per_profile")
    if not isinstance(raw_by_profile, dict):
        raise CalibrationError("resume report has no per-profile spend")
    spent_by_profile = {
        profile: as_int(raw_by_profile.get(profile, 0), f"resume spend {profile}")
        for profile in profiles
    }
    if spent_by_profile != {profile: calculated_by_profile.get(profile, 0) for profile in profiles}:
        raise CalibrationError("resume report per-profile spend does not equal its records")
    if spent_nano > budget_nano:
        raise CalibrationError("resume report already exceeds the aggregate budget")
    return ResumeState(
        run_id=run_id,
        profiles=profiles,
        models=models,
        records=records,
        unavailable=unavailable,
        spent_nano=spent_nano,
        spent_by_profile=spent_by_profile,
    )


def require_healthy_delivery(payload: dict[str, Any], require_empty: bool = True) -> None:
    if payload.get("calibration_authority_available") is not True:
        raise CalibrationError("Gemini exact calibration authority is unavailable")
    delivery = payload.get("calibration_delivery")
    if not isinstance(delivery, dict):
        raise CalibrationError("Gemini response has no calibration delivery diagnostics")
    pending = as_int(delivery.get("pending_events"), "calibration_delivery.pending_events")
    dropped = as_int(delivery.get("dropped_events"), "calibration_delivery.dropped_events")
    if dropped:
        raise CalibrationError(f"Gemini calibration delivery dropped {dropped} events")
    if delivery.get("persistence_ok") is not True:
        raise CalibrationError("Gemini calibration persistence is degraded")
    if require_empty and pending:
        raise CalibrationError(f"Gemini calibration still has {pending} pending events")
    if any(
        isinstance(profile, dict) and profile.get("calibration_persistence_ok") is False
        for profile in payload.get("profiles", [])
    ):
        raise CalibrationError("a Gemini profile has degraded calibration persistence")


def profile_state(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    states: dict[str, dict[str, Any]] = {}
    for raw in payload.get("profiles", []):
        if not isinstance(raw, dict):
            continue
        profile_id = raw.get("id")
        if not isinstance(profile_id, str) or not profile_id:
            continue
        windows = {
            str(window.get("window_kind")): window
            for window in raw.get("windows", [])
            if isinstance(window, dict)
        }
        plan = raw.get("plan")
        states[profile_id] = {
            "plan": plan.strip() if isinstance(plan, str) else "",
            "authenticated": raw.get("authenticated") is True,
            "cooling_until": as_int(raw.get("cooling_until", 0), "profile.cooling_until"),
            "persistence_ok": raw.get("calibration_persistence_ok") is True,
            "quota_updated_at": optional_int(
                raw.get("quota_updated_at"), "profile.quota_updated_at"
            ),
            "used_5h": optional_int(
                windows.get("5h", {}).get("used_fraction_units"), "profile.used_5h"
            ),
            "reset_5h": optional_int(
                windows.get("5h", {}).get("resets_at"), "profile.reset_5h"
            ),
            "used_7d": optional_int(
                windows.get("weekly", {}).get("used_fraction_units"), "profile.used_7d"
            ),
            "reset_7d": optional_int(
                windows.get("weekly", {}).get("resets_at"), "profile.reset_7d"
            ),
        }
    return states


def fraction_delta(before: dict[str, Any], after: dict[str, Any], field: str) -> int | None:
    left, right = before.get(field), after.get(field)
    reset_field = {"used_5h": "reset_5h", "used_7d": "reset_7d"}.get(field)
    if (
        left is None
        or right is None
        or reset_field is None
        or before.get(reset_field) is None
        or before.get(reset_field) != after.get(reset_field)
    ):
        return None
    delta = right - left
    return delta if delta >= 0 else None


def recent_turn_events(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    limit = as_int(payload.get("calibration_recent_turn_limit"), "recent turn limit")
    if limit < MIN_RECENT_TURN_LIMIT:
        raise CalibrationError(f"recent-turn window is too small: {limit}")
    raw_events = payload.get("calibration_recent_turns")
    if not isinstance(raw_events, list):
        raise CalibrationError("Gemini response has no immutable recent turns")
    events: dict[str, dict[str, Any]] = {}
    for raw in raw_events:
        if not isinstance(raw, dict):
            raise CalibrationError("recent Gemini turn is not an object")
        request_id = raw.get("request_id")
        profile_id = raw.get("profile_id")
        model = raw.get("model")
        if not all(isinstance(value, str) and value for value in (request_id, profile_id, model)):
            raise CalibrationError("recent Gemini turn has incomplete identity")
        if request_id in events:
            raise CalibrationError(f"duplicate immutable Gemini request id: {request_id}")
        parsed = dict(raw)
        for field in EVENT_TOKEN_FIELDS + EVENT_MONEY_FIELDS:
            if field not in raw:
                raise CalibrationError(
                    f"Gemini turn {request_id} is missing exact vector field {field}"
                )
            parsed[field] = as_int(raw[field], f"calibration_recent_turns.{field}")
        money_sum = sum(parsed[field] for field in EVENT_MONEY_FIELDS[:-1])
        if money_sum != parsed["api_total_nanousd"] or money_sum <= 0:
            raise CalibrationError(f"Gemini turn {request_id} has a broken exact cost vector")
        if parsed["cached_audio_input_tokens"] > parsed["cache_read_tokens"]:
            raise CalibrationError(f"Gemini turn {request_id} has impossible cached audio")
        if parsed["thinking_output_tokens"] > parsed["output_tokens"]:
            raise CalibrationError(f"Gemini turn {request_id} has impossible thinking output")
        if parsed["tool_prompt_tokens"] > parsed["input_tokens"]:
            raise CalibrationError(f"Gemini turn {request_id} has impossible tool prompt input")
        events[request_id] = parsed
    return events


def exact_new_turn(
    before_ids: set[str],
    payload: dict[str, Any],
    request_id: str,
    profile_id: str,
    model: str,
) -> dict[str, Any] | None:
    if request_id in before_ids:
        raise CalibrationError(f"Gemini calibration request id already existed: {request_id}")
    events = recent_turn_events(payload)
    event = events.get(request_id)
    if event is None:
        return None
    if event["profile_id"] != profile_id or event["model"] != model:
        raise CalibrationError(
            f"Gemini calibration request {request_id} was rebound to "
            f"{event['profile_id']}/{event['model']}"
        )
    return event


@dataclasses.dataclass(frozen=True)
class ModelRates:
    tariff_schedule_id: str
    input_token_limit: int
    input: int
    audio_input: int
    cached_input: int
    cached_audio_input: int
    output: int
    image_output: int
    long_threshold: int
    long_input: int
    long_audio_input: int
    long_cached_input: int
    long_cached_audio_input: int
    long_output: int
    search_unit: str
    search: int
    max_output_tokens: int

    def upper_bound(
        self,
        input_tokens: int,
        max_output_tokens: int,
        kind: str,
        image_size: str | None = None,
    ) -> int:
        if input_tokens > self.input_token_limit:
            raise UnboundedCostError(
                f"countTokens returned {input_tokens}, above model input limit "
                f"{self.input_token_limit}"
            )
        # Code Assist may prepend provider-owned instructions that countTokens does not report.
        # Live evidence has shown this even on ordinary cache legs, not only in
        # toolUsePromptTokenCount. The model's complete accepted input context is therefore the
        # only proved pre-dispatch ceiling for every paid generation request.
        bounded_input_tokens = self.input_token_limit
        long = bounded_input_tokens > self.long_threshold
        input_rates = (
            (self.long_input, self.long_audio_input, self.long_cached_input, self.long_cached_audio_input)
            if long
            else (self.input, self.audio_input, self.cached_input, self.cached_audio_input)
        )
        input_cost = bounded_input_tokens * max(input_rates)
        output_cost = max_output_tokens * (self.long_output if long else self.output)
        image_cost = 0
        if kind == "image":
            image_tokens = IMAGE_OUTPUT_TOKEN_CEILINGS.get(image_size or "")
            if image_tokens is None or self.image_output <= 0:
                raise UnboundedCostError(
                    f"image size {image_size!r} has no proved Gemini money ceiling"
                )
            image_cost = image_tokens * self.image_output
        search_cost = 0
        if kind == "search":
            if self.search_unit != "grounded_prompt":
                raise UnboundedCostError(
                    "per-query Gemini Search has no provider-documented request fanout ceiling"
                )
            search_cost = self.search
        return input_cost + output_cost + image_cost + search_cost


def rate_catalog(payload: dict[str, Any]) -> dict[str, ModelRates]:
    catalog: dict[str, ModelRates] = {}
    for raw in payload.get("conversion_models", []):
        if not isinstance(raw, dict):
            continue
        model = raw.get("id")
        rates = raw.get("rates")
        search = raw.get("search", {})
        if not isinstance(model, str) or not model or not isinstance(rates, dict):
            continue
        schedule_id = raw.get("tariff_schedule_id")
        if not isinstance(schedule_id, str) or not schedule_id:
            raise CalibrationError(f"{model} has no authoritative tariff schedule identity")
        catalog[model] = ModelRates(
            tariff_schedule_id=schedule_id,
            input_token_limit=as_int(raw.get("input_token_limit"), f"{model}.input_limit"),
            input=as_int(rates.get("input_nanousd_per_token"), f"{model}.input"),
            audio_input=as_int(rates.get("audio_input_nanousd_per_token"), f"{model}.audio"),
            cached_input=as_int(rates.get("cached_input_nanousd_per_token"), f"{model}.cache"),
            cached_audio_input=as_int(rates.get("cached_audio_input_nanousd_per_token"), f"{model}.cached_audio"),
            output=as_int(rates.get("output_nanousd_per_token"), f"{model}.output"),
            image_output=as_int(rates.get("image_output_nanousd_per_token"), f"{model}.image"),
            long_threshold=as_int(rates.get("long_context_threshold"), f"{model}.long_threshold"),
            long_input=as_int(rates.get("long_input_nanousd_per_token"), f"{model}.long_input"),
            long_audio_input=as_int(rates.get("long_audio_input_nanousd_per_token"), f"{model}.long_audio"),
            long_cached_input=as_int(rates.get("long_cached_input_nanousd_per_token"), f"{model}.long_cache"),
            long_cached_audio_input=as_int(rates.get("long_cached_audio_input_nanousd_per_token"), f"{model}.long_cached_audio"),
            long_output=as_int(rates.get("long_output_nanousd_per_token"), f"{model}.long_output"),
            search_unit=str(search.get("billing_unit", "")),
            search=as_int(search.get("nanousd_per_unit", 0), f"{model}.search"),
            max_output_tokens=as_int(raw.get("output_token_limit"), f"{model}.output_limit"),
        )
    if not catalog:
        raise CalibrationError("Gemini response has no exact conversion rate catalog")
    return catalog


@dataclasses.dataclass(frozen=True)
class Leg:
    name: str
    model: str
    kind: str
    thinking_level: str | None = None
    stream: bool = False
    cache_key: str | None = None
    cache_phase: str | None = None
    image_size: str | None = None
    max_output_tokens: int = 128


def thinking_levels(model: str) -> tuple[str | None, ...]:
    if model in {"gemini-3-flash-preview", "gemini-3.6-flash", "gemini-3.5-flash"}:
        return ("minimal", "low", "medium", "high")
    if model == "gemini-3.1-pro-preview":
        return ("low", "medium", "high")
    return (None,)


def build_coverage_legs(
    models: Iterable[str],
    run_id: str,
    rates: dict[str, ModelRates] | None = None,
) -> list[Leg]:
    legs: list[Leg] = []
    for model in models:
        for level in thinking_levels(model):
            suffix = level or "default"
            legs.append(Leg(f"thinking:{model}:{suffix}", model, "thinking", level, max_output_tokens=512))
        # Gemini 3 may spend a small output ceiling entirely on thinking and emit one terminal
        # frame. 256 proved enough for a genuinely incremental two-frame Flash response in the
        # owned route probe and remains inside the runner's exact aggregate bound.
        legs.append(Leg(f"sse:{model}", model, "fresh", stream=True, max_output_tokens=256))
        # A 128-token cache turn on Flash Preview exhausted its dynamic-thinking budget without
        # visible output, and the earlier audio probe used 119/128 output tokens. Keep both replay
        # pairs at 512 so the required visible answer is not a harness ceiling artifact; the full
        # input-context reserve still keeps the complete two-plan matrix below the approved cap.
        replay_output_tokens = 512 if model == "gemini-3-flash-preview" else 128
        cache_key = f"{run_id}:{model}:text-cache"
        legs.extend((
            Leg(
                f"cache-write:{model}",
                model,
                "cache",
                cache_key=cache_key,
                cache_phase="write",
                max_output_tokens=replay_output_tokens,
            ),
            Leg(
                f"cache-read:{model}",
                model,
                "cache",
                cache_key=cache_key,
                cache_phase="read",
                max_output_tokens=replay_output_tokens,
            ),
        ))
        audio_key = f"{run_id}:{model}:audio-cache"
        legs.extend((
            Leg(
                f"audio-fresh:{model}",
                model,
                "audio",
                cache_key=audio_key,
                cache_phase="write",
                max_output_tokens=replay_output_tokens,
            ),
            Leg(
                f"audio-replay:{model}",
                model,
                "audio",
                cache_key=audio_key,
                cache_phase="read",
                max_output_tokens=replay_output_tokens,
            ),
        ))
        legs.append(Leg(f"tool-prompt:{model}", model, "tool", max_output_tokens=256))
        legs.append(Leg(f"search:{model}", model, "search", max_output_tokens=256))
        model_rates = rates.get(model) if rates else None
        if model_rates is not None and model_rates.long_threshold < 1_000_000_000:
            legs.append(Leg(f"long-context:{model}", model, "long", max_output_tokens=128))
        if model == "gemini-3.1-flash-image":
            for size in ("1K", "2K", "4K"):
                legs.append(Leg(f"image-{size}:{model}", model, "image", image_size=size))
    return legs


def silent_wav_base64() -> str:
    sample_rate = 8_000
    pcm = b"\0\0" * (sample_rate // 4)
    header = b"RIFF" + struct.pack("<I", 36 + len(pcm)) + b"WAVEfmt "
    header += struct.pack("<IHHIIHH", 16, 1, 1, sample_rate, sample_rate * 2, 2, 16)
    header += b"data" + struct.pack("<I", len(pcm))
    return base64.b64encode(header + pcm).decode()


def profile_cache_scopes(profiles: Iterable[str]) -> dict[str, str]:
    return {
        profile_id: f"profile-{index}"
        for index, profile_id in enumerate(profiles, start=1)
    }


def coverage_schedule(legs: Iterable[Leg], profiles: Iterable[str]) -> list[tuple[str, Leg]]:
    ordered_legs = list(legs)
    ordered_profiles = list(profiles)
    schedule: list[tuple[str, Leg]] = []
    index = 0
    while index < len(ordered_legs):
        write = ordered_legs[index]
        read = ordered_legs[index + 1] if index + 1 < len(ordered_legs) else None
        is_replay_pair = (
            read is not None
            and write.cache_key is not None
            and write.cache_key == read.cache_key
            and write.model == read.model
            and write.kind == read.kind
            and write.cache_phase == "write"
            and read.cache_phase == "read"
        )
        if is_replay_pair:
            # Implicit provider cache admission is time-sensitive. Keep the byte-identical replay
            # immediately behind its own profile's write instead of interposing another profile's
            # generation plus the mandatory immutable-evidence propagation wait. A missing cache
            # class on this adjacent read still fails closed; this changes ordering, not proof.
            for profile in ordered_profiles:
                schedule.append((profile, write))
                schedule.append((profile, read))
            index += 2
            continue
        schedule.extend((profile, write) for profile in ordered_profiles)
        index += 1
    return schedule


def body_for_leg(
    leg: Leg,
    run_id: str,
    cache_scope: str | None = None,
) -> dict[str, Any]:
    shared = leg.cache_key or f"{run_id}:{leg.name}"
    if leg.cache_key and cache_scope:
        shared = f"{shared}:{cache_scope}"
    text = f"Calibration {shared}. Reply with exactly CALIBRATION_OK."
    parts: list[dict[str, Any]] = [{"text": text}]
    if leg.kind == "cache":
        parts[0]["text"] = f"{text}\n" + ("stable calibration context " * 4_096)
    if leg.kind == "audio":
        parts = [
            {"inlineData": {"mimeType": "audio/wav", "data": silent_wav_base64()}},
            {"text": f"Calibration {shared}. State whether the clip is silent in one word."},
        ]
    if leg.kind == "search":
        parts = [{"text": "Use Google Search to report the current UTC date. Cite one source."}]
    if leg.kind == "image":
        parts = [{"text": f"Create a minimal blue circle on a white background. Run {run_id}."}]
    if leg.kind == "thinking":
        parts = [{"text": "Compute 137 * 149 step by step, then return only the integer."}]
    if leg.kind == "tool":
        parts = [{
            "text": (
                "Call calibration_probe exactly once with marker CALIBRATION_OK. "
                "Do not answer with plain text."
            )
        }]
    if leg.kind == "long":
        parts = [{
            "text": ("x " * 220_000)
            + f"\nCalibration {shared}. Reply with exactly CALIBRATION_OK."
        }]
    generation: dict[str, Any] = {"maxOutputTokens": leg.max_output_tokens}
    if leg.thinking_level:
        generation["thinkingConfig"] = {"thinkingLevel": leg.thinking_level}
    if leg.kind == "image":
        generation.update({
            "responseModalities": ["IMAGE"],
            "imageConfig": {"imageSize": leg.image_size},
        })
    body: dict[str, Any] = {
        "contents": [{"role": "user", "parts": parts}],
        "generationConfig": generation,
    }
    if leg.kind == "search":
        body["tools"] = [{"googleSearch": {}}]
    if leg.kind == "tool":
        body["tools"] = [{
            "functionDeclarations": [{
                "name": "calibration_probe",
                "description": "Return the supplied calibration marker.",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {"marker": {"type": "STRING"}},
                    "required": ["marker"],
                },
            }]
        }]
        body["toolConfig"] = {
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": ["calibration_probe"],
            }
        }
    return body


def count_body(body: dict[str, Any]) -> dict[str, Any]:
    return {key: body[key] for key in ("contents", "systemInstruction", "tools") if key in body}


def verify_leg_usage(leg: Leg, event: dict[str, Any]) -> str | None:
    if leg.kind != "image" and event["output_tokens"] <= 0:
        return "output token class was not observed"
    if leg.kind == "audio" and event["audio_input_tokens"] <= 0:
        return "audio input token class was not observed"
    if leg.kind == "cache" and leg.cache_phase == "read" and event["cache_read_tokens"] <= 0:
        return "cached input token class was not observed"
    if (
        leg.kind == "thinking"
        and leg.thinking_level
        and leg.thinking_level != "minimal"
        and event["thinking_output_tokens"] <= 0
    ):
        return THINKING_TOKENS_NOT_OBSERVED
    # `tool_prompt_tokens` is an optional subset diagnostic, not a separately priced leg.
    # Antigravity can return a forced functionCall with exact terminal usage while folding the
    # declaration into ordinary promptTokenCount. The functionCall proof in
    # verify_generation_response plus response/event usage parity proves the control without
    # inventing a subset split; the full fresh-input count remains billed exactly once.
    if leg.kind == "search" and event["search_queries"] + event["grounded_search_prompts"] <= 0:
        return "search billing unit was not observed"
    if leg.kind == "image" and event["image_output_tokens"] <= 0:
        return "image output token class was not observed"
    return None


def _decode_sse_frames(raw: bytes) -> tuple[dict[str, Any], ...]:
    try:
        text = raw.decode("utf-8", "strict")
    except UnicodeDecodeError as error:
        raise CalibrationError("Gemini SSE response is not UTF-8") from error
    frames: list[dict[str, Any]] = []
    data_lines: list[str] = []
    for line in text.splitlines() + [""]:
        if not line:
            if not data_lines:
                continue
            data = "\n".join(data_lines)
            data_lines = []
            if data == "[DONE]":
                continue
            try:
                frame = json.loads(data)
            except json.JSONDecodeError as error:
                raise CalibrationError("Gemini SSE frame contains invalid JSON") from error
            if not isinstance(frame, dict):
                raise CalibrationError("Gemini SSE frame is not an object")
            frames.append(frame)
            continue
        if line.startswith("data:"):
            data_lines.append(line[5:].lstrip())
        elif line.startswith(("event:", "id:", "retry:", ":")):
            continue
        else:
            raise CalibrationError("Gemini SSE response contains an invalid field")
    if not frames:
        raise CalibrationError("Gemini SSE response contains no JSON frames")
    return tuple(frames)


def decode_generation_response(raw: bytes, stream: bool) -> GenerationResponse:
    """Decode native JSON/SSE without retaining raw output in the persisted report."""

    try:
        if stream:
            try:
                payload = json.loads(raw)
            except (json.JSONDecodeError, UnicodeDecodeError):
                frames = _decode_sse_frames(raw)
            else:
                if isinstance(payload, list) and payload and all(
                    isinstance(frame, dict) for frame in payload
                ):
                    frames = tuple(payload)
                elif isinstance(payload, dict):
                    frames = (payload,)
                else:
                    raise CalibrationError(
                        "Gemini streaming response is not an object or non-empty object array"
                    )
        else:
            payload = json.loads(raw)
            if not isinstance(payload, dict):
                raise CalibrationError("Gemini generation response is not an object")
            frames = (payload,)
        return GenerationResponse(frames=frames, stream=stream)
    except (CalibrationError, json.JSONDecodeError, UnicodeDecodeError) as error:
        return GenerationResponse(
            frames=(),
            stream=stream,
            parse_error=str(error) or "Gemini generation response could not be decoded",
        )


def _response_int(value: Any, field: str, default: int | None = None) -> int:
    if value is None and default is not None:
        return default
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise CalibrationError(f"response {field} is not a non-negative integer")
    return value


def _modality_tokens(metadata: dict[str, Any], field: str, modality: str) -> int:
    details = metadata.get(field, [])
    if details is None:
        return 0
    if not isinstance(details, list):
        raise CalibrationError(f"response usageMetadata.{field} is not an array")
    total = 0
    for detail in details:
        if not isinstance(detail, dict):
            raise CalibrationError(f"response usageMetadata.{field} has a non-object item")
        if str(detail.get("modality", "")).upper() == modality:
            total += _response_int(
                detail.get("tokenCount"), f"usageMetadata.{field}.tokenCount"
            )
    return total


def _response_usage_vector(
    metadata: Any,
    leg: Leg,
    image_delivered: bool,
) -> dict[str, int]:
    if not isinstance(metadata, dict):
        raise CalibrationError("terminal response has no usageMetadata object")
    prompt = _response_int(metadata.get("promptTokenCount"), "usageMetadata.promptTokenCount")
    candidates = _response_int(
        metadata.get("candidatesTokenCount"), "usageMetadata.candidatesTokenCount", 0
    )
    thoughts = _response_int(
        metadata.get("thoughtsTokenCount"), "usageMetadata.thoughtsTokenCount", 0
    )
    cached = min(
        _response_int(
            metadata.get("cachedContentTokenCount"),
            "usageMetadata.cachedContentTokenCount",
            0,
        ),
        prompt,
    )
    tool_prompt = _response_int(
        metadata.get("toolUsePromptTokenCount"),
        "usageMetadata.toolUsePromptTokenCount",
        0,
    )
    audio_prompt = min(_modality_tokens(metadata, "promptTokensDetails", "AUDIO"), prompt)
    cached_audio = min(
        _modality_tokens(metadata, "cacheTokensDetails", "AUDIO"),
        cached,
        audio_prompt,
    )
    audio_input = audio_prompt - cached_audio
    uncached_input = max(prompt - cached - audio_input, 0) + tool_prompt
    image_output = min(
        _modality_tokens(metadata, "candidatesTokensDetails", "IMAGE"), candidates
    )
    output = max(candidates - image_output, 0) + thoughts
    if leg.kind == "image" and image_delivered and image_output == 0:
        image_output = IMAGE_OUTPUT_TOKEN_CEILINGS.get(leg.image_size or "", 0)
        output = max(output - image_output, 0)
    vector = {
        "input_tokens": uncached_input,
        "audio_input_tokens": audio_input,
        "cache_read_tokens": cached,
        "cached_audio_input_tokens": cached_audio,
        "output_tokens": output,
        "thinking_output_tokens": thoughts,
        "image_output_tokens": image_output,
        "tool_prompt_tokens": min(tool_prompt, uncached_input),
    }
    if prompt <= 0 or vector["output_tokens"] + vector["image_output_tokens"] <= 0:
        raise CalibrationError("terminal response usage has no positive input/output tokens")
    return vector


def verify_generation_response(
    leg: Leg,
    response: GenerationResponse,
    event: dict[str, Any],
) -> tuple[dict[str, Any], str | None]:
    """Return sanitized response proof and a fail-closed coverage error, if any."""

    evidence: dict[str, Any] = {
        "response_frames": len(response.frames),
        "candidate_frames": 0,
        "visible_text_chars": 0,
        "function_calls": 0,
        "inline_data_parts": 0,
        "terminal_finish": False,
        "terminal_usage": False,
        "incremental_sse": False,
        "model_version": None,
        "usage_matches_immutable_event": False,
    }
    if response.parse_error:
        return evidence, response.parse_error
    if response.stream != leg.stream or not response.frames:
        return evidence, "generation response transport does not match the requested mode"

    model_versions: set[str] = set()
    usage_indexes: list[int] = []
    malformed: str | None = None
    for index, frame in enumerate(response.frames):
        if "error" in frame:
            malformed = "successful generation body contains a provider error frame"
            break
        model_version = frame.get("modelVersion")
        if model_version is not None:
            if not isinstance(model_version, str) or not model_version:
                malformed = "generation response has an invalid modelVersion"
                break
            model_versions.add(model_version)
        if "usageMetadata" in frame:
            usage_indexes.append(index)
        candidates = frame.get("candidates", [])
        if candidates is None:
            candidates = []
        if not isinstance(candidates, list):
            malformed = "generation response candidates is not an array"
            break
        if candidates:
            evidence["candidate_frames"] += 1
        for candidate in candidates:
            if not isinstance(candidate, dict):
                malformed = "generation response has a non-object candidate"
                break
            finish = candidate.get("finishReason")
            if isinstance(finish, str) and finish:
                evidence["terminal_finish"] = True
            content = candidate.get("content", {})
            if content is None:
                content = {}
            if not isinstance(content, dict):
                malformed = "generation candidate content is not an object"
                break
            parts = content.get("parts", [])
            if parts is None:
                parts = []
            if not isinstance(parts, list):
                malformed = "generation candidate parts is not an array"
                break
            for part in parts:
                if not isinstance(part, dict):
                    malformed = "generation response has a non-object part"
                    break
                text_value = part.get("text")
                if (
                    isinstance(text_value, str)
                    and text_value.strip()
                    and part.get("thought") is not True
                ):
                    evidence["visible_text_chars"] += len(text_value.strip())
                function_call = part.get("functionCall")
                if (
                    isinstance(function_call, dict)
                    and isinstance(function_call.get("name"), str)
                    and function_call["name"]
                ):
                    evidence["function_calls"] += 1
                inline = part.get("inlineData", part.get("inline_data"))
                if (
                    isinstance(inline, dict)
                    and isinstance(inline.get("data"), str)
                    and inline["data"]
                ):
                    evidence["inline_data_parts"] += 1
            if malformed:
                break
        if malformed:
            break
    if malformed:
        return evidence, malformed
    if model_versions != {leg.model}:
        return evidence, (
            f"generation modelVersion proof is {sorted(model_versions)!r}, expected {leg.model!r}"
        )
    evidence["model_version"] = leg.model
    if not evidence["terminal_finish"]:
        return evidence, "generation response has no terminal finishReason"
    if not usage_indexes or usage_indexes[-1] != len(response.frames) - 1:
        return evidence, "generation response has no terminal usageMetadata"
    evidence["terminal_usage"] = True
    if leg.stream:
        evidence["incremental_sse"] = (
            len(response.frames) >= 2 and evidence["candidate_frames"] >= 2
        )
        if not evidence["incremental_sse"]:
            return evidence, "SSE response did not contain multiple incremental candidate frames"
    if leg.kind == "tool":
        if evidence["function_calls"] <= 0:
            return evidence, "tool control returned no functionCall"
    elif leg.kind == "image":
        if evidence["inline_data_parts"] <= 0:
            return evidence, "image control returned no inlineData"
    elif evidence["visible_text_chars"] <= 0:
        return evidence, "generation returned no visible non-thought text"
    try:
        response_usage = _response_usage_vector(
            response.frames[-1].get("usageMetadata"),
            leg,
            evidence["inline_data_parts"] > 0,
        )
    except CalibrationError as error:
        return evidence, str(error)
    mismatched = {
        field: (response_usage[field], event[field])
        for field in response_usage
        if response_usage[field] != event[field]
    }
    if mismatched:
        return evidence, f"terminal response usage does not match immutable event: {mismatched}"
    evidence["usage_matches_immutable_event"] = True
    return evidence, None


@dataclasses.dataclass
class Budget:
    limit_nano: int
    total_nano: int = 0
    by_profile: dict[str, int] = dataclasses.field(default_factory=lambda: defaultdict(int))

    def require(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if self.total_nano + upper_bound_nano > self.limit_nano:
            raise CalibrationError("aggregate Gemini budget guard stopped before dispatch")

    def charge(self, profile_id: str, actual_nano: int, upper_bound_nano: int) -> None:
        if actual_nano <= 0 or actual_nano > upper_bound_nano:
            raise CalibrationError("Gemini backend evidence violated the preflight cost bound")
        if self.total_nano + actual_nano > self.limit_nano:
            raise CalibrationError("Gemini backend evidence exceeded the global live budget")
        self.total_nano += actual_nano
        self.by_profile[profile_id] += actual_nano


class JsonHttpClient:
    def __init__(self, api_url: str, api_key: str, timeout: int) -> None:
        self.api_url = api_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def request(self, path: str, method: str = "GET", body: dict[str, Any] | None = None,
                target_profile: str | None = None, raw_ok: bool = False,
                calibration_request_id: str | None = None) -> Any:
        data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
        headers = {"x-goog-api-key": self.api_key, "content-type": "application/json", "accept": "application/json"}
        if target_profile:
            headers["x-apitoken-calibration-profile"] = target_profile
        if calibration_request_id:
            headers["x-apitoken-calibration-request-id"] = calibration_request_id
        request = urllib.request.Request(f"{self.api_url}{path}", data=data, headers=headers, method=method)
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
        generation = path.endswith(":generateContent") or ":streamGenerateContent" in path
        if generation:
            return decode_generation_response(raw, ":streamGenerateContent" in path)
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
    def __init__(
        self,
        timeout: int,
        ssh_target: str = DEFAULT_PRODUCTION_SSH_TARGET,
        api_port: int = DEFAULT_PRODUCTION_API_PORT,
    ) -> None:
        self.timeout = timeout
        self.ssh_target = validate_production_ssh_target(ssh_target)
        self.api_port = validate_production_api_port(api_port)

    def request(self, path: str, method: str = "GET", body: dict[str, Any] | None = None,
                target_profile: str | None = None, raw_ok: bool = False,
                calibration_request_id: str | None = None) -> Any:
        if method not in {"GET", "POST"} or not path.startswith("/v1beta/"):
            raise CalibrationError(f"unsupported Gemini SSH request: {method} {path}")
        if any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/_-?=&.:" for char in path):
            raise CalibrationError(f"unsafe Gemini SSH path: {path!r}")
        headers = ["content-type: application/json", "accept: application/json"]
        if target_profile:
            if not (1 <= len(target_profile) <= 128) or not all(
                char.isascii() and (char.isalnum() or char in "._-") for char in target_profile
            ):
                raise CalibrationError(f"invalid exact Gemini profile id: {target_profile!r}")
            headers.append(f"x-apitoken-calibration-profile: {target_profile}")
        if calibration_request_id:
            if len(calibration_request_id) != 36 or any(
                char not in "0123456789abcdef-" for char in calibration_request_id
            ):
                raise CalibrationError(
                    f"invalid exact Gemini calibration request id: {calibration_request_id!r}"
                )
            headers.append(f"x-apitoken-calibration-request-id: {calibration_request_id}")
        header_args = " ".join(f"-H {shlex.quote(header)}" for header in headers)
        data_arg = "--data-binary @-" if body is not None else ""
        remote = (
            "set -a && . /srv/claude-api/data/server.env && set +a && "
            "calibration_key=${CLAUDE_API_KEYS%%,*} && test -n \"$calibration_key\" && "
            f"curl -sS --max-time {self.timeout} "
            "-w '\\n__CALIBRATION_HTTP__%{http_code}\\n"
            "%header{x-apitoken-execution-state}' "
            f"-X {method} "
            f"-H \"x-goog-api-key: $calibration_key\" {header_args} {data_arg} "
            f"{shlex.quote(f'http://127.0.0.1:{self.api_port}' + path)}"
        )
        data = b"" if body is None else json.dumps(body, separators=(",", ":")).encode()
        safe = method == "GET" or path.endswith(":countTokens")
        attempts = SAFE_READ_ATTEMPTS if safe else 1
        result = None
        for attempt in range(attempts):
            result = subprocess.run(["ssh", self.ssh_target, remote], input=data, capture_output=True,
                                    timeout=self.timeout + 30, check=False)
            if result.returncode == 0:
                break
            if attempt + 1 == attempts:
                raise CalibrationError(f"{path} SSH transport failed: {result.stderr[-800:].decode(errors='replace')}")
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
        generation = path.endswith(":generateContent") or ":streamGenerateContent" in path
        if generation:
            return decode_generation_response(raw, ":streamGenerateContent" in path)
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload


class CapacityReader:
    def __init__(self, command: str | None, url: str | None, panel_key: str | None, timeout: int) -> None:
        self.command = shlex.split(command) if command else None
        self.url = url
        self.panel_key = panel_key
        self.timeout = timeout
        if not self.command and not self.url:
            raise CalibrationError("set --capacity-command or --capacity-url")
        if self.url and not self.panel_key:
            raise CalibrationError("panel key is required with --capacity-url")

    def read(self) -> dict[str, Any]:
        if self.command:
            result = subprocess.run(self.command, capture_output=True, timeout=self.timeout, check=False)
            if result.returncode:
                raise CalibrationError(f"capacity command failed: {result.stderr[-500:].decode(errors='replace')}")
            raw = result.stdout
        else:
            request = urllib.request.Request(self.url or "", headers={"x-api-key": self.panel_key or ""})
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


class Runner:
    def __init__(self, api: Any, capacity: CapacityReader, rates: dict[str, ModelRates], budget: Budget,
                 timeout: int, delay: float, run_id: str, cache_scopes: dict[str, str]) -> None:
        self.api = api
        self.capacity = capacity
        self.rates = rates
        self.budget = budget
        self.timeout = timeout
        self.delay = delay
        self.run_id = run_id
        self.cache_scopes = cache_scopes
        self.records: list[dict[str, Any]] = []

    def execute_leg(self, leg: Leg, profile_id: str) -> dict[str, Any]:
        before = self.capacity.read()
        require_healthy_delivery(before)
        states = profile_state(before)
        state = states.get(profile_id)
        if not state or not state["authenticated"] or state["cooling_until"] > int(time.time()):
            raise CalibrationError(f"target Gemini profile became unavailable: {profile_id}")
        cache_scope = self.cache_scopes.get(profile_id)
        if not cache_scope:
            raise CalibrationError("target Gemini profile has no stable cache scope")
        before_ids = set(recent_turn_events(before))
        body = body_for_leg(
            leg,
            self.run_id,
            cache_scope,
        )
        model_path = urllib.parse.quote(leg.model, safe="-._")
        counted = self.api.request(
            f"/v1beta/models/{model_path}:countTokens", "POST", count_body(body), profile_id
        )
        input_tokens = as_int(counted.get("totalTokens"), f"{leg.name}.countTokens")
        rates = self.rates[leg.model]
        if leg.kind == "long" and input_tokens <= rates.long_threshold:
            raise UnboundedCostError(
                f"countTokens returned {input_tokens}, not above long-context threshold "
                f"{rates.long_threshold}"
            )
        upper = rates.upper_bound(
            input_tokens,
            leg.max_output_tokens,
            leg.kind,
            leg.image_size,
        )
        self.budget.require(upper)
        suffix = "streamGenerateContent?alt=sse" if leg.stream else "generateContent"
        calibration_request_id = str(uuid.uuid4())
        if calibration_request_id in before_ids:
            raise CalibrationError("generated Gemini calibration request id already exists")
        generation_response = self.api.request(
            f"/v1beta/models/{model_path}:{suffix}",
            "POST",
            body,
            profile_id,
            raw_ok=leg.stream,
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
                leg.model,
            )
            if event is not None and observed.get("calibration_delivery", {}).get("pending_events") == 0:
                break
        if event is None:
            raise CalibrationError(f"{leg.name}: exact immutable Gemini event did not appear")
        if event.get("tariff_schedule_id") != rates.tariff_schedule_id:
            raise CalibrationError(
                f"{leg.name}: immutable event tariff {event.get('tariff_schedule_id')!r} "
                f"does not match preflight {rates.tariff_schedule_id!r}"
            )
        actual = event["api_total_nanousd"]
        self.budget.charge(profile_id, actual, upper)
        if not isinstance(generation_response, GenerationResponse):
            raise CalibrationError(
                f"{leg.name}: generation client returned no verifiable response envelope"
            )
        response_evidence, response_error = verify_generation_response(
            leg, generation_response, event
        )
        completed_at = as_int(event.get("completed_at"), f"{leg.name}.completed_at")
        if self.delay > 0:
            time.sleep(self.delay)
        quota_deadline = time.monotonic() + self.timeout
        quota_snapshot_resolved = False
        while True:
            observed = self.capacity.read()
            require_healthy_delivery(observed)
            after_state = profile_state(observed).get(profile_id, {})
            quota_updated_at = after_state.get("quota_updated_at")
            if quota_updated_at is not None and quota_updated_at >= completed_at:
                quota_snapshot_resolved = True
                break
            if time.monotonic() >= quota_deadline:
                break
            time.sleep(2)
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
            "kind": leg.kind,
            "model": leg.model,
            "thinking_level": leg.thinking_level,
            "stream": leg.stream,
            "image_size": leg.image_size,
            "request_id": event["request_id"],
            "tariff_schedule_id": event["tariff_schedule_id"],
            "counted_input_tokens": str(input_tokens),
            "upper_bound_nanousd": str(upper),
            "actual_nanousd": str(actual),
            "fraction_delta_5h": fraction_delta(state, after_state, "used_5h"),
            "fraction_delta_7d": fraction_delta(state, after_state, "used_7d"),
            "profitability_eligible": profitability_eligible,
            "quota_snapshot_resolved": quota_snapshot_resolved,
            "concurrent_profile_request_ids": concurrent_profile_request_ids,
            "before_windows": {
                "5h": {"used_fraction_units": state.get("used_5h"), "resets_at": state.get("reset_5h")},
                "7d": {"used_fraction_units": state.get("used_7d"), "resets_at": state.get("reset_7d")},
            },
            "after_windows": {
                "5h": {"used_fraction_units": after_state.get("used_5h"), "resets_at": after_state.get("reset_5h")},
                "7d": {"used_fraction_units": after_state.get("used_7d"), "resets_at": after_state.get("reset_7d")},
            },
            "response_evidence": response_evidence,
            "coverage_error": response_error or verify_leg_usage(leg, event),
            "usage": {field: str(event[field]) for field in EVENT_TOKEN_FIELDS},
            "api_cost": {field: str(event[field]) for field in EVENT_MONEY_FIELDS},
        }
        self.records.append(record)
        print(f"{profile_id} {leg.name}: ${actual / NANO_PER_USD:.6f}", flush=True)
        return record


def model_profitability(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str, str], dict[str, int]] = defaultdict(
        lambda: {"nano": 0, "fraction": 0, "turns": 0}
    )
    for record in records:
        if (
            record.get("profitability_eligible") is not True
            or record.get("quota_snapshot_resolved") is not True
        ):
            continue
        for window, field in (("5h", "fraction_delta_5h"), ("7d", "fraction_delta_7d")):
            delta = record.get(field)
            if delta is None or int(delta) <= 0:
                continue
            key = (record["plan"], record["model"], record["kind"], window)
            grouped[key]["nano"] += int(record["actual_nanousd"])
            grouped[key]["fraction"] += int(delta)
            grouped[key]["turns"] += 1
    rows = []
    for (plan, model, kind, window), value in grouped.items():
        per_one_percent = value["nano"] * 1_000_000 // value["fraction"]
        rows.append({
            "plan": plan,
            "model": model,
            "token_class": kind,
            "window": window,
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
    api_port: int = DEFAULT_PRODUCTION_API_PORT,
) -> str:
    ssh_target = validate_production_ssh_target(ssh_target)
    api_port = validate_production_api_port(api_port)
    return (
        f"ssh {shlex.quote(ssh_target)} 'set -a; . /srv/claude-api/data/server.env; set +a; "
        'curl -fsS -H "x-api-key: $CLAUDE_API_PANEL_KEY" '
        f"http://127.0.0.1:{api_port}/gemini-subs'"
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--api-url", default="https://gemini.api.apitoken.sale")
    parser.add_argument("--api-key-env", default="APITOKEN_API_KEY")
    parser.add_argument("--capacity-command", default=os.getenv("GEMINI_CALIBRATION_CAPACITY_COMMAND"))
    parser.add_argument("--capacity-url")
    parser.add_argument("--panel-key-env", default="CLAUDE_API_PANEL_KEY")
    parser.add_argument("--budget-usd", default="40")
    parser.add_argument("--models", nargs="*")
    parser.add_argument("--evidence-timeout", type=int, default=DEFAULT_EVIDENCE_TIMEOUT_SECONDS)
    parser.add_argument("--profile-delay", type=float, default=DEFAULT_PROFILE_DELAY_SECONDS)
    parser.add_argument("--http-timeout", type=int, default=240)
    parser.add_argument("--report", default="/tmp/gemini-calibration-report.json")
    parser.add_argument("--resume-report")
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
        validate_production_ssh_target(args.production_ssh_target)
        validate_production_api_port(args.production_capacity_port)
        validate_production_api_port(args.production_api_port)
    except CalibrationError as error:
        parser.error(str(error))
    return args


def dry_run_plan(args: argparse.Namespace, budget_nano: int) -> dict[str, Any]:
    return {
        "schema": "gemini-live-calibration-plan/v1",
        "mode": "dry-run",
        "paid_requests": 0,
        "budget_nanousd_total": str(budget_nano),
        "models": args.models or ["<all backend conversion_models>"],
        "production_capacity_port": args.production_capacity_port,
        "production_api_port": args.production_api_port,
        "coverage": [
            "fresh",
            "sse",
            "thinking-levels",
            "cache-write/read",
            "audio-write/read",
            "function-tool-prompt",
            "google-search-when-hard-bounded",
            "long-context",
            "image-1K/2K/4K",
        ],
        "guards": [
            "exact-profile-target",
            "uuidv4-request-attribution",
            "healthy-authority-and-empty-fifo",
            "countTokens-plus-official-rate-card",
            "full-input-context-ceiling-for-hidden-provider-prompts",
            "single-aggregate-budget",
            "no-paid-request-retry",
            "resume-only-from-not-started-or-completed-turn-proof",
            "public-modelVersion-and-real-output",
            "terminal-response-usage-equals-immutable-event",
            "multiple-incremental-sse-candidate-frames",
            "forced-control-output",
        ],
        "execute_requires": "--execute plus a capacity source and production/admin API access",
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    budget_nano = usd_to_nano(args.budget_usd)
    if budget_nano <= 0 or budget_nano > MAX_BUDGET_NANO:
        raise CalibrationError("--budget-usd must be positive and no greater than 40")
    if not args.execute:
        print(json.dumps(dry_run_plan(args, budget_nano), ensure_ascii=False, indent=2))
        return 0
    resume = (
        load_resume_report(args.resume_report, budget_nano, args.models)
        if args.resume_report
        else None
    )
    api_key = os.getenv(args.api_key_env, "")
    if not args.production_api_over_ssh and not api_key:
        raise CalibrationError(f"missing API key environment variable: {args.api_key_env}")
    capacity = CapacityReader(
        remote_capacity_command(args.production_ssh_target, args.production_capacity_port)
        if args.production_capacity_over_ssh
        else args.capacity_command,
        args.capacity_url,
        os.getenv(args.panel_key_env),
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
    states = profile_state(baseline)
    now = int(time.time())
    healthy_profiles = sorted(
        profile
        for profile, state in states.items()
        if state["authenticated"]
        and state["cooling_until"] <= now
        and state["persistence_ok"]
    )
    profiles = resume.profiles if resume else healthy_profiles
    if not profiles:
        raise CalibrationError("no healthy exact-target Gemini profiles")
    missing_plan = [
        profile for profile in profiles if profile in states and not states[profile]["plan"]
    ]
    if missing_plan:
        raise CalibrationError("Gemini profiles have no authoritative paid plan: " + ", ".join(missing_plan))
    rates = rate_catalog(baseline)
    models = resume.models if resume else (args.models or sorted(rates))
    unknown = sorted(set(models) - set(rates))
    if unknown:
        raise CalibrationError("models have no authoritative Gemini rate card: " + ", ".join(unknown))
    if resume:
        drifted_records = [
            record
            for record in resume.records
            if record["tariff_schedule_id"] != rates[record["model"]].tariff_schedule_id
            or (
                record["profile_id"] in states
                and states[record["profile_id"]]["plan"]
                and record.get("plan") != states[record["profile_id"]]["plan"]
            )
        ]
        if drifted_records:
            raise CalibrationError(
                "resume report crossed a paid-plan or tariff-schedule identity cutover"
            )
    run_id = resume.run_id if resume else f"gemini-cal-{int(time.time())}-{uuid.uuid4().hex[:8]}"
    budget = Budget(
        budget_nano,
        resume.spent_nano if resume else 0,
        defaultdict(int, resume.spent_by_profile if resume else {}),
    )
    runner = Runner(
        api,
        capacity,
        rates,
        budget,
        args.evidence_timeout,
        args.profile_delay,
        run_id,
        profile_cache_scopes(profiles),
    )
    runner.records = list(resume.records) if resume else []
    unavailable: list[dict[str, Any]] = list(resume.unavailable) if resume else []
    stopped: dict[str, str] = {
        profile: "target profile is not currently authenticated or is cooling"
        for profile in profiles
        if profile not in healthy_profiles
    }
    legs = build_coverage_legs(models, run_id, rates)
    expected = {(profile, leg.name): leg for leg in legs for profile in profiles}
    completed = {
        (record["profile_id"], record["leg"])
        for record in runner.records
    } | {
        (item["profile_id"], item["capability"])
        for item in unavailable
    }
    unknown_completed = sorted(completed - set(expected))
    if unknown_completed:
        raise CalibrationError(
            "resume report outcomes do not match the current coverage matrix: "
            + ", ".join(f"{profile}/{leg}" for profile, leg in unknown_completed)
        )
    failure: str | None = None
    try:
        for profile, leg in coverage_schedule(legs, profiles):
            key = (profile, leg.name)
            if profile in stopped or key in completed:
                continue
            try:
                record = runner.execute_leg(leg, profile)
                completed.add(key)
                if record["coverage_error"]:
                    unavailable.append({
                        "profile_id": profile,
                        "model": leg.model,
                        "capability": leg.name,
                        "reason": record["coverage_error"],
                        "blocking": True,
                    })
                    completed.add(key)
                    raise CalibrationError(
                        f"{profile}/{leg.name}: paid response proof failed: "
                        f"{record['coverage_error']}"
                    )
            except UnboundedCostError as error:
                unavailable.append({
                    "profile_id": profile,
                    "model": leg.model,
                    "capability": leg.name,
                    "reason": str(error),
                    "blocking": False,
                    "skipped_before_dispatch": True,
                })
                completed.add(key)
                continue
            except HttpCalibrationError as error:
                if error.status in {400, 403, 404}:
                    unavailable.append({
                        "profile_id": profile,
                        "model": leg.model,
                        "capability": leg.name,
                        "http_status": error.status,
                        "reason": error.detail[:300],
                        "blocking": True,
                    })
                    completed.add(key)
                    raise CalibrationError(
                        f"{profile}/{leg.name}: required generation capability returned "
                        f"HTTP {error.status}"
                    )
                if is_explicit_transient_stop(error):
                    stopped[profile] = str(error)
                    continue
                raise
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        failure = str(error)
    try:
        final = capacity.read()
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        final = baseline
        failure = failure or f"final Gemini capacity read failed: {error}"
    pending = [
        {
            "profile_id": profile,
            "model": leg.model,
            "capability": leg.name,
        }
        for (profile, _), leg in expected.items()
        if (profile, leg.name) not in completed
    ]
    blocking_unavailable = [item for item in unavailable if item.get("blocking", True)]
    complete = failure is None and not pending and not blocking_unavailable
    resume_safe = failure is None and bool(pending) and not blocking_unavailable
    report = {
        "schema": "gemini-live-calibration/v2",
        "run_id": run_id,
        "complete": complete,
        "failure": failure,
        "resume_safe": resume_safe,
        "resume_proof": (
            "x-apitoken-execution-state:not_started" if resume_safe else None
        ),
        "budget_nanousd_total": str(budget_nano),
        "spent_nanousd_total": str(budget.total_nano),
        "spent_nanousd_per_profile": {key: str(value) for key, value in sorted(budget.by_profile.items())},
        "production_transport": {
            "capacity_over_ssh": args.production_capacity_over_ssh,
            "api_over_ssh": args.production_api_over_ssh,
            "ssh_target": args.production_ssh_target if (
                args.production_capacity_over_ssh or args.production_api_over_ssh
            ) else None,
            "capacity_port": args.production_capacity_port if args.production_capacity_over_ssh else None,
            "api_port": args.production_api_port if args.production_api_over_ssh else None,
        },
        "profiles": profiles,
        "models": models,
        "records": runner.records,
        "unavailable_capabilities": unavailable,
        "blocking_unavailable_capabilities": blocking_unavailable,
        "profile_stops": stopped,
        "pending_legs": pending,
        "model_profitability": model_profitability(runner.records),
        "final_capacity": final,
    }
    report_path = Path(args.report)
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"report: {report_path}")
    if not complete:
        reason = failure or f"{len(pending)} Gemini coverage legs remain after explicit provider stops"
        raise CalibrationError(f"{reason}; partial report: {report_path}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        print(f"Gemini calibration stopped safely: {error}", file=sys.stderr)
        sys.exit(1)
