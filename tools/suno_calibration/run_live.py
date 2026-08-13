#!/usr/bin/env python3
"""Fail-closed live calibration runner for the Suno subscription plane (backend-only, dormant).

The runner talks to the PLANE, never to the provider directly: paid generations are created
through `POST /v1/audio/generations` on the exact local engine origin with a forwarding-admin
key (the admin path is unmetered for customer accounts; the subscription pays and the durable
calibration ledger records the turn), and all evidence is read from the admin-only
`GET /suno-subs` projection (control key in `x-api-key`): delivery diagnostics
(`pending_events`/`dropped_events`/`persistence_ok`), the free provider preflight
(`profiles[].quota` — the plane's `GET /api/billing/info/` sweep, raw counters verbatim), the
fleet counters (`tracked_generations`, `unattributed_settlements`, `tariff_anomaly`), and the
per-window durable cumulative dual-ledger spend (`calibration[].observed_spend_nano` /
`observed_spend_native_millicredits`).

Unlike the KIMI plane, Suno has NO admin-only calibration profile/request-id headers: the
gateway deliberately carries no such hook. Exact targeting is instead achieved structurally —
the run REQUIRES a single-profile roster (`fleet.profiles == 1` and the id equals `--profile`),
so selection cannot spill or rebind to a neighbour — and attribution is guarded by the fleet
counters: `tracked_generations` must advance by exactly one and no foreign `inflight` may
appear while a leg settles, otherwise the leg's delta is recorded as ambiguous and the matrix
stops fail closed. This is the strongest attribution the plane supports; do not weaken it.

Dry-run is the default and sends no paid traffic. `--execute` requires an explicit
`--budget-usd` (strict decimal, integer nanoUSD internally) with a hard CLI ceiling of $1.00:
one song is 5 credits = $0.02 at the reviewed derived schedule, so the repo's default $0.0001
admission cap is unusable here by construction (docs/engine/SUNO_PROVIDER.md §7 open
question) — the operator must name a budget, and no flag can raise the ceiling.

A paid create is never retried after a transport ambiguity: the leg is held at its full
worst-case bound and is never re-sent, even on `--resume`. A typed non-2xx create response is
pre-money-boundary evidence (the plane's money boundary is the upstream create inside the
profile lease; every error response is produced before it), so it never holds budget.
Read-only polls (`GET /suno-subs`, generation status) retry in a bounded fashion. A
machine-readable checkpoint is written after every leg.
"""

from __future__ import annotations

import argparse
import dataclasses
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
from pathlib import Path
from typing import Any


NANO_PER_USD = 1_000_000_000
# Reviewed DERIVED schedule: $0.004/credit (Pro unit economics, conservative against Premier),
# docs/engine/SUNO_PROVIDER.md §5.1, crates/metering/src/suno.rs::SUNO_NANOUSD_PER_CREDIT.
NANOUSD_PER_CREDIT = 4_000_000
CREDITS_PER_SONG = 5
# Operations without a published price reserve the documented conservative bound: the highest
# published per-operation price (manifest §5.1, owner directive 2026-08-12).
CONSERVATIVE_RESERVE_CREDITS = 50
# Hard CLI ceiling for the aggregate run budget. The default matrix worst case is 75 credits
# (5 song legs + lyrics at the conservative reserve) = $0.30; with the extend+stems legs
# enabled it is 175 credits = $0.70. There is NO default budget: `--execute` without
# `--budget-usd` is an error.
MAX_BUDGET_USD = "1.00"
MAX_BUDGET_NANO = NANO_PER_USD
SAFE_READ_ATTEMPTS = 3
SAFE_READ_RETRY_DELAY_SECONDS = 2.0
DEFAULT_SETTLE_DELAY_SECONDS = 5.0
DEFAULT_HTTP_TIMEOUT_SECONDS = 120
# Generations are polled by the plane's detached drain; the runner's own status poll must
# outlive a normal song but not run forever.
DEFAULT_TASK_TIMEOUT_SECONDS = 1500
DEFAULT_EVIDENCE_TIMEOUT_SECONDS = 300
DEFAULT_API_URL = "http://127.0.0.1:8787"

CHECKPOINT_SCHEMA = "suno-live-calibration-checkpoint/v1"
REPORT_SCHEMA = "suno-live-calibration/v1"
PLAN_SCHEMA = "suno-live-calibration-plan/v1"

TERMINAL_LEG_STATUSES = frozenset({"ok", "unavailable", "held-ambiguous", "failed"})
# Suno generation lifecycle (crates/forward/src/suno/gateway.rs SunoGenerationView):
# `created` until the first poll answer, then `queued`/`streaming`; finalized `complete`,
# `error`, or `expired` on a poll deadline.
TERMINAL_GENERATION_STATUSES = frozenset({"complete", "error", "expired"})
SUCCESS_STATUS = "complete"

PROFILE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$")
RUN_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,79}$")
GENERATION_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")

# Reviewed paid model catalog (docs/engine/SUNO_PROVIDER.md §3, mirrors
# crates/metering/src/suno.rs::SUNO_PAID_MODELS). Free-tier and deprecated ids are not
# admitted by the plane either. The wire spellings are unknown until this matrix pins them.
PAID_MODELS = ("v4", "v4.5", "v4.5+", "v5", "v5.5")
# The plane onboards Pro/Premier only; the credential and the 0050 CHECK pin these labels.
REVIEWED_PLANS = ("Pro", "Premier")


class CalibrationError(RuntimeError):
    """A fail-closed calibration invariant was not satisfied."""


