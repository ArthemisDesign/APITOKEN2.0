#!/usr/bin/env python3
"""Strict loopback transport for the one-shot Gemini 3.7 Flash admission.

The offline admission module owns the immutable contract and dispatch claims.  This controller
only moves those already-bound requests over one numeric-loopback HTTP connection per call.  It
does not use URL handlers, proxies, redirects, DNS, TLS, retries, or reconnects.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import hashlib
import http.client
import importlib
import json
import os
import re
import socket
import stat
import sys
import time
import uuid
from pathlib import Path
from types import ModuleType
from typing import Any


LOOPBACK_HOST = "127.0.0.1"
PRODUCTION_PORT = 8807
DEFAULT_LIBRARY_ROOT = Path("/usr/local/lib/apitoken-watchdog/controller")
CAPACITY_OUTPUT = "capacity.json"
COUNT_OUTPUT = "count-observation.json"
OUTCOME_OUTPUT = "outcome-observation.json"
ADMIN_KEY_ENV = "GEMINI_ADMISSION_ADMIN_KEY"
PANEL_KEY_ENV = "GEMINI_ADMISSION_PANEL_KEY"
TESTING_ENV = "GEMINI_ADMISSION_TESTING"
TEST_PORT_ENV = "GEMINI_ADMISSION_TEST_PORT"
MAX_CAPACITY_BYTES = 3 * 1024 * 1024
MAX_COUNT_BYTES = 64 * 1024
MAX_GENERATION_BYTES = 512 * 1024
MAX_OBSERVATION_BYTES = 4 * 1024 * 1024
PROFILE_RE = re.compile(r"^[A-Za-z0-9._-]{1,128}$")
UUID_V4_RE = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)
CANONICAL_POSITIVE_INTEGER_RE = re.compile(r"^[1-9][0-9]*$")
CALIBRATION_DISPATCH_HEADER = "x-apitoken-calibration-dispatch-ms"
EVENT_STREAM_CONTENT_TYPE_RE = re.compile(
    r'''^[ \t]*text/event-stream[ \t]*(?:;[ \t]*[!#$%&'*+\-.^_`|~0-9A-Za-z]+'''
    r'''[ \t]*=[ \t]*(?:[!#$%&'*+\-.^_`|~0-9A-Za-z]+|'''
    r'''"(?:[\t !#-\[\]-~]|\\[\t -~])*")[ \t]*)*$''',
    re.IGNORECASE,
)
ADMISSION_SHA256 = "4679ecfb90948c1ce658c647dbb2c91213b410b72ea3149886d0626b20aaf50d"
RUN_LIVE_SHA256 = "061340cbc323180469a5a4e6f10f70b370f53833d0ad583f325b3b9f7b49fdee"
PACKAGE_SHA256 = "cee5d8232c6da8fa74b0d01b3cfaab40709eed914594889ed68826ce6260a532"


class TransportError(RuntimeError):
    """A sanitized fail-closed transport error."""


@dataclasses.dataclass(frozen=True)
class LoopbackResponse:
    status: int
    execution_state: str
    content_type: str
    body: bytes
    calibration_dispatch_ms: int | None


def _testing() -> bool:
    return os.environ.get(TESTING_ENV) == "1"


def _port() -> int:
    if not _testing():
        return PRODUCTION_PORT
    raw = os.environ.get(TEST_PORT_ENV)
    try:
        port = int(raw or "")
    except ValueError as error:
        raise TransportError("test loopback port is invalid") from error
    if not 1 <= port <= 65_535:
        raise TransportError("test loopback port is invalid")
    return port


def _bounded_seconds(value: float, field: str, *, maximum: float, allow_zero: bool = False) -> float:
    minimum = 0.0 if allow_zero and _testing() else 0.01
    if isinstance(value, bool) or value < minimum or value > maximum:
        raise TransportError(f"{field} is outside its fixed bounds")
    return value


def _directory_info(path: Path, *, mode: int | None = None) -> os.stat_result:
    if not path.is_absolute():
        raise TransportError("private path must be absolute")
    try:
        info = path.lstat()
    except OSError as error:
        raise TransportError("private directory is unavailable") from error
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise TransportError("private directory is not a real directory")
    if info.st_uid != os.geteuid():
        raise TransportError("private directory has the wrong owner")
    if mode is not None and stat.S_IMODE(info.st_mode) != mode:
        raise TransportError("private directory has the wrong mode")
    return info


def _immutable_python_file(
    path: Path,
    *,
    owner_uid: int,
    expected_sha256: str,
    label: str,
) -> None:
    try:
        info = path.lstat()
    except OSError as error:
        raise TransportError(f"{label} is unavailable") from error
    if (
        stat.S_ISLNK(info.st_mode)
        or not stat.S_ISREG(info.st_mode)
        or info.st_nlink != 1
        or info.st_uid != owner_uid
        or (not _testing() and stat.S_IMODE(info.st_mode) & 0o022)
    ):
        raise TransportError(f"{label} is not an immutable regular file")
    digest = hashlib.sha256()
    try:
        with path.open("rb") as source:
            while chunk := source.read(65_536):
                digest.update(chunk)
    except OSError as error:
        raise TransportError(f"{label} could not be authenticated") from error
    if digest.hexdigest() != expected_sha256:
        raise TransportError(f"{label} differs from the pinned producer")


def _load_admission(library_root: Path) -> ModuleType:
    info = _directory_info(library_root)
    if not _testing() and (info.st_uid != 0 or stat.S_IMODE(info.st_mode) & 0o022):
        raise TransportError("admission library root is not root-owned and immutable")
    package = library_root / "gemini_calibration"
    package_info = _directory_info(package)
    if not _testing() and (package_info.st_uid != 0 or stat.S_IMODE(package_info.st_mode) & 0o022):
        raise TransportError("admission library package is not root-owned and immutable")
    try:
        package_entries = {entry.name for entry in package.iterdir()}
    except OSError as error:
        raise TransportError("admission library package could not be enumerated") from error
    if package_entries != {"__init__.py", "admission.py", "run_live.py"}:
        raise TransportError("admission library package has an unauthenticated entry")
    expected_init = package / "__init__.py"
    expected = package / "admission.py"
    expected_run_live = package / "run_live.py"
    _immutable_python_file(
        expected_init,
        owner_uid=info.st_uid,
        expected_sha256=PACKAGE_SHA256,
        label="admission package marker",
    )
    _immutable_python_file(
        expected,
        owner_uid=info.st_uid,
        expected_sha256=ADMISSION_SHA256,
        label="admission library",
    )
    _immutable_python_file(
        expected_run_live,
        owner_uid=info.st_uid,
        expected_sha256=RUN_LIVE_SHA256,
        label="admission parser library",
    )
    root_text = str(library_root)
    if root_text not in sys.path:
        sys.path.insert(0, root_text)
    try:
        module = importlib.import_module("gemini_calibration.admission")
    except (ImportError, OSError) as error:
        raise TransportError("admission library could not be loaded") from error
    loaded_package_module = sys.modules.get("gemini_calibration")
    loaded_package = Path(getattr(loaded_package_module, "__file__", ""))
    loaded = Path(module.__file__ or "")
    loaded_run_live = Path(getattr(module.run_live, "__file__", ""))
    try:
        if loaded_package.resolve(strict=True) != expected_init.resolve(strict=True):
            raise TransportError("a foreign admission package was already loaded")
        if loaded.resolve(strict=True) != expected.resolve(strict=True):
            raise TransportError("a foreign admission library was already loaded")
        if loaded_run_live.resolve(strict=True) != expected_run_live.resolve(strict=True):
            raise TransportError("a foreign admission parser library was already loaded")
    except OSError as error:
        raise TransportError("admission library identity is unavailable") from error
    required = {
        "claim_count",
        "record_count",
        "claim_generation",
        "record_outcome",
        "inspect",
        "COUNT_OBSERVATION_SCHEMA",
        "OBSERVATION_SCHEMA",
    }
    if any(not hasattr(module, name) for name in required):
        raise TransportError("admission library lacks the irreversible transport contract")
    return module


def _take_secret(name: str) -> str:
    value = os.environ.pop(name, None)
    if (
        not isinstance(value, str)
        or not 1 <= len(value) <= 4096
        or any(ord(character) < 0x21 or ord(character) > 0x7E for character in value)
    ):
        raise TransportError("required inherited transport credential is unavailable")
    return value


def _validate_output(evidence_dir: Path, output: Path, expected_name: str) -> None:
    if not evidence_dir.is_absolute() or not output.is_absolute():
        raise TransportError("evidence paths must be absolute")
    if output.name != expected_name or output.parent != evidence_dir.parent:
        raise TransportError("observation output is not the fixed evidence sibling")
    _directory_info(evidence_dir.parent, mode=0o700)
    if output.exists() or output.is_symlink():
        raise TransportError("observation output already exists")


def _fsync_directory(path: Path) -> None:
    descriptor = os.open(
        path,
        os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
    )
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _json_bytes(value: dict[str, Any]) -> bytes:
    try:
        data = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    except (TypeError, ValueError) as error:
        raise TransportError("private observation is not canonical JSON") from error
    if len(data) > MAX_OBSERVATION_BYTES:
        raise TransportError("private observation exceeds its fixed ceiling")
    return data


def _exclusive_write(path: Path, data: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError as error:
        raise TransportError("private observation could not be reserved") from error
    try:
        with os.fdopen(descriptor, "wb", closefd=True) as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(path, 0o600, follow_symlinks=False)
        _fsync_directory(path.parent)
    except BaseException:
        # Retain a created evidence file.  For claimed traffic its presence is safer than making a
        # later controller believe no terminalization was attempted.
        raise


def _atomic_replace(path: Path, value: dict[str, Any]) -> None:
    data = _json_bytes(value)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}.{uuid.uuid4().hex}")
    _exclusive_write(temporary, data)
    try:
        os.replace(temporary, path)
        os.chmod(path, 0o600, follow_symlinks=False)
        _fsync_directory(path.parent)
    finally:
        with contextlib.suppress(FileNotFoundError):
            temporary.unlink()


def _reserve_terminal_fallback(path: Path, value: dict[str, Any]) -> None:
    # The conservative unknown-execution fallback exists before an irreversible claim.  A caught
    # result atomically replaces it; a process crash leaves a private recovery observation while
    # the claim itself permanently forbids replay.
    _exclusive_write(path, _json_bytes(value))


def _abort_unclaimed_output(path: Path) -> None:
    with contextlib.suppress(FileNotFoundError):
        path.unlink()
        _fsync_directory(path.parent)


def _read_bounded(response: http.client.HTTPResponse, maximum: int) -> bytes:
    content_lengths = response.headers.get_all("content-length") or []
    transfer_encodings = response.headers.get_all("transfer-encoding") or []
    content_encodings = response.headers.get_all("content-encoding") or []
    if response.version != 11:
        raise TransportError("loopback response did not use HTTP/1.1")
    if content_encodings:
        raise TransportError("loopback response uses unsupported content encoding")
    if len(content_lengths) == 1 and not transfer_encodings:
        raw_length = content_lengths[0]
        if re.fullmatch(r"0|[1-9][0-9]*", raw_length) is None:
            raise TransportError("loopback response has invalid Content-Length framing")
        declared_length = int(raw_length)
        if response.chunked or response.length != declared_length:
            raise TransportError("loopback response has inconsistent Content-Length framing")
        if declared_length > maximum:
            raise TransportError("loopback response body exceeded its fixed ceiling")
    elif len(transfer_encodings) == 1 and not content_lengths:
        if transfer_encodings[0].lower() != "chunked" or response.chunked is not True:
            raise TransportError("loopback response has unsupported transfer framing")
    else:
        raise TransportError("loopback response has ambiguous or missing framing")

    chunks: list[bytes] = []
    total = 0
    while True:
        try:
            chunk = response.read(min(65_536, maximum + 1 - total))
        except (OSError, http.client.HTTPException) as error:
            raise TransportError("loopback response body was interrupted") from error
        if not chunk:
            # Sized HTTPResponse.read() returns a short body at premature EOF instead of raising
            # IncompleteRead.  A still-positive parsed Content-Length therefore means the evidence
            # stream was truncated even when its received prefix already contains STOP and usage.
            if response.length not in (None, 0):
                raise TransportError("loopback response body was interrupted")
            break
        chunks.append(chunk)
        total += len(chunk)
        if total > maximum:
            raise TransportError("loopback response body exceeded its fixed ceiling")
    return b"".join(chunks)


def _calibration_dispatch_ms(
    status: int,
    values: list[str],
    not_after: int | None,
) -> int | None:
    if not_after is None:
        if values:
            raise TransportError("ordinary response exposed a calibration dispatch attestation")
        return None
    if status != 200:
        if values:
            raise TransportError("non-success response exposed a calibration dispatch attestation")
        return None
    if len(values) != 1 or CANONICAL_POSITIVE_INTEGER_RE.fullmatch(values[0]) is None:
        raise TransportError("successful admission response has no canonical dispatch attestation")
    dispatch_ms = int(values[0])
    if dispatch_ms >= not_after * 1000:
        raise TransportError("admission response was dispatched outside its immutable window")
    return dispatch_ms


class _OneConnection:
    """A single-use numeric-loopback HTTP/1.1 connection."""

    def __init__(self, port: int, timeout: float, *, not_after: int | None = None) -> None:
        if not_after is not None and (
            isinstance(not_after, bool) or not isinstance(not_after, int) or not_after <= 0
        ):
            raise TransportError("admission cutoff is invalid")
        self._connection = _NumericLoopbackHTTPConnection(port, timeout, not_after=not_after)
        self._not_after = not_after
        self._used = False

    def request(
        self,
        method: str,
        path: str,
        headers: dict[str, str],
        body: bytes | None,
        maximum: int,
    ) -> LoopbackResponse:
        if self._used:
            raise TransportError("loopback connection cannot be reused")
        self._used = True
        exact_headers = dict(headers)
        exact_headers["connection"] = "close"
        try:
            # HTTPConnection has no redirect or proxy layer.  Its constructor performs no I/O;
            # this request call is the sole point that can open the numeric-loopback socket.
            self._connection.request(method, path, body=body, headers=exact_headers)
            response = self._connection.getresponse()
            status = response.status
            content_type = response.getheader("content-type", "")
            execution_values = response.headers.get_all("x-apitoken-execution-state") or []
            dispatch_values = response.headers.get_all(CALIBRATION_DISPATCH_HEADER) or []
            raw = _read_bounded(response, maximum)
            calibration_dispatch_ms = _calibration_dispatch_ms(
                status,
                dispatch_values,
                self._not_after,
            )
        except (OSError, http.client.HTTPException, TransportError) as error:
            raise TransportError("single loopback request failed") from error
        finally:
            self._connection.close()
        execution_state = (
            "not_started"
            if status != 200 and execution_values == ["not_started"]
            else ("completed" if status == 200 and not execution_values else "unknown")
        )
        return LoopbackResponse(
            status,
            execution_state,
            content_type,
            raw,
            calibration_dispatch_ms,
        )


class _NumericLoopbackHTTPConnection(http.client.HTTPConnection):
    """HTTP/1.1 over a literal AF_INET socket, bypassing getaddrinfo entirely."""

    def __init__(self, port: int, timeout: float, *, not_after: int | None = None) -> None:
        super().__init__(LOOPBACK_HOST, port, timeout=timeout)
        self._not_after_ns = None if not_after is None else not_after * 1_000_000_000

    def connect(self) -> None:
        if self.sock is not None:
            raise TransportError("numeric loopback connection was already opened")
        connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            connection.settimeout(self.timeout)
            # This is the final userspace operation before the sole connect(2).  The outer check
            # records an authoritative not_started result when already expired; this second,
            # nanosecond-resolution fence closes a scheduler-pause window after the claim.
            if self._not_after_ns is not None and time.time_ns() >= self._not_after_ns:
                raise TransportError("admission cutoff crossed before connection open")
            connection.connect((LOOPBACK_HOST, self.port))
            self.sock = connection
        except BaseException:
            connection.close()
            raise


def _json_object(raw: bytes) -> dict[str, Any]:
    try:
        value = json.loads(raw)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise TransportError("loopback response is not valid JSON") from error
    if not isinstance(value, dict):
        raise TransportError("loopback response is not a JSON object")
    return value


def _is_event_stream_content_type(value: str) -> bool:
    return EVENT_STREAM_CONTENT_TYPE_RE.fullmatch(value) is not None


def _ready_once(port: int, timeout: float) -> None:
    response = _OneConnection(port, timeout).request(
        "GET",
        "/ready",
        {"accept": "application/json"},
        None,
        MAX_COUNT_BYTES,
    )
    if response.status != 200 or _json_object(response.body).get("ready") is not True:
        raise TransportError("admission canary is not ready")


def _capacity_once(port: int, timeout: float, panel_key: str) -> dict[str, Any]:
    response = _OneConnection(port, timeout).request(
        "GET",
        "/gemini-subs",
        {"accept": "application/json", "x-api-key": panel_key},
        None,
        MAX_CAPACITY_BYTES,
    )
    if response.status != 200:
        raise TransportError("immutable capacity endpoint rejected the request")
    return _json_object(response.body)


def _private_contract(admission: ModuleType, evidence_dir: Path, kind: str) -> tuple[dict[str, Any], dict[str, Any]]:
    # The state machine's readers enforce mode, hard-link, no-follow, digest, fence, and exact
    # request reconstruction.  Keeping this controller coupled to those readers prevents a second,
    # subtly weaker parser from becoming transport authority.
    journal = admission._load_journal(evidence_dir)
    if kind == "count":
        expected_state = "awaiting_count_tokens"
        filename = admission.COUNT_REQUEST
        expected = admission._count_request(journal)
        digest = journal["count_request_sha256"]
    else:
        expected_state = "generation_armed"
        filename = admission.GENERATION_REQUEST
        expected = admission._generation_request(journal)
        digest = journal["generation_request_sha256"]
    if journal.get("state") != expected_state:
        raise TransportError("admission state is not transportable")
    request = admission._read_json(evidence_dir / filename, required_mode=0o600)
    admission._validate_canonical_request(evidence_dir / filename, expected, digest, kind)
    if request != expected or request.get("method") != "POST":
        raise TransportError("private request differs from the immutable admission contract")
    profile = request.get("target_profile")
    request_id = request.get("request_id" if kind == "count" else "calibration_request_id")
    if not isinstance(profile, str) or not PROFILE_RE.fullmatch(profile):
        raise TransportError("private request has an invalid exact target")
    if not isinstance(request_id, str) or not UUID_V4_RE.fullmatch(request_id):
        raise TransportError("private request has an invalid request identity")
    not_after = request.get("not_after")
    if (
        isinstance(not_after, bool)
        or not isinstance(not_after, int)
        or not_after <= 0
        or not_after != journal.get("not_after")
    ):
        raise TransportError("private request has an invalid dispatch cutoff")
    path = request.get("path")
    body = request.get("body")
    if not isinstance(path, str) or not path.startswith("/v1beta/models/gemini-3.7-flash:"):
        raise TransportError("private request has an invalid fixed model path")
    if not isinstance(body, dict):
        raise TransportError("private request has no body")
    return journal, request


def _request_bytes(request: dict[str, Any], maximum: int) -> bytes:
    data = json.dumps(request["body"], sort_keys=True, separators=(",", ":")).encode()
    if not data or len(data) > maximum:
        raise TransportError("private request body exceeds its fixed ceiling")
    return data


def _headers(secret: str, request: dict[str, Any], *, count: bool) -> dict[str, str]:
    request_id_field = "request_id" if count else "calibration_request_id"
    return {
        "accept": "application/json" if count else "text/event-stream",
        "content-type": "application/json",
        "x-goog-api-key": secret,
        "x-apitoken-calibration-profile": request["target_profile"],
        "x-apitoken-calibration-request-id": request[request_id_field],
        "x-apitoken-calibration-not-after": str(request["not_after"]),
    }


def _count_observation(
    admission: ModuleType,
    journal: dict[str, Any],
    *,
    status: int,
    execution_state: str,
    dispatch_ms: int | None,
    response: dict[str, Any] | None,
) -> dict[str, Any]:
    return {
        "schema": admission.COUNT_OBSERVATION_SCHEMA,
        "request_id": journal["count_request_id"],
        "request_sha256": journal["count_request_sha256"],
        "target_profile": journal["profile_id"],
        "model": admission.MODEL,
        "http_status": status,
        "execution_state": execution_state,
        "dispatch_ms": dispatch_ms,
        "response": response,
    }


def _outcome_observation(
    admission: ModuleType,
    journal: dict[str, Any],
    *,
    status: int,
    execution_state: str,
    dispatch_ms: int | None,
    response: list[dict[str, Any]] | None,
    capacity: dict[str, Any] | None,
) -> dict[str, Any]:
    return {
        "schema": admission.OBSERVATION_SCHEMA,
        "request_id": journal["request_id"],
        "request_sha256": journal["generation_request_sha256"],
        "target_profile": journal["profile_id"],
        "plan": journal["plan"],
        "http_status": status,
        "execution_state": execution_state,
        "dispatch_ms": dispatch_ms,
        "response": response,
        "immutable_capacity": capacity,
        "event_request_id": journal["request_id"],
        "event_plan": journal["plan"],
    }


def _event_present(
    admission: ModuleType,
    capacity: dict[str, Any],
    journal: dict[str, Any],
) -> bool:
    try:
        admission.run_live.require_healthy_delivery(capacity)
        event = admission.run_live.exact_new_turn(
            set(),
            capacity,
            journal["request_id"],
            journal["profile_id"],
            admission_model(journal),
        )
    except (admission.run_live.CalibrationError, KeyError, TypeError, ValueError):
        return False
    profiles = capacity.get("profiles")
    if event is None or not isinstance(profiles, list):
        return False
    profile_matches = [
        profile
        for profile in profiles
        if isinstance(profile, dict)
        and profile.get("id") == journal["profile_id"]
        and profile.get("plan") == journal["plan"]
    ]
    return len(profile_matches) == 1


def admission_model(journal: dict[str, Any]) -> str:
    # Kept as a tiny named helper so event selection never accepts a response-provided model.
    return journal["model"]


def _poll_capacity(
    admission: ModuleType,
    port: int,
    timeout: float,
    panel_key: str,
    journal: dict[str, Any],
    evidence_timeout: float,
    poll_interval: float,
) -> dict[str, Any] | None:
    deadline = time.monotonic() + evidence_timeout
    last: dict[str, Any] | None = None
    while True:
        try:
            last = _capacity_once(port, timeout, panel_key)
        except TransportError:
            # Each read is a separate bounded evidence poll, never a reconnect or a paid replay.
            pass
        else:
            if _event_present(admission, last, journal):
                return last
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return last
        time.sleep(min(poll_interval, remaining))


def _print_summary(summary: dict[str, Any]) -> None:
    print(json.dumps(summary, sort_keys=True, separators=(",", ":")))


def capacity_command(args: argparse.Namespace) -> None:
    _validate_output(args.evidence_dir, args.output, CAPACITY_OUTPUT)
    timeout = _bounded_seconds(args.timeout_seconds, "HTTP timeout", maximum=300)
    panel_key = _take_secret(PANEL_KEY_ENV)
    try:
        capacity = _capacity_once(_port(), timeout, panel_key)
        _exclusive_write(args.output, _json_bytes(capacity))
    finally:
        panel_key = ""
    _print_summary({"schema": "gemini-3.7-admission-transport/v1", "state": "capacity_recorded"})


def count_command(args: argparse.Namespace) -> None:
    admission = _load_admission(args.library_root)
    _validate_output(args.evidence_dir, args.output, COUNT_OUTPUT)
    timeout = _bounded_seconds(args.timeout_seconds, "HTTP timeout", maximum=300)
    journal, request = _private_contract(admission, args.evidence_dir, "count")
    secret = _take_secret(ADMIN_KEY_ENV)
    port = _port()
    body = _request_bytes(request, MAX_COUNT_BYTES)
    headers = _headers(secret, request, count=True)
    connection = _OneConnection(port, timeout, not_after=journal["not_after"])
    _ready_once(port, timeout)
    fallback = _count_observation(
        admission,
        journal,
        status=0,
        execution_state="unknown",
        dispatch_ms=None,
        response=None,
    )
    _reserve_terminal_fallback(args.output, fallback)
    claim_invoked = False
    claimed = False
    try:
        claim_invoked = True
        admission.claim_count(args.evidence_dir)
        claimed = True
        if time.time_ns() >= journal["not_after"] * 1_000_000_000:
            observation = _count_observation(
                admission,
                journal,
                status=0,
                execution_state="not_started",
                dispatch_ms=None,
                response=None,
            )
        else:
            try:
                result = connection.request(
                    "POST", request["path"], headers, body, MAX_COUNT_BYTES
                )
            except TransportError:
                observation = fallback
            else:
                parsed: dict[str, Any] | None = None
                if result.status == 200:
                    with contextlib.suppress(TransportError):
                        parsed = _json_object(result.body)
                observation = _count_observation(
                    admission,
                    journal,
                    status=result.status,
                    execution_state=result.execution_state,
                    dispatch_ms=result.calibration_dispatch_ms,
                    response=parsed,
                )
        _atomic_replace(args.output, observation)
        summary = admission.record_count(args.evidence_dir, args.output)
    except BaseException:
        if claimed:
            # The pre-claim unknown observation is still present unless an atomic, more precise
            # result replaced it.  Either way the offline module gets one terminalization attempt;
            # no network action is repeated.
            with contextlib.suppress(Exception):
                admission.record_count(args.evidence_dir, args.output)
        raise
    finally:
        secret = ""
        headers.clear()
        if not claim_invoked:
            _abort_unclaimed_output(args.output)
    _print_summary(summary)


def generate_command(args: argparse.Namespace) -> None:
    admission = _load_admission(args.library_root)
    _validate_output(args.evidence_dir, args.output, OUTCOME_OUTPUT)
    timeout = _bounded_seconds(args.timeout_seconds, "HTTP timeout", maximum=300)
    evidence_timeout = _bounded_seconds(
        args.evidence_timeout_seconds, "evidence timeout", maximum=300
    )
    poll_interval = _bounded_seconds(
        args.poll_interval_seconds,
        "evidence poll interval",
        maximum=10,
        allow_zero=True,
    )
    journal, request = _private_contract(admission, args.evidence_dir, "generation")
    if journal.get("stream") is not True or ":streamGenerateContent?alt=sse" not in request["path"]:
        raise TransportError("paid admission request is not the required SSE call")
    admin_key = _take_secret(ADMIN_KEY_ENV)
    try:
        panel_key = _take_secret(PANEL_KEY_ENV)
    except BaseException:
        admin_key = ""
        raise
    port = _port()
    body = _request_bytes(request, MAX_GENERATION_BYTES)
    headers = _headers(admin_key, request, count=False)
    connection = _OneConnection(port, timeout, not_after=journal["not_after"])
    _ready_once(port, timeout)
    fallback = _outcome_observation(
        admission,
        journal,
        status=0,
        execution_state="unknown",
        dispatch_ms=None,
        response=None,
        capacity=None,
    )
    _reserve_terminal_fallback(args.output, fallback)
    claim_invoked = False
    claimed = False
    try:
        claim_invoked = True
        admission.claim_generation(args.evidence_dir)
        claimed = True
        # This is deliberately the only operation between the irreversible claim and the one
        # HTTPConnection.request call.  The connection object, bytes, headers and output fallback
        # were all prepared before the claim.
        if time.time_ns() >= journal["not_after"] * 1_000_000_000:
            observation = _outcome_observation(
                admission,
                journal,
                status=0,
                execution_state="not_started",
                dispatch_ms=None,
                response=None,
                capacity=None,
            )
        else:
            try:
                result = connection.request(
                    "POST", request["path"], headers, body, MAX_GENERATION_BYTES
                )
            except TransportError:
                observation = fallback
            else:
                frames: list[dict[str, Any]] | None = None
                capacity: dict[str, Any] | None = None
                if result.status == 200 and result.execution_state == "completed":
                    if not _is_event_stream_content_type(result.content_type):
                        result = dataclasses.replace(result, execution_state="unknown")
                    else:
                        decoded = admission.run_live.decode_generation_response(result.body, True)
                        if decoded.parse_error is None:
                            frames = list(decoded.frames)
                            capacity = _poll_capacity(
                                admission,
                                port,
                                timeout,
                                panel_key,
                                journal,
                                evidence_timeout,
                                poll_interval,
                            )
                observation = _outcome_observation(
                    admission,
                    journal,
                    status=result.status,
                    execution_state=result.execution_state,
                    dispatch_ms=result.calibration_dispatch_ms,
                    response=frames,
                    capacity=capacity,
                )
        _atomic_replace(args.output, observation)
        summary = admission.record_outcome(args.evidence_dir, args.output)
    except BaseException:
        if claimed:
            with contextlib.suppress(Exception):
                admission.record_outcome(args.evidence_dir, args.output)
        raise
    finally:
        admin_key = ""
        panel_key = ""
        headers.clear()
        if not claim_invoked:
            _abort_unclaimed_output(args.output)
    _print_summary(summary)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    def paths(command: argparse.ArgumentParser, output_name: str) -> None:
        command.add_argument("--evidence-dir", type=Path, required=True)
        command.add_argument(
            "--output",
            type=Path,
            required=True,
            help=f"fixed private sibling named {output_name}",
        )
        command.add_argument("--timeout-seconds", type=float, default=30.0)

    capacity = commands.add_parser("capacity")
    paths(capacity, CAPACITY_OUTPUT)

    count = commands.add_parser("count")
    paths(count, COUNT_OUTPUT)
    count.add_argument("--library-root", type=Path, default=DEFAULT_LIBRARY_ROOT)

    generate = commands.add_parser("generate")
    paths(generate, OUTCOME_OUTPUT)
    generate.add_argument("--library-root", type=Path, default=DEFAULT_LIBRARY_ROOT)
    generate.add_argument("--evidence-timeout-seconds", type=float, default=180.0)
    generate.add_argument("--poll-interval-seconds", type=float, default=1.0)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(sys.argv[1:] if argv is None else argv)
        if args.command == "capacity":
            capacity_command(args)
        elif args.command == "count":
            count_command(args)
        else:
            generate_command(args)
        return 0
    except Exception:
        # No exception detail is ever emitted: HTTP library messages may contain paths, while
        # admission errors intentionally remain private to the root-owned evidence journal.
        print("Gemini 3.7 admission transport stopped safely.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
