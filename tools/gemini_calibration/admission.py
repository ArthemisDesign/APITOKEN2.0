#!/usr/bin/env python3
"""Offline one-shot admission state machine for dormant Gemini 3.7 Flash.

This module never opens a network connection. A future fixed root controller owns transport and
passes provider responses back as private files. The irreversible dispatch fence is committed
before that controller is allowed to read the paid request.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import fcntl
import hashlib
import json
import os
import re
import stat
import sys
import time
import uuid
from pathlib import Path
from typing import Any

try:
    from . import run_live
except ImportError:  # Direct `python3 tools/gemini_calibration/admission.py ...` execution.
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    from tools.gemini_calibration import run_live


MODEL = "gemini-3.7-flash"
SCHEMA = "gemini-3.7-admission/v3"
SUMMARY_SCHEMA = "gemini-3.7-admission-summary/v3"
COUNT_OBSERVATION_SCHEMA = "gemini-3.7-admission-count-observation/v3"
OBSERVATION_SCHEMA = "gemini-3.7-admission-observation/v2"
DEFAULT_BUDGET_NANOUSD = 100_000
DEFAULT_MAX_OUTPUT_TOKENS = 16
AUTHORIZED_PROMO_BUDGET_NANOUSD = 786_492_000
AUTHORIZED_MAX_OUTPUT_TOKENS = 16
OFFICIAL_TARIFF_SCHEDULE_ID = "google/gemini-developer-api/2026-08-14"
OFFICIAL_INPUT_TOKEN_LIMIT = 1_048_576
OFFICIAL_OUTPUT_TOKEN_LIMIT = 65_536
OFFICIAL_SEARCH_NANOUSD_PER_QUERY = 14_000_000
PROMO_END_EPOCH = 1_798_761_600  # 2027-01-01 00:00:00 UTC.
UINT64_MAX = 2**64 - 1
MAX_JSON_BYTES = 4 * 1024 * 1024
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
OPAQUE_PROFILE_RE = re.compile(r"^[A-Za-z0-9._-]{1,128}$")
PAID_PLANS = {
    "google_ai_pro",
    "google_ai_ultra",
    "code_assist_standard",
    "code_assist_enterprise",
    "workspace_ai_ultra",
}
JOURNAL = "journal.json"
CONTRACT_FENCE = "contract.fence"
COUNT_REQUEST = "count-request.json"
COUNT_DISPATCH_CLAIM = "count.dispatch.claim"
COUNT_RECEIPT = "count.receipt"
COUNT_OUTCOME_RECEIPT = "count.outcome.receipt"
GENERATION_REQUEST = "generation-request.json"
DISPATCH_FENCE = "dispatch.fence"
DISPATCH_CLAIM = "dispatch.claim"
OUTCOME_RECEIPT = "outcome.receipt"
KNOWN_ENTRIES = {
    JOURNAL,
    CONTRACT_FENCE,
    COUNT_REQUEST,
    COUNT_DISPATCH_CLAIM,
    COUNT_RECEIPT,
    COUNT_OUTCOME_RECEIPT,
    GENERATION_REQUEST,
    DISPATCH_FENCE,
    DISPATCH_CLAIM,
    OUTCOME_RECEIPT,
}


class AdmissionError(RuntimeError):
    """The one-shot must remain closed; the message contains no credential or profile id."""


def _read_regular_bytes(
    path: Path,
    *,
    maximum: int = MAX_JSON_BYTES,
    required_mode: int | None = None,
) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise AdmissionError("private file input is unavailable") from error
    try:
        info = os.fstat(descriptor)
        if (
            not stat.S_ISREG(info.st_mode)
            or info.st_nlink != 1
            or info.st_size > maximum
            or (required_mode is not None and stat.S_IMODE(info.st_mode) != required_mode)
        ):
            raise AdmissionError("private file input is not a bounded regular file")
        chunks: list[bytes] = []
        remaining = maximum + 1
        while remaining > 0:
            chunk = os.read(descriptor, min(65_536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        data = b"".join(chunks)
        if len(data) > maximum:
            raise AdmissionError("private file input exceeded its read ceiling")
        final = os.fstat(descriptor)
        if (
            final.st_dev != info.st_dev
            or final.st_ino != info.st_ino
            or final.st_size != info.st_size
            or final.st_mtime_ns != info.st_mtime_ns
            or final.st_ctime_ns != info.st_ctime_ns
        ):
            raise AdmissionError("private file input changed while it was read")
        return data
    finally:
        os.close(descriptor)


def _read_json(path: Path, *, required_mode: int | None = None) -> dict[str, Any]:
    value, _ = _read_json_with_digest(path, required_mode=required_mode)
    return value


def _read_json_with_digest(
    path: Path,
    *,
    required_mode: int | None = None,
) -> tuple[dict[str, Any], str]:
    try:
        data = _read_regular_bytes(path, required_mode=required_mode)
        value = json.loads(data)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise AdmissionError("private JSON input is invalid") from error
    if not isinstance(value, dict):
        raise AdmissionError("private JSON input is not an object")
    return value, hashlib.sha256(data).hexdigest()


def _fsync_dir(directory: Path) -> None:
    descriptor = os.open(directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _canonical_bytes(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def _atomic_json(path: Path, value: dict[str, Any], *, replace: bool) -> None:
    data = _canonical_bytes(value)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}.{uuid.uuid4().hex}")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(temporary, flags, 0o600)
    try:
        with os.fdopen(descriptor, "wb", closefd=True) as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        if replace:
            os.replace(temporary, path)
        else:
            try:
                os.link(temporary, path, follow_symlinks=False)
            except FileExistsError as error:
                raise AdmissionError("one-shot evidence already exists") from error
            temporary.unlink()
        os.chmod(path, 0o600, follow_symlinks=False)
        _fsync_dir(path.parent)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _exclusive_json(path: Path, value: dict[str, Any]) -> None:
    data = _canonical_bytes(value)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags, 0o600)
    except FileExistsError as error:
        raise AdmissionError("network dispatch is already permanently fenced") from error
    try:
        with os.fdopen(descriptor, "wb", closefd=True) as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        _fsync_dir(path.parent)
    except BaseException:
        # Deliberately retain even a partial fence. Its existence blocks replay and requires a
        # non-network investigation; deleting it would make an ambiguous paid attempt repeatable.
        raise


def _private_directory(path: Path) -> None:
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise AdmissionError("private evidence directory is unavailable") from error
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISDIR(info.st_mode):
            raise AdmissionError("private evidence path is not a real directory")
        if stat.S_IMODE(info.st_mode) != 0o700:
            raise AdmissionError("private evidence directory must have mode 0700")
    finally:
        os.close(descriptor)


def _lock_path(directory: Path) -> Path:
    try:
        parent = directory.parent.resolve(strict=True)
    except OSError as error:
        raise AdmissionError("private evidence parent is unavailable") from error
    if not directory.name or directory.name in {".", ".."}:
        raise AdmissionError("private evidence directory name is invalid")
    return parent / f".{directory.name}.gemini-3.7-admission.lock"


@contextlib.contextmanager
def _state_lock(directory: Path, *, exclusive: bool):
    """Serialize a complete state transition across processes.

    The sibling lock exists before the evidence directory, so initialization is covered too. It is
    deliberately retained: removing and recreating a lock inode would let two controllers hold
    different locks for the same one-shot.
    """

    flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(_lock_path(directory), flags, 0o600)
    except OSError as error:
        raise AdmissionError("one-shot state lock is unavailable") from error
    try:
        os.fchmod(descriptor, 0o600)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise AdmissionError("one-shot state lock is not a private regular file")
        fcntl.flock(descriptor, fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH)
        locked = os.fstat(descriptor)
        if (
            locked.st_dev != info.st_dev
            or locked.st_ino != info.st_ino
            or locked.st_nlink != 1
            or stat.S_IMODE(locked.st_mode) != 0o600
        ):
            raise AdmissionError("one-shot state lock changed while it was acquired")
        yield descriptor
    finally:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        finally:
            os.close(descriptor)


def _validate_entries(directory: Path) -> None:
    unexpected = {entry.name for entry in directory.iterdir()} - KNOWN_ENTRIES
    if unexpected:
        raise AdmissionError("private evidence directory contains unexpected artifacts")


def _sha(value: str, field: str) -> str:
    if not SHA_RE.fullmatch(value):
        raise AdmissionError(f"{field} must be an exact lowercase 40-hex SHA")
    return value


def _positive_int(value: Any, field: str, maximum: int | None = None) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise AdmissionError(f"{field} must be a positive integer")
    if maximum is not None and value > maximum:
        raise AdmissionError(f"{field} exceeds its fixed ceiling")
    return value


def _canonical_positive_event_int(value: Any, field: str) -> int:
    if isinstance(value, bool):
        raise AdmissionError(f"{field} must be a canonical positive integer")
    if isinstance(value, int):
        parsed = value
    elif isinstance(value, str) and re.fullmatch(r"[1-9][0-9]*", value):
        parsed = int(value)
    else:
        raise AdmissionError(f"{field} must be a canonical positive integer")
    if parsed <= 0:
        raise AdmissionError(f"{field} must be a canonical positive integer")
    return parsed


def _require_promo_not_after(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value != PROMO_END_EPOCH:
        raise AdmissionError(f"{field} must be the exact promotional dispatch cutoff")
    return value


def _nonnegative_int(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise AdmissionError(f"{field} must be a non-negative integer")
    return value


def _expected_official_rate_at(rate_epoch: int) -> run_live.ModelRates:
    """Return the exact official standard paid-tier rate effective at a UTC epoch."""

    rate_epoch = _nonnegative_int(rate_epoch, "rate epoch")
    if rate_epoch < PROMO_END_EPOCH:
        input_rate, cached_rate, output_rate = 750, 75, 3_750
    else:
        input_rate, cached_rate, output_rate = 1_500, 150, 7_500
    return run_live.ModelRates(
        tariff_schedule_id=OFFICIAL_TARIFF_SCHEDULE_ID,
        input_token_limit=OFFICIAL_INPUT_TOKEN_LIMIT,
        input=input_rate,
        audio_input=input_rate,
        cached_input=cached_rate,
        cached_audio_input=cached_rate,
        output=output_rate,
        image_output=0,
        long_threshold=UINT64_MAX,
        long_input=input_rate,
        long_audio_input=input_rate,
        long_cached_input=cached_rate,
        long_cached_audio_input=cached_rate,
        long_output=output_rate,
        search_unit="query",
        search=OFFICIAL_SEARCH_NANOUSD_PER_QUERY,
        max_output_tokens=OFFICIAL_OUTPUT_TOKEN_LIMIT,
    )


def _require_exact_official_rate(
    rate: run_live.ModelRates,
    rate_epoch: int,
) -> run_live.ModelRates:
    expected = _expected_official_rate_at(rate_epoch)
    if rate != expected:
        raise AdmissionError(
            "Gemini 3.7 Flash rate row does not exactly match the official effective tariff"
        )
    return rate


def _validate_admission_controls(
    rate_epoch: int,
    budget_nanousd: Any,
    max_output_tokens: Any,
) -> tuple[int, int]:
    rate_epoch = _nonnegative_int(rate_epoch, "rate epoch")
    if rate_epoch >= PROMO_END_EPOCH:
        raise AdmissionError(
            "Gemini 3.7 Flash post-promotion admission requires a new explicit contract"
        )
    budget = _positive_int(
        budget_nanousd,
        "budget",
        AUTHORIZED_PROMO_BUDGET_NANOUSD,
    )
    output = _positive_int(
        max_output_tokens,
        "max output tokens",
        AUTHORIZED_MAX_OUTPUT_TOKENS,
    )
    return budget, output


def _profile_from_file(path: Path) -> str:
    try:
        value = _read_regular_bytes(
            path,
            maximum=256,
            required_mode=0o600,
        ).decode("utf-8").strip()
    except (AdmissionError, UnicodeError) as error:
        raise AdmissionError("opaque profile source is unavailable") from error
    if not OPAQUE_PROFILE_RE.fullmatch(value):
        raise AdmissionError("opaque profile source is invalid")
    return value


def _count_request(journal: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": "gemini-3.7-admission-request/v2",
        "kind": "count_tokens",
        "method": "POST",
        "path": f"/v1beta/models/{MODEL}:countTokens",
        "target_profile": journal["profile_id"],
        "request_id": journal["count_request_id"],
        "body": {
            "contents": [
                {"role": "user", "parts": [{"text": "Reply with exactly OK."}]}
            ]
        },
    }


def _generation_request(journal: dict[str, Any]) -> dict[str, Any]:
    suffix = "streamGenerateContent?alt=sse" if journal["stream"] else "generateContent"
    return {
        "schema": "gemini-3.7-admission-request/v2",
        "kind": "generation",
        "method": "POST",
        "path": f"/v1beta/models/{MODEL}:{suffix}",
        "target_profile": journal["profile_id"],
        "calibration_request_id": journal["request_id"],
        "not_after": journal["not_after"],
        "body": {
            "contents": _count_request(journal)["body"]["contents"],
            "generationConfig": {
                "maxOutputTokens": journal["max_output_tokens"],
            },
        },
    }


def _canonical_digest(value: dict[str, Any]) -> str:
    return hashlib.sha256(_canonical_bytes(value)).hexdigest()


def _contract(journal: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": journal["schema"],
        "model": journal["model"],
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "profile_id": journal["profile_id"],
        "plan": journal["plan"],
        "stream": journal["stream"],
        "budget_nanousd": journal["budget_nanousd"],
        "max_output_tokens": journal["max_output_tokens"],
        "rate_epoch": journal["rate_epoch"],
        "not_after": journal["not_after"],
        "count_request_id": journal["count_request_id"],
        "count_request_sha256": journal["count_request_sha256"],
        "request_id": journal["request_id"],
        "generation_request_sha256": journal["generation_request_sha256"],
        "rate": journal["rate"],
    }


def _contract_digest(journal: dict[str, Any]) -> str:
    return hashlib.sha256(_canonical_bytes(_contract(journal))).hexdigest()


def _contract_fence(journal: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": "gemini-3.7-admission-contract-fence/v3",
        "contract_sha256": journal["contract_sha256"],
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "count_request_sha256": journal["count_request_sha256"],
        "generation_request_sha256": journal["generation_request_sha256"],
        "not_after": journal["not_after"],
    }


def _validate_contract_fence(directory: Path, journal: dict[str, Any]) -> None:
    fence = _read_json(directory / CONTRACT_FENCE, required_mode=0o600)
    if fence != _contract_fence(journal):
        raise AdmissionError("immutable admission contract fence does not match")


def _validate_canonical_request(
    path: Path,
    expected: dict[str, Any],
    expected_digest: str,
    label: str,
) -> None:
    observed, raw_digest = _read_json_with_digest(path, required_mode=0o600)
    if observed != expected or raw_digest != expected_digest:
        raise AdmissionError(f"private {label} request does not match the immutable contract")


def _rates_from_dict(value: Any) -> run_live.ModelRates:
    expected = {field.name for field in dataclasses.fields(run_live.ModelRates)}
    if not isinstance(value, dict) or set(value) != expected:
        raise AdmissionError("private journal rate card has an unexpected schema")
    parsed: dict[str, Any] = {}
    for field in dataclasses.fields(run_live.ModelRates):
        item = value[field.name]
        if field.name in {"tariff_schedule_id", "search_unit"}:
            if not isinstance(item, str) or not item or len(item) > 256:
                raise AdmissionError("private journal rate identity is invalid")
            parsed[field.name] = item
            continue
        if isinstance(item, bool) or not isinstance(item, int) or item < 0:
            raise AdmissionError("private journal rate value is invalid")
        parsed[field.name] = item
    if (
        parsed["input_token_limit"] <= 0
        or parsed["max_output_tokens"] <= 0
        or parsed["input"] <= 0
        or parsed["output"] <= 0
    ):
        raise AdmissionError("private journal rate card has no positive text bounds")
    return run_live.ModelRates(**parsed)


def _request_digest(path: Path) -> str:
    data = _read_regular_bytes(path, required_mode=0o600)
    return hashlib.sha256(data).hexdigest()


def _load_journal(directory: Path) -> dict[str, Any]:
    _private_directory(directory)
    _validate_entries(directory)
    journal_path = directory / JOURNAL
    journal = _read_json(journal_path, required_mode=0o600)
    required = {
        "schema",
        "state",
        "model",
        "implementation_sha",
        "release_sha",
        "contract_sha256",
        "profile_id",
        "plan",
        "stream",
        "budget_nanousd",
        "max_output_tokens",
        "rate_epoch",
        "not_after",
        "count_request_id",
        "count_request_sha256",
        "request_id",
        "rate",
        "counted_input_tokens",
        "upper_bound_nanousd",
        "generation_request_sha256",
        "actual_nanousd",
        "failure_class",
        "http_status",
        "execution_state",
        "evidence",
    }
    if set(journal) != required:
        raise AdmissionError("private journal has an unexpected schema")
    if journal["schema"] != SCHEMA or journal["model"] != MODEL:
        raise AdmissionError("private journal is not the exact Gemini 3.7 admission contract")
    implementation = _sha(journal["implementation_sha"], "implementation SHA")
    release = _sha(journal["release_sha"], "release SHA")
    if implementation != release:
        raise AdmissionError("implementation and immutable release SHA differ")
    if journal["contract_sha256"] != _contract_digest(journal):
        raise AdmissionError("private journal contract digest does not match")
    _validate_contract_fence(directory, journal)
    if not OPAQUE_PROFILE_RE.fullmatch(journal["profile_id"]):
        raise AdmissionError("private journal has an invalid opaque profile")
    if journal["plan"] not in PAID_PLANS:
        raise AdmissionError("private journal has an invalid paid plan")
    if not isinstance(journal["budget_nanousd"], str) or not journal["budget_nanousd"].isdigit():
        raise AdmissionError("private journal budget is invalid")
    budget, max_output_tokens = _validate_admission_controls(
        journal["rate_epoch"],
        int(journal["budget_nanousd"]),
        journal["max_output_tokens"],
    )
    not_after = _require_promo_not_after(journal["not_after"], "private journal not_after")
    if journal["rate_epoch"] >= not_after:
        raise AdmissionError("private journal has an invalid promotional dispatch cutoff")
    if (
        str(budget) != journal["budget_nanousd"]
        or max_output_tokens != journal["max_output_tokens"]
    ):
        raise AdmissionError("private journal admission controls are non-canonical")
    for field in ("count_request_id", "request_id"):
        try:
            parsed_request_id = uuid.UUID(journal[field])
        except (ValueError, TypeError, AttributeError) as error:
            raise AdmissionError("private journal has an invalid request identity") from error
        if parsed_request_id.version != 4 or str(parsed_request_id) != journal[field]:
            raise AdmissionError("private journal has a non-canonical request identity")
    if journal["count_request_id"] == journal["request_id"]:
        raise AdmissionError("free and paid request identities must differ")
    allowed_states = {
        "awaiting_count_tokens",
        "count_tokens_claimed",
        "counted",
        "generation_armed",
        "generation_claimed",
        "success",
        "withdrawn_count_tokens",
        "withdrawn_budget",
        "withdrawn_contract_expired",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    }
    if journal["state"] not in allowed_states:
        raise AdmissionError("private journal has an invalid state")
    for field in ("counted_input_tokens", "upper_bound_nanousd", "actual_nanousd"):
        value = journal[field]
        if value is not None and (
            isinstance(value, bool)
            or not isinstance(value, (int, str))
            or not str(value).isdigit()
        ):
            raise AdmissionError(f"private journal {field} is invalid")
    for field in (
        "count_request_sha256",
        "generation_request_sha256",
        "contract_sha256",
    ):
        value = journal[field]
        if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise AdmissionError(f"private journal {field} is invalid")
    if journal["http_status"] is not None and (
        isinstance(journal["http_status"], bool)
        or not isinstance(journal["http_status"], int)
        or not 0 <= journal["http_status"] <= 599
    ):
        raise AdmissionError("private journal HTTP status is invalid")
    if journal["execution_state"] not in {
        None,
        "not_started",
        "started",
        "completed",
        "unknown",
    }:
        raise AdmissionError("private journal execution state is invalid")
    if journal["failure_class"] is not None and (
        not isinstance(journal["failure_class"], str)
        or not re.fullmatch(r"[a-z0-9_]{1,64}", journal["failure_class"])
    ):
        raise AdmissionError("private journal failure class is invalid")
    if not isinstance(journal["rate"], dict):
        raise AdmissionError("private journal has no exact rate card")
    _require_exact_official_rate(
        _rates_from_dict(journal["rate"]),
        journal["rate_epoch"],
    )
    _validate_canonical_request(
        directory / COUNT_REQUEST,
        _count_request(journal),
        journal["count_request_sha256"],
        "countTokens",
    )
    if (directory / GENERATION_REQUEST).exists():
        _validate_canonical_request(
            directory / GENERATION_REQUEST,
            _generation_request(journal),
            journal["generation_request_sha256"],
            "generation",
        )
    return journal


def _validate_count_receipt(directory: Path, journal: dict[str, Any]) -> None:
    receipt = _read_json(directory / COUNT_RECEIPT, required_mode=0o600)
    expected = {
        "schema": "gemini-3.7-admission-count-receipt/v3",
        "request_id": journal["count_request_id"],
        "request_sha256": journal["count_request_sha256"],
        "target_profile": journal["profile_id"],
        "model": MODEL,
        "total_tokens": int(journal["counted_input_tokens"]),
    }
    if receipt != expected:
        raise AdmissionError("immutable countTokens receipt does not match the contract")


def _count_outcome_receipt(journal: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": "gemini-3.7-admission-count-outcome-receipt/v1",
        "state": journal["state"],
        "request_id": journal["count_request_id"],
        "request_sha256": journal["count_request_sha256"],
        "failure_class": journal["failure_class"],
        "http_status": journal["http_status"],
        "execution_state": journal["execution_state"],
    }


def _validate_count_outcome_receipt(directory: Path, journal: dict[str, Any]) -> None:
    receipt = _read_json(directory / COUNT_OUTCOME_RECEIPT, required_mode=0o600)
    if receipt != _count_outcome_receipt(journal):
        raise AdmissionError("immutable countTokens terminal receipt does not match the journal")


def _outcome_receipt(journal: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": "gemini-3.7-admission-outcome-receipt/v2",
        "state": journal["state"],
        "request_id": journal["request_id"],
        "request_sha256": journal["generation_request_sha256"],
        "target_profile": journal["profile_id"],
        "plan": journal["plan"],
        "actual_nanousd": journal["actual_nanousd"],
        "failure_class": journal["failure_class"],
        "http_status": journal["http_status"],
        "execution_state": journal["execution_state"],
    }


def _validate_outcome_receipt(directory: Path, journal: dict[str, Any]) -> None:
    receipt = _read_json(directory / OUTCOME_RECEIPT, required_mode=0o600)
    if receipt != _outcome_receipt(journal):
        raise AdmissionError("immutable terminal outcome receipt does not match the journal")


def _write_journal(directory: Path, journal: dict[str, Any]) -> None:
    _atomic_json(directory / JOURNAL, journal, replace=True)


def _terminalize(
    directory: Path,
    journal: dict[str, Any],
    state: str,
    failure_class: str,
    *,
    http_status: int | None = None,
    execution_state: str | None = None,
) -> None:
    journal["state"] = state
    journal["failure_class"] = failure_class
    journal["http_status"] = http_status
    journal["execution_state"] = execution_state
    journal["evidence"] = None
    _write_journal(directory, journal)
    if state in {
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    }:
        _exclusive_json(directory / OUTCOME_RECEIPT, _outcome_receipt(journal))
    elif state == "withdrawn_count_tokens":
        _exclusive_json(directory / COUNT_OUTCOME_RECEIPT, _count_outcome_receipt(journal))


def _expire_before_dispatch(directory: Path, journal: dict[str, Any]) -> None:
    if int(time.time()) < journal["not_after"]:
        return
    _terminalize(
        directory,
        journal,
        "withdrawn_contract_expired",
        "promo_admission_contract_expired",
    )
    raise AdmissionError(
        "Gemini 3.7 Flash promotional admission contract expired before dispatch"
    )


def initialize(
    directory: Path,
    capacity_path: Path,
    profile_path: Path,
    plan: str,
    implementation_sha: str,
    release_sha: str,
    budget_nanousd: int = DEFAULT_BUDGET_NANOUSD,
    max_output_tokens: int = DEFAULT_MAX_OUTPUT_TOKENS,
    stream: bool = False,
) -> dict[str, Any]:
    with _state_lock(directory, exclusive=True):
        return _initialize_locked(
            directory,
            capacity_path,
            profile_path,
            plan,
            implementation_sha,
            release_sha,
            budget_nanousd,
            max_output_tokens,
            stream,
        )


def _initialize_locked(
    directory: Path,
    capacity_path: Path,
    profile_path: Path,
    plan: str,
    implementation_sha: str,
    release_sha: str,
    budget_nanousd: int,
    max_output_tokens: int,
    stream: bool,
) -> dict[str, Any]:
    implementation_sha = _sha(implementation_sha, "implementation SHA")
    release_sha = _sha(release_sha, "release SHA")
    if implementation_sha != release_sha:
        raise AdmissionError("implementation and immutable release SHA must match")
    rate_epoch = int(time.time())
    budget_nanousd, max_output_tokens = _validate_admission_controls(
        rate_epoch,
        budget_nanousd,
        max_output_tokens,
    )
    if plan not in PAID_PLANS:
        raise AdmissionError("paid plan is not an admitted Gemini subscription plan")
    profile_id = _profile_from_file(profile_path)
    capacity = _read_json(capacity_path)
    try:
        run_live.require_healthy_delivery(capacity)
        states = run_live.profile_state(capacity)
        rates = run_live.rate_catalog(capacity)
        baseline_events = run_live.recent_turn_events(capacity)
    except run_live.CalibrationError as error:
        raise AdmissionError("Gemini immutable authority preflight is invalid") from error
    selected = states.get(profile_id)
    selected_rows = [
        item
        for item in capacity.get("profiles", [])
        if isinstance(item, dict) and item.get("id") == profile_id
    ]
    model_rows = [
        item
        for item in capacity.get("conversion_models", [])
        if isinstance(item, dict) and item.get("id") == MODEL
    ]
    if (
        len(selected_rows) != 1
        or selected is None
        or selected.get("plan") != plan
        or selected.get("authenticated") is not True
        or selected.get("persistence_ok") is not True
        or selected.get("cooling_until", rate_epoch + 1) > rate_epoch
    ):
        raise AdmissionError(
            "the explicitly selected paid profile is not admitted on its exact plan"
        )
    if len(model_rows) != 1:
        raise AdmissionError("Gemini 3.7 Flash must have exactly one authoritative rate row")
    rate = rates.get(MODEL)
    if rate is None:
        raise AdmissionError("Gemini 3.7 Flash has no exact authoritative rate card")
    _require_exact_official_rate(rate, rate_epoch)
    count_request_id = str(uuid.uuid4())
    request_id = str(uuid.uuid4())
    if count_request_id in baseline_events or request_id in baseline_events:
        raise AdmissionError("fresh admission request identity already exists")
    try:
        directory.mkdir(mode=0o700)
    except FileExistsError as error:
        raise AdmissionError("one-shot evidence directory already exists") from error
    except OSError as error:
        raise AdmissionError("one-shot evidence directory could not be created") from error
    os.chmod(directory, 0o700, follow_symlinks=False)
    journal: dict[str, Any] = {
        "schema": SCHEMA,
        "state": "awaiting_count_tokens",
        "model": MODEL,
        "implementation_sha": implementation_sha,
        "release_sha": release_sha,
        "contract_sha256": "",
        "profile_id": profile_id,
        "plan": plan,
        "stream": bool(stream),
        "budget_nanousd": str(budget_nanousd),
        "max_output_tokens": max_output_tokens,
        "rate_epoch": rate_epoch,
        "not_after": PROMO_END_EPOCH,
        "count_request_id": count_request_id,
        "count_request_sha256": "",
        "request_id": request_id,
        "rate": dataclasses.asdict(rate),
        "counted_input_tokens": None,
        "upper_bound_nanousd": None,
        "generation_request_sha256": "",
        "actual_nanousd": None,
        "failure_class": None,
        "http_status": None,
        "execution_state": None,
        "evidence": None,
    }
    journal["count_request_sha256"] = _canonical_digest(_count_request(journal))
    journal["generation_request_sha256"] = _canonical_digest(_generation_request(journal))
    journal["contract_sha256"] = _contract_digest(journal)
    try:
        _atomic_json(directory / COUNT_REQUEST, _count_request(journal), replace=False)
        _exclusive_json(directory / CONTRACT_FENCE, _contract_fence(journal))
        _atomic_json(directory / JOURNAL, journal, replace=False)
    except BaseException:
        # Directory presence itself is a permanent no-paid-dispatch fence. Never remove a partial
        # initialization automatically because a concurrent controller may already have observed it.
        raise
    return _inspect_locked(directory)


def record_count(directory: Path, response_path: Path) -> dict[str, Any]:
    with _state_lock(directory, exclusive=True):
        return _record_count_locked(directory, response_path)


def _record_count_locked(directory: Path, response_path: Path) -> dict[str, Any]:
    journal = _load_journal(directory)
    if journal["state"] != "count_tokens_claimed":
        raise AdmissionError("free countTokens outcome is already terminal or recorded")
    _validate_count_claim(directory, journal)
    try:
        response = _read_json(response_path)
        if set(response) != {
            "schema",
            "request_id",
            "request_sha256",
            "target_profile",
            "model",
            "http_status",
            "execution_state",
            "response",
        }:
            raise AdmissionError("countTokens observation has an unexpected schema")
        if response != {
            "schema": COUNT_OBSERVATION_SCHEMA,
            "request_id": journal["count_request_id"],
            "request_sha256": journal["count_request_sha256"],
            "target_profile": journal["profile_id"],
            "model": MODEL,
            "http_status": response.get("http_status"),
            "execution_state": response.get("execution_state"),
            "response": response.get("response"),
        }:
            raise AdmissionError("countTokens observation is not bound to the immutable request")
        status = response["http_status"]
        execution_state = response["execution_state"]
        if (
            isinstance(status, bool)
            or not isinstance(status, int)
            or not 0 <= status <= 599
            or execution_state not in {
                None,
                "not_started",
                "started",
                "completed",
                "unknown",
            }
        ):
            raise AdmissionError("countTokens transport outcome is invalid")
        if status != 200:
            if execution_state == "completed":
                raise AdmissionError(
                    "failed countTokens contradicts completed execution evidence"
                )
            _terminalize(
                directory,
                journal,
                "withdrawn_count_tokens",
                "count_tokens_failed_no_retry",
                http_status=status,
                execution_state=execution_state,
            )
            raise AdmissionError(
                "free countTokens failed; the exact attempt is permanently withdrawn"
            )
        if execution_state != "completed":
            raise AdmissionError(
                "successful countTokens has no completed execution evidence"
            )
        raw_response = response["response"]
        if not isinstance(raw_response, dict):
            raise AdmissionError("countTokens observation has no response object")
        total_tokens = run_live.as_int(
            raw_response.get("totalTokens"),
            "countTokens.totalTokens",
        )
        if total_tokens <= 0:
            raise run_live.CalibrationError("countTokens returned no positive input")
        rate = _rates_from_dict(journal["rate"])
        upper = rate.upper_bound(
            total_tokens,
            journal["max_output_tokens"],
            "fresh",
        )
    except (AdmissionError, run_live.CalibrationError, run_live.UnboundedCostError, TypeError):
        refreshed = _load_journal(directory)
        if refreshed["state"] == "count_tokens_claimed":
            _terminalize(
                directory,
                refreshed,
                "withdrawn_count_tokens",
                "count_tokens_invalid",
            )
        raise AdmissionError("free countTokens evidence was rejected; paid dispatch remains closed")
    journal["counted_input_tokens"] = total_tokens
    journal["upper_bound_nanousd"] = str(upper)
    budget = int(journal["budget_nanousd"])
    if upper <= 0 or upper > budget:
        _terminalize(directory, journal, "withdrawn_budget", "pre_dispatch_bound_exceeds_budget")
        raise AdmissionError(
            "worst-case Gemini cost bound exceeds the immutable aggregate ceiling; no paid dispatch"
        )
    receipt = {
        "schema": "gemini-3.7-admission-count-receipt/v3",
        "request_id": journal["count_request_id"],
        "request_sha256": journal["count_request_sha256"],
        "target_profile": journal["profile_id"],
        "model": MODEL,
        "total_tokens": total_tokens,
    }
    _atomic_json(directory / GENERATION_REQUEST, _generation_request(journal), replace=False)
    journal["state"] = "counted"
    _write_journal(directory, journal)
    _exclusive_json(directory / COUNT_RECEIPT, receipt)
    return _inspect_locked(directory)


def claim_count(directory: Path) -> dict[str, Any]:
    """Irreversibly consume the one free countTokens dispatch before transport opens."""

    with _state_lock(directory, exclusive=True):
        journal = _load_journal(directory)
        if (directory / COUNT_DISPATCH_CLAIM).exists() or (
            directory / COUNT_DISPATCH_CLAIM
        ).is_symlink():
            raise AdmissionError("countTokens dispatch was already permanently claimed")
        if journal["state"] != "awaiting_count_tokens":
            raise AdmissionError("countTokens cannot be claimed from the current state")
        _expire_before_dispatch(directory, journal)
        _validate_canonical_request(
            directory / COUNT_REQUEST,
            _count_request(journal),
            journal["count_request_sha256"],
            "countTokens",
        )
        claim = {
            "schema": "gemini-3.7-admission-count-dispatch-claim/v1",
            "model": MODEL,
            "implementation_sha": journal["implementation_sha"],
            "release_sha": journal["release_sha"],
            "contract_sha256": journal["contract_sha256"],
            "request_sha256": journal["count_request_sha256"],
            "request_id": journal["count_request_id"],
            "target_profile": journal["profile_id"],
            "plan": journal["plan"],
            "not_after": journal["not_after"],
        }
        _exclusive_json(directory / COUNT_DISPATCH_CLAIM, claim)
        # Persisting the claim is the irreversible dispatch boundary. A transport must perform its
        # own final UTC comparison immediately after this call, then open exactly one connection.
        # A crash or cutoff crossed after this fsync remains a consumed, ambiguous one-shot.
        journal["state"] = "count_tokens_claimed"
        _write_journal(directory, journal)
        return _inspect_locked(directory)


def _validate_count_claim(directory: Path, journal: dict[str, Any]) -> None:
    claim = _read_json(directory / COUNT_DISPATCH_CLAIM, required_mode=0o600)
    expected = {
        "schema": "gemini-3.7-admission-count-dispatch-claim/v1",
        "model": MODEL,
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "contract_sha256": journal["contract_sha256"],
        "request_sha256": journal["count_request_sha256"],
        "request_id": journal["count_request_id"],
        "target_profile": journal["profile_id"],
        "plan": journal["plan"],
        "not_after": journal["not_after"],
    }
    if claim != expected:
        raise AdmissionError("countTokens dispatch claim does not match the contract")


def arm_generation(directory: Path) -> dict[str, Any]:
    with _state_lock(directory, exclusive=True):
        return _arm_generation_locked(directory)


def _arm_generation_locked(directory: Path) -> dict[str, Any]:
    journal = _load_journal(directory)
    if (directory / DISPATCH_FENCE).exists() or (directory / DISPATCH_FENCE).is_symlink():
        raise AdmissionError("paid generation is already permanently fenced")
    if journal["state"] != "counted":
        raise AdmissionError("paid generation cannot be armed before a bounded free countTokens")
    _expire_before_dispatch(directory, journal)
    _validate_canonical_request(
        directory / GENERATION_REQUEST,
        _generation_request(journal),
        journal["generation_request_sha256"],
        "generation",
    )
    generation_request = _read_json(directory / GENERATION_REQUEST, required_mode=0o600)
    request_not_after = _require_promo_not_after(
        generation_request.get("not_after"),
        "private generation request not_after",
    )
    if request_not_after != journal["not_after"]:
        raise AdmissionError("private generation request cutoff differs from the contract")
    fence = {
        "schema": "gemini-3.7-admission-dispatch-fence/v1",
        "model": MODEL,
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "contract_sha256": journal["contract_sha256"],
        "generation_request_sha256": journal["generation_request_sha256"],
        "request_id": journal["request_id"],
        "not_after": journal["not_after"],
    }
    _exclusive_json(directory / DISPATCH_FENCE, fence)
    journal["state"] = "generation_armed"
    _write_journal(directory, journal)
    return _inspect_locked(directory)


def claim_generation(directory: Path) -> dict[str, Any]:
    """Irreversibly consume the one paid dispatch immediately before transport opens.

    A root controller must call this only after readiness and token acquisition, then perform its
    own final UTC cutoff comparison and open the provider connection without another preparatory
    step. The append-only claim is retained even if this process crashes before journal replacement.
    """

    with _state_lock(directory, exclusive=True):
        journal = _load_journal(directory)
        if (directory / DISPATCH_CLAIM).exists() or (directory / DISPATCH_CLAIM).is_symlink():
            raise AdmissionError("paid generation dispatch was already permanently claimed")
        if journal["state"] != "generation_armed":
            raise AdmissionError("paid generation cannot be claimed before irreversible arming")
        _expire_before_dispatch(directory, journal)
        _validate_fence(directory, journal)
        _validate_count_receipt(directory, journal)
        claim = {
            "schema": "gemini-3.7-admission-dispatch-claim/v2",
            "model": MODEL,
            "implementation_sha": journal["implementation_sha"],
            "release_sha": journal["release_sha"],
            "contract_sha256": journal["contract_sha256"],
            "generation_request_sha256": journal["generation_request_sha256"],
            "request_id": journal["request_id"],
            "target_profile": journal["profile_id"],
            "plan": journal["plan"],
            "not_after": journal["not_after"],
        }
        _exclusive_json(directory / DISPATCH_CLAIM, claim)
        journal["state"] = "generation_claimed"
        _write_journal(directory, journal)
        return _inspect_locked(directory)


def _validate_fence(directory: Path, journal: dict[str, Any]) -> None:
    fence = _read_json(directory / DISPATCH_FENCE, required_mode=0o600)
    if set(fence) != {
        "schema",
        "model",
        "implementation_sha",
        "release_sha",
        "contract_sha256",
        "generation_request_sha256",
        "request_id",
        "not_after",
    }:
        raise AdmissionError("paid dispatch fence has an unexpected schema")
    fence_not_after = _require_promo_not_after(
        fence.get("not_after"),
        "paid dispatch fence not_after",
    )
    if fence_not_after != journal["not_after"]:
        raise AdmissionError("paid dispatch fence cutoff differs from the contract")
    if fence != {
        "schema": "gemini-3.7-admission-dispatch-fence/v1",
        "model": MODEL,
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "contract_sha256": journal["contract_sha256"],
        "generation_request_sha256": journal["generation_request_sha256"],
        "request_id": journal["request_id"],
        "not_after": journal["not_after"],
    }:
        raise AdmissionError("paid dispatch fence does not match the immutable contract")
    generation_request = _read_json(
        directory / GENERATION_REQUEST,
        required_mode=0o600,
    )
    request_not_after = _require_promo_not_after(
        generation_request.get("not_after"),
        "fenced generation request not_after",
    )
    if request_not_after != journal["not_after"]:
        raise AdmissionError("fenced generation request cutoff differs from the contract")
    if _request_digest(directory / GENERATION_REQUEST) != journal["generation_request_sha256"]:
        raise AdmissionError("fenced generation request digest does not match")


def _validate_claim(directory: Path, journal: dict[str, Any]) -> None:
    claim = _read_json(directory / DISPATCH_CLAIM, required_mode=0o600)
    expected = {
        "schema": "gemini-3.7-admission-dispatch-claim/v2",
        "model": MODEL,
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "contract_sha256": journal["contract_sha256"],
        "generation_request_sha256": journal["generation_request_sha256"],
        "request_id": journal["request_id"],
        "target_profile": journal["profile_id"],
        "plan": journal["plan"],
        "not_after": journal["not_after"],
    }
    if claim != expected:
        raise AdmissionError("paid dispatch claim does not match the immutable contract")


def _response(observation: dict[str, Any], stream: bool) -> run_live.GenerationResponse:
    raw = observation.get("response")
    if stream:
        if (
            not isinstance(raw, list)
            or not raw
            or not all(isinstance(frame, dict) for frame in raw)
        ):
            raise AdmissionError("stream observation has no bounded frame array")
        return run_live.GenerationResponse(tuple(raw), stream=True)
    if not isinstance(raw, dict):
        raise AdmissionError("generation observation has no response object")
    return run_live.GenerationResponse((raw,), stream=False)


def _verify_fresh_event_cost(
    event: dict[str, Any],
    rate: run_live.ModelRates,
    max_output_tokens: int,
) -> None:
    # This admission is intentionally a plain text turn. Accepting an unrequested cache, audio,
    # image, tool, or search class would make the response/price proof incomplete.
    zero_token_fields = (
        "audio_input_tokens",
        "cache_read_tokens",
        "cached_audio_input_tokens",
        "cache_write_5m_tokens",
        "cache_write_1h_tokens",
        "image_output_tokens",
        "tool_prompt_tokens",
        "search_queries",
        "grounded_search_prompts",
    )
    zero_money_fields = (
        "api_audio_input_nanousd",
        "api_cache_read_nanousd",
        "api_cached_audio_input_nanousd",
        "api_cache_write_5m_nanousd",
        "api_cache_write_1h_nanousd",
        "api_image_output_nanousd",
        "api_search_nanousd",
    )
    if any(event[field] != 0 for field in zero_token_fields + zero_money_fields):
        raise AdmissionError("immutable event contains an unrequested token or money class")
    long = event["input_tokens"] > rate.long_threshold
    input_rate = rate.long_input if long else rate.input
    output_rate = rate.long_output if long else rate.output
    if (
        event["input_tokens"] <= 0
        or event["output_tokens"] <= 0
        or event["input_tokens"] > rate.input_token_limit
        or event["output_tokens"] > max_output_tokens
        or event["api_input_nanousd"] != event["input_tokens"] * input_rate
        or event["api_output_nanousd"] != event["output_tokens"] * output_rate
        or event["api_total_nanousd"]
        != event["api_input_nanousd"] + event["api_output_nanousd"]
    ):
        raise AdmissionError("immutable usage and exact rate card do not reproduce the cost")


def record_outcome(directory: Path, observation_path: Path) -> dict[str, Any]:
    with _state_lock(directory, exclusive=True):
        return _record_outcome_locked(directory, observation_path)


def _record_outcome_locked(directory: Path, observation_path: Path) -> dict[str, Any]:
    journal = _load_journal(directory)
    if journal["state"] != "generation_claimed":
        raise AdmissionError("paid generation outcome is already terminal or was never dispatched")
    _validate_fence(directory, journal)
    _validate_claim(directory, journal)
    try:
        observation = _read_json(observation_path)
        if observation.get("schema") != OBSERVATION_SCHEMA:
            raise AdmissionError("generation observation schema is invalid")
        if set(observation) != {
            "schema",
            "request_id",
            "request_sha256",
            "target_profile",
            "plan",
            "http_status",
            "execution_state",
            "response",
            "immutable_capacity",
            "event_request_id",
            "event_plan",
        }:
            raise AdmissionError("generation observation has an unexpected schema")
        if (
            observation.get("request_id") != journal["request_id"]
            or observation.get("request_sha256") != journal["generation_request_sha256"]
            or observation.get("target_profile") != journal["profile_id"]
            or observation.get("plan") != journal["plan"]
        ):
            raise AdmissionError("generation observation is not bound to the claimed request")
        status = observation.get("http_status")
        if isinstance(status, bool) or not isinstance(status, int) or not 0 <= status <= 599:
            raise AdmissionError("generation observation HTTP status is invalid")
        execution_state = observation.get("execution_state")
        if execution_state not in {None, "not_started", "started", "completed", "unknown"}:
            raise AdmissionError("generation execution state is invalid")
        if status != 200:
            state = (
                "withdrawn_generation_not_started"
                if execution_state == "not_started"
                else "withdrawn_generation_ambiguous"
            )
            _terminalize(
                directory,
                journal,
                state,
                "generation_failed_no_retry",
                http_status=status,
                execution_state=execution_state,
            )
            raise AdmissionError(
                "generation failed; the exact paid attempt is permanently withdrawn"
            )
        if execution_state == "not_started":
            raise AdmissionError("successful generation contradicts not_started execution evidence")
        event_payload = observation.get("immutable_capacity")
        if not isinstance(event_payload, dict):
            raise AdmissionError("generation has no immutable authority snapshot")
        try:
            run_live.require_healthy_delivery(event_payload)
        except run_live.CalibrationError as error:
            raise AdmissionError("generation immutable authority snapshot is unhealthy") from error
        event = run_live.exact_new_turn(
            set(),
            event_payload,
            journal["request_id"],
            journal["profile_id"],
            MODEL,
        )
        if event is None:
            raise AdmissionError("generation exact immutable event is absent")
        terminal_profiles = [
            item
            for item in event_payload.get("profiles", [])
            if isinstance(item, dict) and item.get("id") == journal["profile_id"]
        ]
        if len(terminal_profiles) != 1 or terminal_profiles[0].get("plan") != journal["plan"]:
            raise AdmissionError("generation authority snapshot has no exact contemporaneous plan")
        event_plan = observation.get("event_plan")
        if event_plan != journal["plan"]:
            raise AdmissionError("generation outcome has no exact immutable-event plan binding")
        if observation.get("event_request_id") != journal["request_id"]:
            raise AdmissionError("generation outcome plan proof is not bound to the immutable event")
        rate = _rates_from_dict(journal["rate"])
        if event.get("tariff_schedule_id") != rate.tariff_schedule_id:
            raise AdmissionError("generation tariff identity differs from the preflight")
        priced_ts = _canonical_positive_event_int(
            event.get("priced_ts"),
            "immutable event priced_ts",
        )
        completed_at = _canonical_positive_event_int(
            event.get("completed_at"),
            "immutable event completed_at",
        )
        if not journal["rate_epoch"] <= priced_ts < journal["not_after"]:
            raise AdmissionError("immutable event was priced outside the admission rate epoch")
        if completed_at < priced_ts:
            raise AdmissionError("immutable event completed before its pricing snapshot")
        _require_exact_official_rate(rate, priced_ts)
        _verify_fresh_event_cost(event, rate, journal["max_output_tokens"])
        actual = event["api_total_nanousd"]
        upper = int(journal["upper_bound_nanousd"])
        budget = int(journal["budget_nanousd"])
        if actual <= 0 or actual > upper or actual > budget:
            raise AdmissionError("immutable cost violates the pre-dispatch aggregate bound")
        leg = run_live.Leg(
            "admission:gemini-3.7-flash",
            MODEL,
            "fresh",
            stream=journal["stream"],
            max_output_tokens=journal["max_output_tokens"],
        )
        response_evidence, response_error = run_live.verify_generation_response(
            leg,
            _response(observation, journal["stream"]),
            event,
        )
        usage_error = run_live.verify_leg_usage(leg, event)
        if response_error or usage_error:
            raise AdmissionError("generation response and immutable event do not match")
        journal["state"] = "success"
        journal["actual_nanousd"] = str(actual)
        journal["failure_class"] = None
        journal["http_status"] = status
        journal["execution_state"] = execution_state
        journal["evidence"] = {
            "response": response_evidence,
            "usage": {field: str(event[field]) for field in run_live.EVENT_TOKEN_FIELDS},
            "api_cost": {field: str(event[field]) for field in run_live.EVENT_MONEY_FIELDS},
            "tariff_schedule_id": event["tariff_schedule_id"],
            "priced_ts": str(priced_ts),
            "completed_at": str(completed_at),
        }
        _write_journal(directory, journal)
        _exclusive_json(directory / OUTCOME_RECEIPT, _outcome_receipt(journal))
        return _inspect_locked(directory)
    except AdmissionError:
        refreshed = _load_journal(directory)
        if refreshed["state"] == "generation_claimed":
            _terminalize(
                directory,
                refreshed,
                "withdrawn_evidence",
                "generation_evidence_rejected",
            )
        raise
    except (run_live.CalibrationError, TypeError, ValueError, KeyError):
        refreshed = _load_journal(directory)
        if refreshed["state"] == "generation_claimed":
            _terminalize(
                directory,
                refreshed,
                "withdrawn_evidence",
                "generation_evidence_rejected",
            )
        raise AdmissionError("generation evidence was rejected; the exact attempt is fenced")


def inspect(directory: Path, *, require_success: bool = False) -> dict[str, Any]:
    with _state_lock(directory, exclusive=False):
        return _inspect_locked(directory, require_success=require_success)


def _inspect_locked(directory: Path, *, require_success: bool = False) -> dict[str, Any]:
    journal = _load_journal(directory)
    count_claimed = (directory / COUNT_DISPATCH_CLAIM).exists()
    if count_claimed:
        _validate_count_claim(directory, journal)
        if journal["state"] == "awaiting_count_tokens":
            raise AdmissionError(
                "countTokens claim exists without a completed journal transition"
            )
    if journal["state"] in {
        "count_tokens_claimed",
        "counted",
        "generation_armed",
        "generation_claimed",
        "success",
        "withdrawn_count_tokens",
        "withdrawn_budget",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    } and not count_claimed:
        raise AdmissionError("post-count state has no permanent countTokens claim")
    count_terminal = journal["state"] == "withdrawn_count_tokens"
    has_count_outcome = (directory / COUNT_OUTCOME_RECEIPT).exists()
    if has_count_outcome:
        _validate_count_outcome_receipt(directory, journal)
    if count_terminal != has_count_outcome:
        raise AdmissionError(
            "terminal countTokens state and immutable outcome receipt differ"
        )
    counted = journal["counted_input_tokens"] is not None
    has_count_receipt = (directory / COUNT_RECEIPT).exists()
    if has_count_receipt:
        _validate_count_receipt(directory, journal)
    receipt_required = journal["state"] in {
        "counted",
        "generation_armed",
        "generation_claimed",
        "success",
        "withdrawn_contract_expired",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    } and (directory / GENERATION_REQUEST).exists()
    if receipt_required != has_count_receipt:
        raise AdmissionError("counted state and immutable countTokens receipt differ")
    armed = (directory / DISPATCH_FENCE).exists()
    claimed = (directory / DISPATCH_CLAIM).exists()
    if armed:
        _validate_fence(directory, journal)
        if journal["state"] not in {
            "generation_armed",
            "generation_claimed",
            "success",
            "withdrawn_generation_not_started",
            "withdrawn_generation_ambiguous",
            "withdrawn_evidence",
        }:
            raise AdmissionError("paid dispatch fence exists without a completed arm transition")
    if journal["state"] in {
        "generation_armed",
        "generation_claimed",
        "success",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    } and not armed:
        raise AdmissionError("paid-state journal has no permanent dispatch fence")
    if claimed:
        _validate_claim(directory, journal)
        if journal["state"] not in {
            "generation_claimed",
            "success",
            "withdrawn_generation_not_started",
            "withdrawn_generation_ambiguous",
            "withdrawn_evidence",
        }:
            raise AdmissionError("paid dispatch claim exists without a claimed transition")
    if journal["state"] in {
        "generation_claimed",
        "success",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    } and not claimed:
        raise AdmissionError("dispatched state has no permanent single-use claim")
    terminal_paid = journal["state"] in {
        "success",
        "withdrawn_generation_not_started",
        "withdrawn_generation_ambiguous",
        "withdrawn_evidence",
    }
    has_outcome_receipt = (directory / OUTCOME_RECEIPT).exists()
    if has_outcome_receipt:
        _validate_outcome_receipt(directory, journal)
    if terminal_paid != has_outcome_receipt:
        raise AdmissionError("terminal paid state and immutable outcome receipt differ")
    summary = {
        "schema": SUMMARY_SCHEMA,
        "state": journal["state"],
        "model": MODEL,
        "implementation_sha": journal["implementation_sha"],
        "release_sha": journal["release_sha"],
        "count_tokens_claimed": count_claimed,
        "count_tokens_recorded": counted,
        "upper_bound_nanousd": journal["upper_bound_nanousd"],
        "budget_nanousd": journal["budget_nanousd"],
        "max_output_tokens": journal["max_output_tokens"],
        "rate_epoch": journal["rate_epoch"],
        "not_after": journal["not_after"],
        "generation_armed": armed,
        "generation_claimed": claimed,
        # Backward-compatible summary field: a claim is the only offline proof that transport was
        # authorized to make its exactly-once attempt; this state machine cannot observe socket I/O.
        "generation_dispatched": claimed,
        "actual_nanousd": journal["actual_nanousd"],
        "http_status": journal["http_status"],
        "execution_state": journal["execution_state"],
        "failure_class": journal["failure_class"],
        "response_evidence": (
            journal["evidence"]["response"]
            if isinstance(journal["evidence"], dict)
            else None
        ),
    }
    if require_success and journal["state"] != "success":
        raise AdmissionError("Gemini 3.7 admission has no exact terminal success evidence")
    return summary


def _print_summary(summary: dict[str, Any]) -> None:
    print(json.dumps(summary, sort_keys=True, separators=(",", ":")))


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init")
    init.add_argument("--evidence-dir", type=Path, required=True)
    init.add_argument("--capacity-file", type=Path, required=True)
    init.add_argument("--profile-id-file", type=Path, required=True)
    init.add_argument("--plan", choices=sorted(PAID_PLANS), required=True)
    init.add_argument("--implementation-sha", required=True)
    init.add_argument("--release-sha", required=True)
    init.add_argument("--budget-nanousd", type=int, default=DEFAULT_BUDGET_NANOUSD)
    init.add_argument("--max-output-tokens", type=int, default=DEFAULT_MAX_OUTPUT_TOKENS)
    init.add_argument("--stream", action="store_true")
    count = commands.add_parser("record-count")
    count.add_argument("--evidence-dir", type=Path, required=True)
    count.add_argument("--response-file", type=Path, required=True)
    claim_count_parser = commands.add_parser("claim-count")
    claim_count_parser.add_argument("--evidence-dir", type=Path, required=True)
    arm = commands.add_parser("arm-generation")
    arm.add_argument("--evidence-dir", type=Path, required=True)
    claim = commands.add_parser("claim-generation")
    claim.add_argument("--evidence-dir", type=Path, required=True)
    outcome = commands.add_parser("record-outcome")
    outcome.add_argument("--evidence-dir", type=Path, required=True)
    outcome.add_argument("--observation-file", type=Path, required=True)
    inspector = commands.add_parser("inspect")
    inspector.add_argument("--evidence-dir", type=Path, required=True)
    inspector.add_argument("--require-success", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.command == "init":
        summary = initialize(
            args.evidence_dir,
            args.capacity_file,
            args.profile_id_file,
            args.plan,
            args.implementation_sha,
            args.release_sha,
            args.budget_nanousd,
            args.max_output_tokens,
            args.stream,
        )
    elif args.command == "record-count":
        summary = record_count(args.evidence_dir, args.response_file)
    elif args.command == "claim-count":
        summary = claim_count(args.evidence_dir)
    elif args.command == "arm-generation":
        summary = arm_generation(args.evidence_dir)
    elif args.command == "claim-generation":
        summary = claim_generation(args.evidence_dir)
    elif args.command == "record-outcome":
        summary = record_outcome(args.evidence_dir, args.observation_file)
    else:
        summary = inspect(args.evidence_dir, require_success=args.require_success)
    _print_summary(summary)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AdmissionError as error:
        print(f"Gemini 3.7 admission stopped safely: {error}", file=sys.stderr)
        raise SystemExit(1)