class TransportFailureError(CalibrationError):
    """Transport failed. Retriable on a read-only poll; on a paid create it is an ambiguity
    after which the request is never repeated automatically."""


class HttpCalibrationError(CalibrationError):
    """A typed plane response: every non-2xx create response is produced before the plane's
    money boundary (the upstream create), so it is safe to classify."""

    def __init__(self, path: str, status: int, detail: str) -> None:
        super().__init__(f"{path} returned HTTP {status}: {detail}")
        self.path = path
        self.status = status
        self.detail = detail


class PaidLegError(CalibrationError):
    """A paid create was dispatched but its outcome cannot be proved. The leg is held at its
    full worst-case bound and never re-sent."""

    def __init__(self, reason: str, upper_bound_nano: int) -> None:
        super().__init__(reason)
        self.upper_bound_nano = upper_bound_nano


class PostSpendPollError(CalibrationError):
    """The paid leg completed and was charged, but the attribution read failed afterwards."""


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


def fmt_usd(nano: int) -> str:
    whole, fraction = divmod(nano, NANO_PER_USD)
    if fraction == 0:
        return f"${whole}"
    return f"${whole}.{fraction:09d}".rstrip("0")


def validate_profile_id(value: Any) -> str:
    """Opaque roster id, mirrored from the credential crate's bounds."""
    if not isinstance(value, str) or not PROFILE_RE.match(value):
        raise CalibrationError(
            "--profile must be an opaque roster id of 1-128 chars: alnum, dash, underscore"
        )
    return value


def validate_generation_id(value: Any) -> str:
    """The plane's internal request id, embedded into the status path — must be path-safe."""
    if not isinstance(value, str) or not GENERATION_ID_RE.match(value):
        raise CalibrationError(f"unsafe generation id from the plane: {value!r}")
    return value


def validate_url(value: str, flag: str) -> str:
    candidate = value.strip().rstrip("/")
    if (
        not candidate
        or len(candidate) > 2048
        or not (candidate.startswith("https://") or candidate.startswith("http://"))
        or any(char.isspace() for char in candidate)
        or "@" in candidate.split("/", 3)[2]
    ):
        raise CalibrationError(f"{flag} must be a plain http(s) URL without credentials")
    return candidate


def validate_upstream_id(value: str, flag: str) -> str:
    """An upstream clip/song id the operator pastes; bounded text, path-safe."""
    if not 1 <= len(value) <= 128 or not all(
        char.isascii() and (char.isalnum() or char in "-_") for char in value
    ):
        raise CalibrationError(f"{flag} must be 1-128 chars: alnum, dash, underscore")
    return value


@dataclasses.dataclass(frozen=True)
class Leg:
    name: str
    operation: str  # song | extend | lyrics | stems
    model: str | None = None
    continue_clip_id: str | None = None
    song_id: str | None = None
    # A typed refusal of a required leg stops the run; optional legs record unavailability
    # and the matrix continues.
    required: bool = True


def leg_reserve_credits(leg: Leg) -> int:
    """Published price where one exists (song: 5), else the documented conservative reserve
    (50) — mirrors OperationKind::reserve_credits in crates/forward/src/suno/gateway.rs."""
    if leg.operation == "song":
        if leg.model not in PAID_MODELS:
            raise CalibrationError(f"unreviewed paid model id: {leg.model}")
        return CREDITS_PER_SONG
    if leg.operation in ("extend", "lyrics", "stems"):
        return CONSERVATIVE_RESERVE_CREDITS
    raise CalibrationError(f"runner matrix has an unpriceable operation: {leg.operation}")


def leg_upper_bound_nano(leg: Leg) -> int:
    return leg_reserve_credits(leg) * NANOUSD_PER_CREDIT


def build_legs(
    models: list[str],
    continue_clip_id: str | None,
    song_id: str | None,
) -> tuple[list[Leg], list[dict[str, Any]]]:
    """The reviewed matrix: one song per paid model, one lyrics leg, and — only with the
    operator-supplied upstream ids — the extend and stems legs at their conservative reserve.
    Legs that cannot be built without operator input are returned as unavailable entries,
    never silently skipped."""
    legs: list[Leg] = []
    unavailable: list[dict[str, Any]] = []
    for model in models:
        if model not in PAID_MODELS:
            raise CalibrationError(f"--models carries an unreviewed paid model id: {model}")
        legs.append(Leg(f"song:{model}", "song", model=model))
    legs.append(Leg("lyrics", "lyrics", required=False))
    if continue_clip_id is not None:
        legs.append(
            Leg("extend", "extend", continue_clip_id=continue_clip_id, required=False)
        )
    else:
        unavailable.append({
            "capability": "extend",
            "reason": (
                "needs --continue-clip-id (an upstream clip id of a finished song; the plane "
                "never exposes upstream ids, so the operator supplies one); not provided"
            ),
            "blocking": False,
            "skipped_before_dispatch": True,
        })
    if song_id is not None:
        legs.append(Leg("stems", "stems", song_id=song_id, required=False))
    else:
        unavailable.append({
            "capability": "stems",
            "reason": (
                "needs --song-id (an upstream song id; the plane never exposes upstream ids, "
                "so the operator supplies one); not provided"
            ),
            "blocking": False,
            "skipped_before_dispatch": True,
        })
    return legs, unavailable


