#!/usr/bin/env python3
"""Credential-safe Stage 5 controlled Gemini Batch runner.

Dry-run is the default.  Execution is deliberately restricted to the production host: the test
account key and panel key are expanded only by the remote shell and never cross the SSH boundary.
The runner emits a sanitized checkpoint with opaque identifiers, holds, settlements and profiles;
it never emits request contents, results, credentials, or raw diagnostic responses.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import re
import shlex
import subprocess
import sys
import time
import uuid
from pathlib import Path
from typing import Any, Callable

NANO_PER_USD = 1_000_000_000
AUTHORIZED_BUDGET_NANO = 10_000_000_000
DEFAULT_SSH_TARGET = "apitokensale"
DEFAULT_API_PORT = 8794
DEFAULT_MODEL = "gemini-2.5-flash"
DEFAULT_MAX_OUTPUT_TOKENS = 16
CHECKPOINT_SCHEMA = "gemini-batch-stage5-checkpoint/v1"
PLAN_SCHEMA = "gemini-batch-stage5-plan/v1"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
SAFE_ID_RE = re.compile(r"^[A-Za-z0-9._-]{1,128}$")


class RunnerError(RuntimeError):
    """A fail-closed invariant prevents any further paid action."""


class AmbiguousPaidCreate(RunnerError):
    """The sole paid create attempt has no authoritative HTTP result and cannot be resumed."""


@dataclasses.dataclass(frozen=True)
class Scenario:
    name: str
    items: int
    paid_create: bool
    action: str


SCENARIOS = (
    Scenario("distribution-two-items", 2, True, "poll-terminal"),
    Scenario("cancel", 1, True, "cancel-then-poll"),
    Scenario("restart-safe-boundary", 1, True, "operator-restart-boundary"),
    Scenario("headroom-no-paid", 1, False, "observe-no-dispatch"),
    Scenario("ordinary-parity", 1, True, "ordinary-generate-parity"),
)


def as_nonnegative_int(value: Any, field: str) -> int:
    if isinstance(value, bool):
        raise RunnerError(f"{field} must be an integer nanoUSD value")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as error:
        raise RunnerError(f"{field} must be an integer nanoUSD value") from error
    if parsed < 0:
        raise RunnerError(f"{field} must not be negative")
    return parsed


def usd_to_nano(value: str) -> int:
    whole, dot, fraction = value.strip().partition(".")
    if not whole.isdigit() or (dot and not fraction.isdigit()) or len(fraction) > 9:
        raise RunnerError(f"invalid exact USD amount: {value!r}")
    return int(whole) * NANO_PER_USD + int((fraction + "000000000")[:9])


def remaining_budget(previous_spend_nano: int, new_spend_nano: int = 0) -> int:
    previous = as_nonnegative_int(previous_spend_nano, "previous_spend_nano")
    new = as_nonnegative_int(new_spend_nano, "new_spend_nano")
    total = previous + new
    if total > AUTHORIZED_BUDGET_NANO:
        raise RunnerError("the original Stage 5+6 $10 aggregate budget would be exceeded")
    return AUTHORIZED_BUDGET_NANO - total


def reserve_budget(previous_spend_nano: int, planned_holds_nano: list[int]) -> int:
    holds = [as_nonnegative_int(value, "hold_nano") for value in planned_holds_nano]
    available = remaining_budget(previous_spend_nano)
    required = sum(holds)
    if required > available:
        raise RunnerError(
            f"server-authoritative holds require {required} nanoUSD, only {available} remains"
        )
    return available - required


def validate_sha(value: str) -> str:
    if not SHA_RE.fullmatch(value):
        raise RunnerError("implementation SHA must be exactly 40 lowercase hexadecimal characters")
    return value


def validate_safe_id(value: Any, field: str) -> str:
    if not isinstance(value, str) or not SAFE_ID_RE.fullmatch(value):
        raise RunnerError(f"{field} is missing or unsafe")
    return value


def validate_ssh_target(value: str) -> str:
    if (
        not value
        or len(value) > 255
        or not value[0].isalnum()
        or value.count("@") > 1
        or not all(char.isascii() and (char.isalnum() or char in ".-_:@") for char in value)
    ):
        raise RunnerError(f"invalid SSH target: {value!r}")
    return value


def validate_port(value: int) -> int:
    if isinstance(value, bool) or not 1 <= value <= 65_535:
        raise RunnerError(f"invalid production API port: {value!r}")
    return value


def remote_command(api_port: int) -> str:
    """Return the fixed secret-loading remote program; no credential value is interpolated."""
    port = validate_port(api_port)
    return f"""set -eu