def body_for_leg(leg: Leg, run_id: str) -> dict[str, Any]:
    body: dict[str, Any] = {"operation": leg.operation}
    if leg.operation == "song":
        # Instrumental description form: the smallest reviewed shape, no lyrics content risk.
        body["model"] = leg.model
        body["make_instrumental"] = True
        body["prompt"] = (
            f"Suno calibration {run_id} {leg.name}: a short calm instrumental."
        )
    elif leg.operation == "lyrics":
        body["prompt"] = f"Suno calibration {run_id}: a two-line verse about calibration."
    elif leg.operation == "extend":
        body["continue_clip_id"] = leg.continue_clip_id
    else:
        body["song_id"] = leg.song_id
    return body


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
        if actual_nano < 0:
            raise CalibrationError("priced usage must not be negative")
        if self.committed_nano() + actual_nano > self.limit_nano:
            raise CalibrationError("provider evidence exceeded the run budget")
        self.spent_nano += actual_nano

    def hold(self, upper_bound_nano: int) -> None:
        if upper_bound_nano <= 0:
            raise CalibrationError("request upper bound must be positive")
        if self.committed_nano() + upper_bound_nano > self.limit_nano:
            raise CalibrationError("worst-case hold exceeded the run budget")
        self.held_nano += upper_bound_nano


def require_healthy_plane(payload: dict[str, Any], require_empty: bool = True) -> None:
    if payload.get("enabled") is not True:
        raise CalibrationError("Suno provider plane is not enabled")
    if payload.get("calibration_authority_available") is not True:
        raise CalibrationError("Suno exact calibration authority is unavailable")
    delivery = payload.get("delivery")
    if not isinstance(delivery, dict):
        raise CalibrationError("/suno-subs response has no delivery diagnostics")
    pending = as_int(delivery.get("pending_events"), "delivery.pending_events")
    dropped = as_int(delivery.get("dropped_events"), "delivery.dropped_events")
    if dropped:
        raise CalibrationError(f"Suno calibration delivery dropped {dropped} events")
    if delivery.get("persistence_ok") is not True:
        raise CalibrationError("Suno calibration persistence is degraded")
    if require_empty and pending:
        raise CalibrationError(f"Suno calibration still has {pending} pending events")


def fleet_counters(payload: dict[str, Any]) -> dict[str, int]:
    fleet = payload.get("fleet")
    if not isinstance(fleet, dict):
        raise CalibrationError("/suno-subs response has no fleet block")
    counters = {}
    for field in (
        "profiles",
        "inflight_requests",
        "inflight_drains",
        "tracked_generations",
        "unattributed_settlements",
        "tariff_anomaly",
    ):
        counters[field] = as_int(fleet.get(field), f"fleet.{field}")
    return counters


def calibration_rows(profile: dict[str, Any]) -> dict[int, dict[str, int]]:
    """Per-window cumulative spend, keyed by exact `window_duration_secs`. An absent block
    means no row yet; a duplicate duration is contract drift and fails closed."""
    rows: dict[int, dict[str, int]] = {}
    calibration = profile.get("calibration")
    if calibration is None:
        return rows
    if not isinstance(calibration, list):
        raise CalibrationError("Suno profile calibration block is not a window list")
    for row in calibration:
        if not isinstance(row, dict):
            raise CalibrationError("Suno calibration row is not an object")
        duration = as_int(row.get("window_duration_secs"), "calibration.window_duration_secs")
        if duration in rows:
            raise CalibrationError(f"duplicate Suno window duration {duration}")
        rows[duration] = {
            "spend_nano": as_int(
                row.get("observed_spend_nano"), "calibration.observed_spend_nano"
            ),
            "spend_millicredits": as_int(
                row.get("observed_spend_native_millicredits"),
                "calibration.observed_spend_native_millicredits",
            ),
            "unattributed_fraction_units": as_int(
                row.get("unattributed_fraction_units"),
                "calibration.unattributed_fraction_units",
            ),
        }
    return rows


def spend_delta(
    before_rows: dict[int, dict[str, int]], after_rows: dict[int, dict[str, int]]
) -> tuple[int, int, int]:
    """Cumulative dual-ledger delta across the exact-duration rows. Every row present in both
    snapshots must show the SAME delta (the subject spend is one ledger seen through each
    window); disagreement, a changed row set, or no rows at all is broken evidence and fails
    closed. Returns (nano, millicredits, unattributed_fraction_units)."""
    if not before_rows or not after_rows:
        raise CalibrationError("Suno calibration window rows are missing on one side")
    if set(before_rows) != set(after_rows):
        raise CalibrationError(
            "Suno calibration window row set changed during the leg (reset/cutover)"
        )
    deltas = set()
    for duration in before_rows:
        deltas.add((
            after_rows[duration]["spend_nano"] - before_rows[duration]["spend_nano"],
            after_rows[duration]["spend_millicredits"]
            - before_rows[duration]["spend_millicredits"],
            after_rows[duration]["unattributed_fraction_units"]
            - before_rows[duration]["unattributed_fraction_units"],
        ))
    if len(deltas) != 1:
        raise CalibrationError("Suno calibration window rows disagree on the spend delta")
    nano, milli, unattributed = deltas.pop()
    if nano < 0 or milli < 0 or unattributed < 0:
        raise CalibrationError("Suno cumulative spend counters moved backwards")
    return nano, milli, unattributed


def profile_view(payload: dict[str, Any], profile_id: str, now: int) -> dict[str, Any]:
    """Parse the exact target profile and enforce the single-profile no-spill guard: the Suno
    plane has no admin-only calibration header hook, so a one-profile roster is the
    structural guarantee that selection cannot rebind to a neighbour."""
    counters = fleet_counters(payload)
    if counters["profiles"] != 1:
        raise CalibrationError(
            f"the calibration roster must hold exactly one owned profile, found "
            f"{counters['profiles']}"
        )
    profiles = payload.get("profiles")
    if not isinstance(profiles, list) or len(profiles) != 1:
        raise CalibrationError("/suno-subs profiles block is not a single-entry list")
    raw = profiles[0]
    if not isinstance(raw, dict) or raw.get("id") != profile_id:
        raise CalibrationError(f"exact Suno profile is absent from /suno-subs: {profile_id}")
    if raw.get("live") is not True or raw.get("routable") is not True:
        raise CalibrationError(f"exact Suno profile is dead or out of rotation: {profile_id}")
    if raw.get("quota_walled") is True:
        raise CalibrationError(f"exact Suno profile is quota-walled: {profile_id}")
    cooling = raw.get("cooling")
    if not isinstance(cooling, dict):
        raise CalibrationError("Suno profile has no cooling block")
    for field in ("rate_limit_until", "auth_until", "captcha_until", "transport_until"):
        value = optional_int(cooling.get(field), f"profile.cooling.{field}")
        if value is not None and value > now:
            raise CalibrationError(f"exact Suno profile is cooling ({field}): {profile_id}")
    plan = raw.get("plan")
    if not isinstance(plan, str) or plan not in REVIEWED_PLANS:
        raise CalibrationError(
            f"exact Suno profile has no authoritative reviewed paid plan: {plan!r}"
        )
    quota = raw.get("quota")
    if not isinstance(quota, dict):
        raise CalibrationError("Suno profile has no quota block")
    return {
        "id": profile_id,
        "plan": plan,
        "inflight": as_int(raw.get("inflight"), "profile.inflight"),
        "quota_observed_at": optional_int(quota.get("observed_at"), "profile.quota.observed_at"),
        "monthly_limit": optional_int(quota.get("monthly_limit"), "profile.quota.monthly_limit"),
        "monthly_usage": optional_int(quota.get("monthly_usage"), "profile.quota.monthly_usage"),
        "total_credits_left": optional_int(
            quota.get("total_credits_left"), "profile.quota.total_credits_left"
        ),
        "calibration_rows": calibration_rows(raw),
    }