umask 077
set -a
. /srv/claude-api/data/server.env
set +a
batch_key=${{GEMINI_BATCH_STAGE5_API_KEY:-}}
panel_key=${{CLAUDE_API_PANEL_KEY:-}}
database_url=${{CLAUDE_API_DATABASE_URL:-${{DATABASE_URL:-}}}}
test -n "$batch_key"
test -n "$panel_key"
test -n "$database_url"
export batch_key panel_key database_url
python3 - {port}
"""


def ssh_argv(ssh_target: str, api_port: int) -> list[str]:
    return ["ssh", validate_ssh_target(ssh_target), remote_command(api_port)]


REMOTE_HELPER = r'''
import json, os, sys, urllib.error, urllib.request

PORT = int(sys.argv[1])
BASE = f"http://127.0.0.1:{PORT}"
BATCH_KEY = os.environ["batch_key"]
PANEL_KEY = os.environ["panel_key"]


def request(path, method="GET", body=None, panel=False):
    headers = {"accept": "application/json"}
    headers["x-api-key" if panel else "x-goog-api-key"] = PANEL_KEY if panel else BATCH_KEY
    data = None
    if body is not None:
        headers["content-type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode()
    req = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=45) as response:
            raw = response.read(2_000_000)
            return response.status, json.loads(raw or b"{}"), dict(response.headers.items())
    except urllib.error.HTTPError as error:
        raw = error.read(200_000)
        try:
            payload = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            payload = {"error": "non-json upstream response"}
        return error.code, payload, dict(error.headers.items())


def safe_id(value, prefix):
    return isinstance(value, str) and value.startswith(prefix) and len(value) <= 160


def batch_name(payload):
    value = payload.get("name")
    if not safe_id(value, "batches/"):
        raise RuntimeError("create returned no safe batch id")
    return value


def sanitize_profiles(payload):
    values = []
    for profile in payload.get("profiles", []):
        ident = profile.get("id")
        if isinstance(ident, str) and ident and len(ident) <= 128:
            values.append(ident)
    return sorted(set(values))


def diagnostic_projection(payload):
    batch = payload.get("batch") if isinstance(payload.get("batch"), dict) else {}
    return {
        "profiles": sanitize_profiles(payload),
        "batch": {
            "enabled": batch.get("enabled") is True,
            "public_enabled": batch.get("public_enabled") is True,
            "authority_available": batch.get("authority_available") is True,
            "queue_depth": batch.get("queue_depth"),
            "active_items": batch.get("active_items"),
            "completed_items": batch.get("completed_items"),
            "error_items": batch.get("error_items"),
            "indeterminate_items": batch.get("indeterminate_items"),
            "headroom_stops": batch.get("headroom_stops"),
            "settlement_backlog": batch.get("settlement_backlog"),
        },
    }


def item_holds(job_id):
    # Deliberately secret-free diagnostic projection: only public batch/item ids and integer money.
    # This query never selects encrypted blobs, customer text, raw keys, result bytes, or subjects.
    sql = """COPY (SELECT json_build_object('item_id',job_id||':'||item_index::text,'hold_nano',hold_nano)::text FROM gemini_batch_items WHERE job_id='""" + job_id.replace("'", "") + "' ORDER BY item_index) TO STDOUT"
    database_url = os.environ.get("database_url")
    if not database_url:
        raise RuntimeError("engine PostgreSQL URL is unavailable for hold projection")
    import subprocess
    result = subprocess.run(["psql", database_url, "-At", "-c", sql], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError("server-authoritative hold projection failed")
    rows = [json.loads(line) for line in result.stdout.splitlines() if line]
    if not rows:
        raise RuntimeError("server-authoritative hold projection returned no items")
    return rows


def settlements(job_id):
    database_url = os.environ.get("database_url")
    if not database_url:
        raise RuntimeError("engine PostgreSQL URL is unavailable for settlement projection")
    import subprocess
    sql = """COPY (SELECT json_build_object('item_id',i.job_id||':'||i.item_index::text,'state',i.state,'terminal_class',i.terminal_class,'actual_nano',COALESCE(o.actual_nano,0),'profile_id',COALESCE(o.calibration_profile_id,''))::text FROM gemini_batch_items i LEFT JOIN gemini_batch_settlement_outbox o ON o.request_id=i.settlement_id WHERE i.job_id='""" + job_id.replace("'", "") + "' ORDER BY i.item_index) TO STDOUT"
    result = subprocess.run(["psql", database_url, "-At", "-c", sql], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError("server-authoritative settlement projection failed")
    return [json.loads(line) for line in result.stdout.splitlines() if line]


def prompt(index):
    return {"contents":[{"role":"user","parts":[{"text":f"Reply only with the digit {index}."}]}],"generationConfig":{"maxOutputTokens":16}}


def main(command):
    op = command["op"]
    model = command["model"]
    if op == "preflight":
        counts = []
        for index in range(command["items"]):
            status, payload, _ = request(f"/v1beta/models/{model}:countTokens", "POST", prompt(index))
            if status // 100 != 2 or int(payload.get("totalTokens", 0)) <= 0:
                raise RuntimeError("countTokens preflight failed")
            counts.append(int(payload["totalTokens"]))
        # Create-dry preflight is attempted only when the deployed producer offers it. A 404/405/501
        # is recorded as unavailable; any other failure blocks the paid create.
        status, _, _ = request(f"/v1beta/models/{model}:batchGenerateContent?dryRun=true", "POST", {
            "batch":{"displayName":"stage5-preflight","inputConfig":{"requests":[{"request":prompt(i)} for i in range(command["items"])]}}
        })
        if status // 100 == 2:
            dry = "passed"
        elif status in (404, 405, 501):
            dry = "unavailable"
        else:
            raise RuntimeError("create dry preflight failed")
        return {"count_tokens": counts, "create_dry_preflight": dry}
    if op == "diagnostic":
        status, payload, _ = request("/gemini-subs", panel=True)
        if status // 100 != 2:
            raise RuntimeError("diagnostic projection failed")
        return diagnostic_projection(payload)
    if op == "create":
        body = {"batch":{"displayName":"stage5-controlled","inputConfig":{"requests":[{"request":prompt(i)} for i in range(command["items"])]}}}
        status, payload, _ = request(f"/v1beta/models/{model}:batchGenerateContent", "POST", body)
        if status // 100 != 2:
            raise RuntimeError(f"paid create returned HTTP {status}")
        name = batch_name(payload)
        rows = item_holds(name.split("/", 1)[1])
        required = sum(int(row["hold_nano"]) for row in rows)
        if len(rows) != command["items"]:
            raise RuntimeError("server-authoritative hold projection is incomplete")
        if required > int(command["budget_remaining_nano"]):
            raise RuntimeError("server-authoritative holds exceed the original budget remainder")
        return {"batch_id": name, "holds": rows}
    if op == "cancel":
        status, _, _ = request("/v1beta/" + command["batch_id"] + ":cancel", "POST", {})
        if status // 100 != 2:
            raise RuntimeError("cancel failed")
        return {"batch_id": command["batch_id"], "canceled": True}
    if op == "observe":
        batch_id = command["batch_id"]
        status, payload, _ = request("/v1beta/" + batch_id)
        if status // 100 != 2:
            raise RuntimeError("batch observation failed")
        ident = batch_id.split("/", 1)[1]
        return {"batch_id": batch_id, "done": payload.get("done") is True, "settlements": settlements(ident)}
    raise RuntimeError("unsupported remote operation")

try:
    command = json.load(sys.stdin)
    print(json.dumps({"ok": True, "value": main(command)}, separators=(",", ":")))
except Exception as error:
    print(json.dumps({"ok": False, "error": str(error)[:240]}, separators=(",", ":")))
    raise SystemExit(1)
'''


class Remote:
    def __init__(self, ssh_target: str, api_port: int, timeout: int = 90) -> None:
        self.argv = ssh_argv(ssh_target, api_port)
        self.timeout = timeout

    def call(self, command: dict[str, Any], *, paid_create: bool = False) -> dict[str, Any]:
        payload = (REMOTE_HELPER + "\n").encode() + json.dumps(command, separators=(",", ":")).encode()
        try:
            result = subprocess.run(
                self.argv,
                input=payload,
                capture_output=True,
                timeout=self.timeout,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            if paid_create:
                raise AmbiguousPaidCreate(
                    "paid create transport timed out; attempt is terminal and nonresumable"
                ) from error
            raise RunnerError("remote read/preflight timed out") from error
        if paid_create and (result.returncode == 255 or not result.stdout.strip()):
            raise AmbiguousPaidCreate(
                "paid create transport is ambiguous; attempt is terminal and nonresumable"
            )
        try:
            envelope = json.loads(result.stdout.decode())
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            if paid_create:
                raise AmbiguousPaidCreate(
                    "paid create returned no authoritative envelope; attempt is nonresumable"
                ) from error
            raise RunnerError("remote operation returned no valid sanitized envelope") from error
        if result.returncode != 0 or envelope.get("ok") is not True:
            message = envelope.get("error", "remote operation failed")
            raise RunnerError(str(message)[:240])
        value = envelope.get("value")
        if not isinstance(value, dict):
            raise RunnerError("remote operation returned an invalid projection")
        return value


def production_green_sha(remote: Remote) -> str:
    command = {
        "op": "release-sha",
    }
    # Release identity is read without application credentials by a separate fixed SSH command.
    argv = remote.argv[:2] + [
        "set -eu; readlink -f /srv/claude-api/releases/current | sed -n 's#.*/\\([0-9a-f]\\{40\\}\\)$#\\1#p'"
    ]
    result = subprocess.run(argv, capture_output=True, timeout=30, check=False, text=True)
    if result.returncode != 0:
        raise RunnerError("could not read the production release SHA")
    return validate_sha(result.stdout.strip())


def build_plan(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "schema": PLAN_SCHEMA,
        "mode": "execute" if args.execute else "dry-run",
        "network_requests": 0 if not args.execute else "operator-controlled",
        "authorized_budget_nanousd": str(AUTHORIZED_BUDGET_NANO),
        "previous_spend_nanousd": str(args.previous_spend_nanousd),
        "remaining_budget_nanousd": str(remaining_budget(args.previous_spend_nanousd)),
        "implementation_sha": args.implementation_sha,
        "model": args.model,
        "scenarios": [dataclasses.asdict(scenario) for scenario in SCENARIOS],
        "safeguards": [
            "dry-run-default",
            "exact-production-green-sha",
            "ssh-remote-only-key-loading",
            "free-countTokens-before-paid-create",
            "create-dry-preflight-when-supported",
            "one-paid-create-attempt",
            "ambiguous-create-nonresumable",
            "server-authoritative-holds-immediately-after-create",
            "integer-nanoUSD-budget",
            "sanitized-report-only",
        ],
    }


def checkpoint_path(path: str) -> Path:
    value = Path(path)
    if value.exists():
        raise RunnerError("checkpoint path already exists; runs are immutable and nonresumable")
    return value


def write_checkpoint(path: Path, report: dict[str, Any]) -> None:
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def execute(args: argparse.Namespace, remote: Remote) -> dict[str, Any]:
    active_sha = production_green_sha(remote)
    if active_sha != args.implementation_sha:
        raise RunnerError(
            f"production release SHA {active_sha} does not equal authorized {args.implementation_sha}"
        )
    checkpoint = checkpoint_path(args.checkpoint)
    report: dict[str, Any] = {
        "schema": CHECKPOINT_SCHEMA,
        "status": "running",
        "implementation_sha": active_sha,
        "budget_nanousd": {
            "authorized": str(AUTHORIZED_BUDGET_NANO),
            "previous_spend": str(args.previous_spend_nanousd),
            "new_settled": "0",
            "remaining": str(remaining_budget(args.previous_spend_nanousd)),
        },
        "scenarios": [],
    }
    write_checkpoint(checkpoint, report)
    paid_create_used = False
    new_settled = 0
    for scenario in SCENARIOS:
        entry: dict[str, Any] = {"name": scenario.name, "paid_create": scenario.paid_create}
        before = remote.call({"op": "diagnostic", "model": args.model})
        entry["profiles_before"] = before.get("profiles", [])
        entry["batch_before"] = before.get("batch", {})
        if not scenario.paid_create:
            entry["outcome"] = "no-paid-create"
            after = remote.call({"op": "diagnostic", "model": args.model})
            entry["profiles_after"] = after.get("profiles", [])
            entry["batch_after"] = after.get("batch", {})
            report["scenarios"].append(entry)
            write_checkpoint(checkpoint, report)
            continue
        if paid_create_used:
            raise RunnerError("one paid create attempt per invocation is mandatory")
        preflight = remote.call(
            {"op": "preflight", "model": args.model, "items": scenario.items}
        )
        entry["count_tokens"] = preflight.get("count_tokens", [])
        entry["create_dry_preflight"] = preflight.get("create_dry_preflight")
        paid_create_used = True
        try:
            created = remote.call(
                {
                    "op": "create",
                    "model": args.model,
                    "items": scenario.items,
                    "budget_remaining_nano": remaining_budget(
                        args.previous_spend_nanousd, new_settled
                    ),
                },
                paid_create=True,
            )
        except AmbiguousPaidCreate:
            entry["outcome"] = "ambiguous-nonresumable"
            report["status"] = "ambiguous-nonresumable"
            report["scenarios"].append(entry)
            write_checkpoint(checkpoint, report)
            raise
        batch_id = validate_safe_id(created.get("batch_id"), "batch_id")
        holds = created.get("holds")
        if not isinstance(holds, list) or len(holds) != scenario.items:
            raise RunnerError("paid create succeeded without complete immediate hold projection")
        normalized_holds = []
        for row in holds:
            if not isinstance(row, dict):
                raise RunnerError("invalid hold projection row")
            normalized_holds.append(
                {
                    "item_id": validate_safe_id(row.get("item_id"), "item_id"),
                    "hold_nanousd": str(as_nonnegative_int(row.get("hold_nano"), "hold_nano")),
                }
            )
        reserve_budget(
            args.previous_spend_nanousd + new_settled,
            [int(row["hold_nanousd"]) for row in normalized_holds],
        )
        entry["batch_id"] = batch_id
        entry["holds"] = normalized_holds
        if scenario.action == "cancel-then-poll":
            remote.call({"op": "cancel", "batch_id": batch_id, "model": args.model})
        observed = remote.call({"op": "observe", "batch_id": batch_id, "model": args.model})
        settlements = []
        for row in observed.get("settlements", []):
            if not isinstance(row, dict):
                continue
            actual = as_nonnegative_int(row.get("actual_nano", 0), "actual_nano")
            new_settled += actual
            remaining_budget(args.previous_spend_nanousd, new_settled)
            settlements.append(
                {
                    "item_id": validate_safe_id(row.get("item_id"), "item_id"),
                    "state": str(row.get("state", "unknown"))[:32],
                    "terminal_class": str(row.get("terminal_class") or "")[:32],
                    "actual_nanousd": str(actual),
                    "profile_id": (
                        validate_safe_id(row["profile_id"], "profile_id")
                        if row.get("profile_id")
                        else None
                    ),
                }
            )
        entry["settlements"] = settlements
        entry["done"] = observed.get("done") is True
        entry["outcome"] = "observed"
        report["scenarios"].append(entry)
        report["budget_nanousd"]["new_settled"] = str(new_settled)
        report["budget_nanousd"]["remaining"] = str(
            remaining_budget(args.previous_spend_nanousd, new_settled)
        )
        write_checkpoint(checkpoint, report)
        # Intentional: a single invocation can never perform a second paid create. Operators use a
        # fresh immutable checkpoint and the updated previous-spend value for the next scenario.
        break
    report["status"] = "complete"
    write_checkpoint(checkpoint, report)
    return report


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--implementation-sha")
    parser.add_argument("--previous-spend-nanousd", type=int)
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--ssh-target", default=DEFAULT_SSH_TARGET)
    parser.add_argument("--api-port", type=int, default=DEFAULT_API_PORT)
    parser.add_argument("--checkpoint", default="/tmp/gemini-batch-stage5-checkpoint.json")
    args = parser.parse_args(argv)
    try:
        validate_ssh_target(args.ssh_target)
        validate_port(args.api_port)
        if not SAFE_ID_RE.fullmatch(args.model):
            raise RunnerError("model id is unsafe")
        if args.previous_spend_nanousd is None:
            if args.execute:
                raise RunnerError("--execute requires --previous-spend-nanousd")
            args.previous_spend_nanousd = 0
        remaining_budget(args.previous_spend_nanousd)
        if args.execute:
            if not args.implementation_sha:
                raise RunnerError("--execute requires --implementation-sha")
            validate_sha(args.implementation_sha)
        elif args.implementation_sha is not None:
            validate_sha(args.implementation_sha)
    except RunnerError as error:
        parser.error(str(error))
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if not args.execute:
        print(json.dumps(build_plan(args), indent=2, sort_keys=True))
        return 0
    remote = Remote(args.ssh_target, args.api_port)
    try:
        report = execute(args, remote)
    except (RunnerError, subprocess.TimeoutExpired) as error:
        print(json.dumps({"schema": CHECKPOINT_SCHEMA, "status": "failed", "error": str(error)}))
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