class PlaneClient:
    """The paid/status surface of the local plane. A paid create gets exactly one transport
    attempt; only the read-only status poll retries."""

    def __init__(self, base_url: str, api_key: str, timeout: int) -> None:
        self.base_url = base_url
        self.api_key = api_key
        self.timeout = timeout

    def _send(self, path: str, method: str, body: dict[str, Any] | None) -> bytes:
        data = None if body is None else json.dumps(body, separators=(",", ":")).encode()
        headers = {
            "x-api-key": self.api_key,
            "content-type": "application/json",
            "accept": "application/json",
        }
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            raw = error.read(800).decode(errors="replace")
            detail = raw.replace(self.api_key, "***") if self.api_key else raw
            raise HttpCalibrationError(path, error.code, detail) from error
        except (urllib.error.URLError, OSError) as error:
            raise TransportFailureError(f"{path} transport failed: {error}") from error

    @staticmethod
    def _parse(path: str, raw: bytes) -> dict[str, Any]:
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError(f"{path} returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError(f"{path} returned a non-object")
        return payload

    def create(self, body: dict[str, Any]) -> dict[str, Any]:
        # Exactly one attempt, ever: a transport failure here is a paid ambiguity.
        return self._parse(
            "/v1/audio/generations", self._send("/v1/audio/generations", "POST", body)
        )

    def generation_status_once(self, generation_id: str) -> dict[str, Any]:
        path = f"/v1/audio/generations/{generation_id}"
        return self._parse(path, self._send(path, "GET", None))

    def generation_status(self, generation_id: str) -> dict[str, Any]:
        """Read-only poll: bounded transport retries are safe."""
        for attempt in range(SAFE_READ_ATTEMPTS):
            try:
                return self.generation_status_once(generation_id)
            except TransportFailureError:
                if attempt + 1 == SAFE_READ_ATTEMPTS:
                    raise
                time.sleep(SAFE_READ_RETRY_DELAY_SECONDS)
        raise CalibrationError("unreachable generation-status retry state")


class CapacityReader:
    """Read-only `GET /suno-subs` via an operator command or a URL with the control key."""

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
            raise CalibrationError(
                "control key is required with --capacity-url (env CLAUDE_API_CONTROL_KEY)"
            )

    def read_once(self) -> dict[str, Any]:
        if self.command:
            result = subprocess.run(
                self.command, capture_output=True, timeout=self.timeout, check=False
            )
            if result.returncode:
                raise TransportFailureError(
                    "capacity command failed: "
                    + result.stderr[-500:].decode(errors="replace")
                )
            raw = result.stdout
        else:
            request = urllib.request.Request(
                self.url or "", headers={"x-api-key": self.control_key or ""}
            )
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    raw = response.read()
            except urllib.error.HTTPError as error:
                raise TransportFailureError(
                    f"capacity read returned HTTP {error.code}"
                ) from error
            except urllib.error.URLError as error:
                raise TransportFailureError(f"capacity read failed: {error}") from error
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as error:
            raise CalibrationError("capacity source returned invalid JSON") from error
        if not isinstance(payload, dict):
            raise CalibrationError("capacity source returned a non-object")
        return payload

    def read(self) -> dict[str, Any]:
        """Bounded retry is allowed because /suno-subs reads are strictly read-only."""
        for attempt in range(SAFE_READ_ATTEMPTS):
            try:
                return self.read_once()
            except (TransportFailureError, subprocess.TimeoutExpired):
                if attempt + 1 == SAFE_READ_ATTEMPTS:
                    raise
                time.sleep(SAFE_READ_RETRY_DELAY_SECONDS)
        raise CalibrationError("capacity source produced no result")


class Runner:
    def __init__(
        self,
        client: PlaneClient,
        capacity: CapacityReader,
        budget: Budget,
        run_id: str,
        task_timeout: int,
        evidence_timeout: int,
        settle_delay: float,
        poll_interval: float = 5.0,
    ) -> None:
        self.client = client
        self.capacity = capacity
        self.budget = budget
        self.run_id = run_id
        self.task_timeout = task_timeout
        self.evidence_timeout = evidence_timeout
        self.settle_delay = settle_delay
        self.poll_interval = poll_interval

    def execute_leg(self, leg: Leg, profile_id: str) -> dict[str, Any]:
        bound = leg_upper_bound_nano(leg)
        self.budget.require_room(bound)
        print(
            f"{leg.name}: worst-case {leg_reserve_credits(leg)} credits ({fmt_usd(bound)})",
            flush=True,
        )
        before = self.capacity.read()
        require_healthy_plane(before)
        before_fleet = fleet_counters(before)
        before_profile = profile_view(before, profile_id, int(time.time()))
        if before_profile["inflight"]:
            raise CalibrationError("exact Suno profile has in-flight work before dispatch")
        if before_profile["quota_observed_at"] is None:
            raise CalibrationError(
                "no free quota preflight yet (quota.observed_at is null); the plane's "
                "billing-info sweep must answer before paid traffic"
            )
        body = body_for_leg(leg, self.run_id)
        try:
            response = self.client.create(body)
        except HttpCalibrationError:
            # A typed non-2xx create is pre-money-boundary evidence; nothing to hold.
            raise
        except (TransportFailureError, CalibrationError) as error:
            raise PaidLegError(str(error), bound) from error
        generation_id = validate_generation_id(response.get("generation_id"))
        # Status poll until the generation finalizes (read-only; retries inside).
        deadline = time.monotonic() + self.task_timeout
        view: dict[str, Any] | None = None
        while time.monotonic() < deadline:
            view = self.client.generation_status(generation_id)
            status = view.get("status")
            if not isinstance(status, str):
                raise PostSpendPollError(
                    f"generation {generation_id} status payload is malformed"
                )
            if status in TERMINAL_GENERATION_STATUSES:
                break
            time.sleep(self.poll_interval)
        else:
            raise PostSpendPollError(
                f"generation {generation_id} did not finalize within {self.task_timeout}s"
            )
        completed_at = int(time.time())
        if self.settle_delay > 0:
            time.sleep(self.settle_delay)
        # Settlement wait: FIFO drained, drains back to baseline, and a post-turn quota
        # observation at or after completion. Unresolved is not zero.
        observed = before
        after_profile: dict[str, Any] | None = None
        settle_deadline = time.monotonic() + self.evidence_timeout
        while time.monotonic() < settle_deadline:
            observed = self.capacity.read()
            require_healthy_plane(observed, require_empty=False)
            delivery_pending = as_int(
                observed["delivery"]["pending_events"], "delivery.pending_events"
            )
            fleet = fleet_counters(observed)
            candidate = profile_view(observed, profile_id, int(time.time()))
            quota_advanced = (
                candidate["quota_observed_at"] is not None
                and candidate["quota_observed_at"] >= completed_at
            )
            if (
                delivery_pending == 0
                and fleet["inflight_drains"] <= before_fleet["inflight_drains"]
                and quota_advanced
            ):
                after_profile = candidate
                break
            time.sleep(self.poll_interval)
        if after_profile is None:
            raise PostSpendPollError(
                f"{leg.name}: settlement evidence did not resolve within "
                f"{self.evidence_timeout}s"
            )
        require_healthy_plane(observed)
        after_fleet = fleet_counters(observed)
        # A tariff anomaly quarantines the turn — a typed fail-closed stop, never silent.
        if after_fleet["tariff_anomaly"] != before_fleet["tariff_anomaly"]:
            raise CalibrationError(
                f"{leg.name}: the plane quarantined a tariff anomaly during the leg"
            )
        # The settlement path itself is evidence: attributed delta vs conservative-reserve
        # fallback (the unattributed_settlements counter advanced).
        settlement = (
            "reserve-fallback"
            if after_fleet["unattributed_settlements"]
            != before_fleet["unattributed_settlements"]
            else "attributed"
        )
        # Concurrency: our generation is exactly one new tracked generation, and no foreign
        # in-flight work may touch the profile while the delta is read.
        foreign_traffic = (
            after_fleet["tracked_generations"] - before_fleet["tracked_generations"] != 1
            or after_fleet["inflight_requests"] != 0
            or after_profile["inflight"] != 0
        )
        nano_delta, milli_delta, unattributed_fraction_delta = spend_delta(
            before_profile["calibration_rows"], after_profile["calibration_rows"]
        )
        usage_delta: int | None = None
        if (
            before_profile["monthly_usage"] is not None
            and after_profile["monthly_usage"] is not None
            and before_profile["monthly_limit"] == after_profile["monthly_limit"]
        ):
            usage_delta = after_profile["monthly_usage"] - before_profile["monthly_usage"]
        credits_left_drawdown: int | None = None
        if (
            before_profile["total_credits_left"] is not None
            and after_profile["total_credits_left"] is not None
        ):
            credits_left_drawdown = (
                before_profile["total_credits_left"] - after_profile["total_credits_left"]
            )
        attribution = "ambiguous" if foreign_traffic else "exact"
        if view["status"] != SUCCESS_STATUS:
            raise CalibrationError(
                f"{leg.name}: generation finalized as {view['status']!r}, expected complete"
            )
        if nano_delta <= 0:
            raise CalibrationError(
                f"{leg.name}: completed generation produced no settled spend"
            )
        if nano_delta > bound:
            raise CalibrationError(
                f"{leg.name}: settled spend {fmt_usd(nano_delta)} exceeded the preflight bound"
            )
        self.budget.charge(nano_delta)
        record = {
            "leg": leg.name,
            "operation": leg.operation,
            "model": leg.model,
            "generation_id": generation_id,
            "generation_status": view["status"],
            "generation_error": view.get("error"),
            "artifacts": view.get("artifacts"),
            "reserve_credits": leg_reserve_credits(leg),
            "upper_bound_nanousd": str(bound),
            "settled_nanousd": str(nano_delta),
            "settled_native_millicredits": str(milli_delta),
            "settlement": settlement,
            "unattributed_fraction_delta": unattributed_fraction_delta,
            "attribution": attribution,
            "foreign_traffic": foreign_traffic,
            "quota_before": {
                "monthly_limit": before_profile["monthly_limit"],
                "monthly_usage": before_profile["monthly_usage"],
                "total_credits_left": before_profile["total_credits_left"],
                "observed_at": before_profile["quota_observed_at"],
            },
            "quota_after": {
                "monthly_limit": after_profile["monthly_limit"],
                "monthly_usage": after_profile["monthly_usage"],
                "total_credits_left": after_profile["total_credits_left"],
                "observed_at": after_profile["quota_observed_at"],
            },
            "monthly_usage_delta": usage_delta,
            "credits_left_drawdown": credits_left_drawdown,
        }
        print(
            f"{leg.name}: status={view['status']} settled={fmt_usd(nano_delta)} "
            f"settlement={settlement} attribution={attribution}",
            flush=True,
        )
        # An ambiguous delta is RETURNED, not raised: the money moved and must be recorded and
        # charged; the caller stops the matrix fail closed on attribution != "exact".
        return record


def new_run_id() -> str:
    return f"suno-cal-{int(time.time())}-{uuid.uuid4().hex[:8]}"


def fresh_state() -> dict[str, Any]:
    return {
        "spent_nano": 0,
        "held_nano": 0,
        "records": [],
        "unavailable": [],
        "stops": [],
        "leg_status": {},
    }


def matrix_identity(
    models: list[str], continue_clip_id: str | None, song_id: str | None
) -> dict[str, Any]:
    return {
        "models": list(models),
        "continue_clip_id": continue_clip_id,
        "song_id": song_id,
    }


def checkpoint_payload(
    run_id: str,
    profile: str,
    api_url: str,
    budget_nano: int,
    matrix: dict[str, Any],
    state: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": CHECKPOINT_SCHEMA,
        "run_id": run_id,
        "profile": profile,
        "api_url": api_url,
        "budget_nanousd": str(budget_nano),
        "matrix": matrix,
        "spent_nano": state["spent_nano"],
        "held_nano": state["held_nano"],
        "records": state["records"],
        "unavailable": state["unavailable"],
        "stops": state["stops"],
        "leg_status": state["leg_status"],
    }


CHECKPOINT_REQUIRED_KEYS = frozenset(
    checkpoint_payload("", "", "", 0, matrix_identity([], None, None), fresh_state())
)


def save_checkpoint(path: Path, payload: dict[str, Any]) -> None:
    tmp = Path(str(path) + ".tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    os.replace(tmp, path)


def load_resume(
    path: str,
    profile: str,
    api_url: str,
    budget_nano: int,
    matrix: dict[str, Any],
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
    if payload["api_url"] != api_url:
        mismatches.append("api_url")
    if as_int(payload["budget_nanousd"], "checkpoint budget") != budget_nano:
        mismatches.append("budget")
    if payload["matrix"] != matrix:
        mismatches.append("matrix")
    if mismatches:
        raise CalibrationError(
            "resume identity mismatch with the checkpoint: " + ", ".join(mismatches)
        )
    state = {
        "spent_nano": as_int(payload["spent_nano"], "checkpoint spent_nano"),
        "held_nano": as_int(payload["held_nano"], "checkpoint held_nano"),
        "records": payload["records"],
        "unavailable": payload["unavailable"],
        "stops": payload["stops"],
        "leg_status": payload["leg_status"],
    }
    return str(payload["run_id"]), state


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true", help="required to send paid traffic")
    parser.add_argument("--profile", help="exact opaque roster id (required with --execute)")
    parser.add_argument(
        "--api-url",
        default=DEFAULT_API_URL,
        help="exact local plane origin (default http://127.0.0.1:8787)",
    )
    parser.add_argument("--api-key-env", default="APITOKEN_API_KEY")
    parser.add_argument("--capacity-command")
    parser.add_argument("--capacity-url", help="default: <api-url>/suno-subs")
    parser.add_argument("--control-key-env", default="CLAUDE_API_CONTROL_KEY")
    parser.add_argument(
        "--budget-usd",
        default=None,
        help=(
            "explicit aggregate run budget, strict decimal USD; REQUIRED with --execute, hard "
            f"ceiling {MAX_BUDGET_USD} (the default $0.0001 admission cap cannot buy one "
            "5-credit song — naming a budget IS the operator's authorization)"
        ),
    )
    parser.add_argument("--models", nargs="*", choices=PAID_MODELS)
    parser.add_argument(
        "--continue-clip-id",
        help="upstream clip id of a finished song; enables the extend leg",
    )
    parser.add_argument(
        "--song-id",
        help="upstream song id; enables the stems leg",
    )
    parser.add_argument("--run-id")
    parser.add_argument("--resume", help="path to a checkpoint of an earlier incomplete run")
    parser.add_argument("--report", default="/tmp/suno-calibration-report.json")
    parser.add_argument("--checkpoint", help="default: <report>.checkpoint.json")
    parser.add_argument("--task-timeout", type=int, default=DEFAULT_TASK_TIMEOUT_SECONDS)
    parser.add_argument("--evidence-timeout", type=int, default=DEFAULT_EVIDENCE_TIMEOUT_SECONDS)
    parser.add_argument("--settle-delay", type=float, default=DEFAULT_SETTLE_DELAY_SECONDS)
    parser.add_argument("--http-timeout", type=int, default=DEFAULT_HTTP_TIMEOUT_SECONDS)
    args = parser.parse_args(argv)
    try:
        if args.budget_usd is not None:
            budget_nano = usd_to_nano(args.budget_usd)
            if budget_nano <= 0 or budget_nano > MAX_BUDGET_NANO:
                raise CalibrationError(
                    f"--budget-usd must be positive and no greater than {MAX_BUDGET_USD}"
                )
        if args.profile is not None:
            validate_profile_id(args.profile)
        validate_url(args.api_url, "--api-url")
        if args.capacity_url is not None:
            validate_url(args.capacity_url, "--capacity-url")
        if args.continue_clip_id is not None:
            validate_upstream_id(args.continue_clip_id, "--continue-clip-id")
        if args.song_id is not None:
            validate_upstream_id(args.song_id, "--song-id")
        if args.run_id and not RUN_ID_RE.match(args.run_id):
            raise CalibrationError("--run-id has unsafe characters")
    except CalibrationError as error:
        parser.error(str(error))
    if args.execute and not args.profile:
        parser.error("--profile is required with --execute")
    if args.execute and args.budget_usd is None:
        parser.error(
            "--budget-usd is required with --execute: one Suno song is $0.02 at the reviewed "
            "derived schedule, which exceeds the repo's default $0.0001 admission cap, so the "
            "operator must name the authorized budget explicitly"
        )
    return args


def plan_json(
    args: argparse.Namespace,
    run_id: str,
    legs: list[Leg],
    matrix_unavailable: list[dict[str, Any]],
    baseline: dict[str, Any] | None,
    baseline_error: str | None,
) -> dict[str, Any]:
    leg_plans = []
    for leg in legs:
        leg_plans.append({
            "leg": leg.name,
            "operation": leg.operation,
            "model": leg.model,
            "reserve_credits": leg_reserve_credits(leg),
            "worst_case_nanousd": str(leg_upper_bound_nano(leg)),
            "required": leg.required,
        })
    total = sum(int(plan["worst_case_nanousd"]) for plan in leg_plans)
    notes = [
        "dry-run: no paid traffic was sent; the baseline, when present, is the free "
        "read-only GET /suno-subs projection"
    ]
    if args.budget_usd is not None and total > usd_to_nano(args.budget_usd):
        notes.append(
            "the total worst case exceeds the budget: the per-leg guard will stop the "
            "matrix partway; raise --budget-usd or shrink --models"
        )
    if baseline_error is not None:
        notes.append(f"the free baseline read failed; fix this before --execute: {baseline_error}")
    return {
        "schema": PLAN_SCHEMA,
        "mode": "dry-run",
        "run_id_preview": run_id,
        "paid_requests": 0,
        "target": {"profile": args.profile, "api_url": args.api_url},
        "budget_nanousd": (
            str(usd_to_nano(args.budget_usd)) if args.budget_usd is not None else None
        ),
        "budget_hard_cap_nanousd": str(MAX_BUDGET_NANO),
        "legs": leg_plans,
        "total_worst_case_nanousd": str(total),
        "matrix_unavailable": matrix_unavailable,
        "baseline": baseline,
        "guards": [
            "single-profile-roster-no-spill-no-rebind",
            "exact-internal-generation-id-plus-fleet-counter-attribution",
            "healthy-authority-and-empty-fifo-before-every-leg",
            "published-price-or-conservative-reserve-as-worst-case-bound",
            "explicit-budget-required-hard-capped-at-1.00-usd",
            "no-paid-create-retry-after-transport-ambiguity",
            "typed-non-2xx-create-is-pre-money-boundary",
            "post-turn-quota-observation-before-deltas",
            "tariff-anomaly-counter-advance-fails-closed",
            "reserve-fallback-settlement-is-recorded-not-hidden",
            "secrets-from-env-only-redacted-from-errors",
        ],
        "execute_requires": (
            "--execute, --profile, --budget-usd, an enabled local plane with a "
            "single-owned-profile roster, and explicit human authorization"
        ),
        "notes": notes,
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    models = list(args.models) if args.models else list(PAID_MODELS)
    legs, matrix_unavailable = build_legs(models, args.continue_clip_id, args.song_id)
    matrix = matrix_identity(models, args.continue_clip_id, args.song_id)
    capacity_url = args.capacity_url or f"{args.api_url}/suno-subs"

    if not args.execute:
        baseline = None
        baseline_error = None
        control_key = os.getenv(args.control_key_env, "")
        if control_key or args.capacity_command:
            try:
                capacity = CapacityReader(
                    args.capacity_command, None if args.capacity_command else capacity_url,
                    control_key or None, args.http_timeout,
                )
                baseline = capacity.read()
            except (CalibrationError, subprocess.TimeoutExpired) as error:
                baseline_error = str(error)
        print(
            json.dumps(
                plan_json(
                    args,
                    args.run_id or new_run_id(),
                    legs,
                    matrix_unavailable,
                    baseline,
                    baseline_error,
                ),
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0

    budget_nano = usd_to_nano(args.budget_usd)
    api_key = os.getenv(args.api_key_env, "")
    if not api_key:
        raise CalibrationError(f"missing API key environment variable: {args.api_key_env}")
    profile = validate_profile_id(args.profile)

    checkpoint_path = Path(args.checkpoint or (args.report + ".checkpoint.json"))
    if args.resume:
        run_id, state = load_resume(
            args.resume, profile, args.api_url, budget_nano, matrix
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

    capacity = CapacityReader(
        args.capacity_command,
        None if args.capacity_command else capacity_url,
        os.getenv(args.control_key_env),
        args.http_timeout,
    )
    client = PlaneClient(args.api_url, api_key, args.http_timeout)
    try:
        baseline = capacity.read()
        require_healthy_plane(baseline)
        baseline_profile = profile_view(baseline, profile, int(time.time()))
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        raise CalibrationError(
            f"baseline health gate failed; paid traffic was not started: {error}"
        ) from error
    budget = Budget(
        budget_nano, spent_nano=state["spent_nano"], held_nano=state["held_nano"]
    )
    runner = Runner(
        client,
        capacity,
        budget,
        run_id,
        args.task_timeout,
        args.evidence_timeout,
        args.settle_delay,
    )

    def save() -> None:
        save_checkpoint(
            checkpoint_path,
            checkpoint_payload(run_id, profile, args.api_url, budget_nano, matrix, state),
        )

    for entry in matrix_unavailable:
        # Resume must not duplicate the static entries already persisted in the checkpoint.
        if not any(
            existing.get("capability") == entry["capability"]
            for existing in state["unavailable"]
        ):
            state["unavailable"].append(entry)
    failure: str | None = None
    save()
    for leg in legs:
        if state["leg_status"].get(leg.name) in TERMINAL_LEG_STATUSES:
            continue
        try:
            record = runner.execute_leg(leg, profile)
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
            if error.status == 400 and not leg.required:
                state["unavailable"].append({
                    "capability": leg.name,
                    "http_status": error.status,
                    "reason": error.detail[:300],
                    "blocking": False,
                })
                state["leg_status"][leg.name] = "unavailable"
                print(f"{leg.name}: refused with a typed 400; recorded unavailable", flush=True)
            elif error.status == 400:
                state["unavailable"].append({
                    "capability": leg.name,
                    "http_status": error.status,
                    "reason": error.detail[:300],
                    "blocking": True,
                })
                state["leg_status"][leg.name] = "unavailable"
                failure = (
                    f"{leg.name}: a required generation capability was refused with HTTP 400"
                )
            elif error.status in {401, 403}:
                state["leg_status"][leg.name] = "failed"
                state["stops"].append({
                    "scope": f"profile:{profile}",
                    "reason": f"HTTP {error.status} on {leg.name}: session/quota wall; the "
                    "profile needs an operator",
                })
                failure = f"{leg.name}: HTTP {error.status} (session/quota wall); stopped"
            elif error.status == 429:
                state["leg_status"][leg.name] = "failed"
                state["stops"].append({
                    "scope": f"profile:{profile}",
                    "reason": f"HTTP 429 on {leg.name}: provider rate/quota wall",
                })
                failure = f"{leg.name}: provider rate wall (HTTP 429); stopped"
            else:
                state["leg_status"][leg.name] = "failed"
                failure = f"{leg.name}: {error}"
        except (TransportFailureError, subprocess.TimeoutExpired) as error:
            failure = f"read-only surface unavailable before {leg.name}: {error}"
        except CalibrationError as error:
            state["leg_status"][leg.name] = "failed"
            failure = f"{leg.name}: {error}"
        else:
            state["records"].append(record)
            state["leg_status"][leg.name] = "ok"
            if record["attribution"] != "exact":
                failure = (
                    f"{leg.name}: foreign traffic touched the profile during the leg; the "
                    "delta is recorded as ambiguous and the matrix stops fail closed"
                )
        state["spent_nano"] = budget.spent_nano
        state["held_nano"] = budget.held_nano
        save()
        if failure:
            break

    try:
        final = capacity.read()
    except (CalibrationError, subprocess.TimeoutExpired) as error:
        final = baseline
        failure = failure or f"final /suno-subs read failed: {error}"
    pending = [leg.name for leg in legs if leg.name not in state["leg_status"]]
    complete = (
        failure is None
        and all(
            state["leg_status"].get(leg.name) in {"ok", "unavailable"} for leg in legs
        )
    )
    report = {
        "schema": REPORT_SCHEMA,
        "run_id": run_id,
        "complete": complete,
        "failure": failure,
        "target": {"profile": profile, "api_url": args.api_url, "plan": baseline_profile["plan"]},
        "budget_nanousd": str(budget_nano),
        "spent_nanousd": str(budget.spent_nano),
        "held_nanousd": str(budget.held_nano),
        "models": models,
        "records": state["records"],
        "leg_status": state["leg_status"],
        "coverage": {
            "expected_legs": [leg.name for leg in legs],
            "completed_legs": sorted(
                name for name, status in state["leg_status"].items()
                if status in TERMINAL_LEG_STATUSES
            ),
            "pending_legs": pending,
        },
        "unavailable_capabilities": state["unavailable"],
        "stops": state["stops"],
        "baseline_observations": baseline,
        "final_observations": final,
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
        print(f"Suno calibration stopped safely: {error}", file=sys.stderr)
        sys.exit(1)
