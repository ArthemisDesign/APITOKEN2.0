#!/usr/bin/env python3
"""Hermetic integration tests for the fixed Gemini 3.7 one-shot root gate."""

from __future__ import annotations

import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
GATE = ROOT / "deploy/gemini-3-7-admission-gate.sh"
TRANSPORT = ROOT / "deploy/gemini-3-7-admission-transport.py"
PACKAGE_INIT = ROOT / "tools/gemini_calibration/__init__.py"
ADMISSION = ROOT / "tools/gemini_calibration/admission.py"
RUN_LIVE = ROOT / "tools/gemini_calibration/run_live.py"
WATCHDOG_LIB = ROOT / "deploy/watchdog-lib.sh"
UNIT = ROOT / "systemd/claude-api-gemini-3-7-admission.service"
SHA = "264363f7838ddd2d156b14668a320047ad33b6ee"
PROFILE = "opaque-profile-test"
PLAN = "google_ai_pro"
ADMIN_KEY = "admin-key-do-not-print-0123456789abcdef"
PANEL_KEY = "panel-key-do-not-print-0123456789abcdef"
CREDENTIAL_DIGEST_A = "blake3:" + "a" * 64
CREDENTIAL_DIGEST_B = "blake3:" + "b" * 64
ENVELOPE_SECRET = "encrypted-envelope-do-not-print-0123456789abcdef"
UNUSED_ENVELOPE_SECRET = "unused-envelope-do-not-print-0123456789abcdef"


def official_rate() -> dict:
    return {
        "id": "gemini-3.7-flash",
        "tariff_schedule_id": "google/gemini-developer-api/2026-08-14",
        "input_token_limit": "1048576",
        "output_token_limit": "65536",
        "rates": {
            "input_nanousd_per_token": "750",
            "audio_input_nanousd_per_token": "750",
            "cached_input_nanousd_per_token": "75",
            "cached_audio_input_nanousd_per_token": "75",
            "output_nanousd_per_token": "3750",
            "image_output_nanousd_per_token": "0",
            "long_context_threshold": str(2**64 - 1),
            "long_input_nanousd_per_token": "750",
            "long_audio_input_nanousd_per_token": "750",
            "long_cached_input_nanousd_per_token": "75",
            "long_cached_audio_input_nanousd_per_token": "75",
            "long_output_nanousd_per_token": "3750",
        },
        "search": {"billing_unit": "query", "nanousd_per_unit": "14000000"},
    }


class AdmissionServer(ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(
        self,
        *,
        fail_generation: bool = False,
        credential_digests: list[str] | None = None,
    ) -> None:
        super().__init__(("127.0.0.1", 0), AdmissionHandler)
        self.fail_generation = fail_generation
        self.credential_digests = credential_digests or [CREDENTIAL_DIGEST_A]
        self.gemini_sub_reads = 0
        self.requests: list[tuple[str, str]] = []
        self.generation_request_id: str | None = None
        self.generation_profile: str | None = None
        self.priced_ts: int | None = None

    def capacity(self) -> dict:
        events = []
        if self.generation_request_id is not None and not self.fail_generation:
            priced = self.priced_ts or int(time.time())
            events.append(
                {
                    "request_id": self.generation_request_id,
                    "profile_id": self.generation_profile,
                    "model": "gemini-3.7-flash",
                    "service_tier": "default",
                    "inference_geo": "test",
                    "tariff_schedule_id": "google/gemini-developer-api/2026-08-14",
                    "priced_ts": str(priced),
                    "completed_at": str(priced),
                    "input_tokens": "2",
                    "audio_input_tokens": "0",
                    "cache_read_tokens": "0",
                    "cached_audio_input_tokens": "0",
                    "cache_write_5m_tokens": "0",
                    "cache_write_1h_tokens": "0",
                    "output_tokens": "2",
                    "thinking_output_tokens": "0",
                    "image_output_tokens": "0",
                    "tool_prompt_tokens": "0",
                    "search_queries": "0",
                    "grounded_search_prompts": "0",
                    "api_input_nanousd": "1500",
                    "api_audio_input_nanousd": "0",
                    "api_cache_read_nanousd": "0",
                    "api_cached_audio_input_nanousd": "0",
                    "api_cache_write_5m_nanousd": "0",
                    "api_cache_write_1h_nanousd": "0",
                    "api_output_nanousd": "7500",
                    "api_image_output_nanousd": "0",
                    "api_search_nanousd": "0",
                    "api_total_nanousd": "9000",
                }
            )
        return {
            "now": int(time.time()),
            "enabled": True,
            "calibration_authority_available": True,
            "calibration_delivery": {
                "pending_events": 0,
                "dropped_events": 0,
                "persistence_ok": True,
            },
            "calibration_recent_turn_limit": 512,
            "calibration_recent_turns": events,
            "profiles": [
                {
                    "id": PROFILE,
                    "plan": PLAN,
                    "authenticated": True,
                    "disabled": False,
                    "hidden": False,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "windows": [],
                    "quotas": [],
                }
            ],
            "conversion_models": [official_rate()],
        }

    def next_capacity(self) -> dict:
        capacity = self.capacity()
        index = min(self.gemini_sub_reads, len(self.credential_digests) - 1)
        capacity["credential_generation_digest"] = self.credential_digests[index]
        self.gemini_sub_reads += 1
        return capacity


class AdmissionHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args: object) -> None:
        return

    def respond(
        self,
        status: int,
        body: bytes,
        content_type: str = "application/json",
        *,
        dispatch: bool = False,
    ) -> None:
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        if dispatch:
            self.send_header(
                "x-apitoken-calibration-dispatch-ms",
                str(time.time_ns() // 1_000_000),
            )
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        self.server.requests.append(("GET", self.path))
        if self.path == "/ready":
            self.respond(200, b'{"ready":true}')
        elif self.path == "/gemini-subs":
            self.respond(200, json.dumps(self.server.next_capacity()).encode())
        else:
            self.respond(404, b'{"error":"not found"}')

    def do_POST(self) -> None:
        length = int(self.headers.get("content-length", "0"))
        if length:
            self.rfile.read(length)
        self.server.requests.append(("POST", self.path))
        if (
            self.headers.get_all("x-apitoken-calibration-profile") != [PROFILE]
            or self.headers.get_all("x-apitoken-calibration-not-after")
            != ["1798761600"]
        ):
            self.respond(400, b'{"error":"invalid private contract"}')
            return
        request_ids = self.headers.get_all("x-apitoken-calibration-request-id") or []
        if len(request_ids) != 1:
            self.respond(400, b'{"error":"invalid request identity"}')
            return
        if self.path.endswith(":countTokens"):
            self.respond(200, b'{"totalTokens":2}', dispatch=True)
            return
        if self.path.endswith(":streamGenerateContent?alt=sse"):
            self.server.generation_request_id = self.headers.get(
                "x-apitoken-calibration-request-id"
            )
            self.server.generation_profile = self.headers.get("x-apitoken-calibration-profile")
            self.server.priced_ts = int(time.time())
            if self.server.fail_generation:
                self.respond(503, b'{"error":"unavailable"}')
                return
            expected_output = " ".join(str(value) for value in range(1, 65))
            split = len(expected_output) // 2
            frames = [
                {
                    "modelVersion": "gemini-3.7-flash",
                    "candidates": [
                        {"content": {"parts": [{"text": expected_output[:split]}]}}
                    ],
                },
                {
                    "modelVersion": "gemini-3.7-flash",
                    "candidates": [
                        {
                            "content": {"parts": [{"text": expected_output[split:]}]},
                            "finishReason": "STOP",
                        }
                    ],
                    "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 2},
                },
            ]
            body = b"".join(
                b"data: " + json.dumps(frame, separators=(",", ":")).encode() + b"\n\n"
                for frame in frames
            )
            self.respond(200, body, "text/event-stream", dispatch=True)
            return
        self.respond(404, b'{"error":"not found"}')


class GateFixture:
    def __init__(
        self,
        *,
        fail_generation: bool = False,
        credential_digests: list[str] | None = None,
    ) -> None:
        self.temp = tempfile.TemporaryDirectory()
        # macOS exposes /var through /private/var. The production gate deliberately rejects any
        # noncanonical/symlink-bearing source path, so the fixture must pass the canonical root.
        self.root = Path(self.temp.name).resolve(strict=True)
        self.controller = self.root / "controller"
        self.package = self.controller / "gemini_calibration"
        self.release_root = self.root / "releases"
        self.release = self.release_root / SHA
        self.producer = self.root / "producer"
        self.profiles_source = self.root / "profiles.json"
        self.credentials_source = self.root / "credentials"
        self.credential_source = self.credentials_source / f"{PROFILE}.json"
        self.unused_credential_source = self.credentials_source / "unused-profile.json"
        self.proc = self.root / "proc"
        self.runtime = self.root / "run"
        self.state_parent = self.root / "state"
        self.mock_bin = self.root / "bin"
        self.system_state = self.root / "system-state"
        self.show_count = self.root / "show-count"
        self.command_log = self.root / "commands.log"
        self.snapshot_capture = self.root / "snapshot-capture.json"
        self.unit_file = self.root / "claude-api-gemini-3-7-admission.service"
        self.server = AdmissionServer(
            fail_generation=fail_generation,
            credential_digests=credential_digests,
        )
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.prepare_files()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        self.temp.cleanup()

    @staticmethod
    def install(source: Path, destination: Path, mode: int) -> None:
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        destination.chmod(mode)

    def executable(self, name: str, source: str) -> Path:
        path = self.mock_bin / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)
        return path

    def prepare_files(self) -> None:
        self.package.mkdir(parents=True, mode=0o755)
        self.package.chmod(0o755)
        self.controller.chmod(0o755)
        self.install(TRANSPORT, self.controller / TRANSPORT.name, 0o755)
        self.install(PACKAGE_INIT, self.package / "__init__.py", 0o644)
        self.install(ADMISSION, self.package / "admission.py", 0o644)
        self.install(RUN_LIVE, self.package / "run_live.py", 0o644)
        self.install(UNIT, self.unit_file, 0o644)

        self.release.mkdir(parents=True)
        self.install(Path(shutil.which("true") or "/usr/bin/true"), self.release / "claude-api", 0o755)
        (self.release / ".release-sha").write_text(SHA + "\n", encoding="ascii")
        (self.release_root / "current").symlink_to(self.release)

        self.producer.mkdir(mode=0o755)
        self.install(self.release / "claude-api", self.producer / "claude-api", 0o555)
        producer_digest = hashlib.sha256((self.producer / "claude-api").read_bytes()).hexdigest()
        (self.producer / "claude-api.sha256").write_text(producer_digest + "\n", encoding="ascii")
        (self.producer / "claude-api.sha256").chmod(0o444)
        (self.producer / ".release-sha").write_text(SHA + "\n", encoding="ascii")
        (self.producer / ".release-sha").chmod(0o444)
        self.producer.chmod(0o555)
        self.credentials_source.mkdir(mode=0o700)
        self.credentials_source.chmod(0o700)
        self.credential_source.write_text(
            json.dumps({"sealed": ENVELOPE_SECRET}, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        self.credential_source.chmod(0o600)
        self.unused_credential_source.write_text(
            json.dumps({"sealed": UNUSED_ENVELOPE_SECRET}, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        self.unused_credential_source.chmod(0o600)
        self.profiles_source.write_text(
            json.dumps(
                {
                    "profiles": [
                        {
                            "id": PROFILE,
                            "credential_file": str(self.credential_source),
                        }
                    ]
                },
                separators=(",", ":"),
            )
            + "\n",
            encoding="utf-8",
        )
        self.profiles_source.chmod(0o600)

        for pid in ("1001", "2001"):
            directory = self.proc / pid
            directory.mkdir(parents=True)
            executable = self.release / "claude-api" if pid == "1001" else self.producer / "claude-api"
            (directory / "exe").symlink_to(executable.resolve(strict=True))
            stat_fields = ["S", *("0" for _ in range(18)), str(10_000 + int(pid)), *("0" for _ in range(5))]
            (directory / "stat").write_text(
                f"{pid} (claude-api) " + " ".join(stat_fields) + "\n",
                encoding="ascii",
            )
        (self.proc / "1001" / "environ").write_bytes(
            b"CLAUDE_API_KEYS=" + ADMIN_KEY.encode() + b"\0"
            b"CLAUDE_API_PANEL_KEY=" + PANEL_KEY.encode() + b"\0"
        )
        self.runtime.mkdir(mode=0o700)
        self.state_parent.mkdir(mode=0o700)
        lock_file = self.state_parent / "gate.lock"
        lock_file.write_bytes(b"")
        lock_file.chmod(0o600)
        self.mock_bin.mkdir()
        self.system_state.write_text("inactive\n", encoding="ascii")
        self.show_count.write_text("0\n", encoding="ascii")
        self.command_log.write_text("", encoding="utf-8")

        self.systemctl = self.executable(
            "systemctl",
            """#!/usr/bin/env python3
import hashlib, json, os, pathlib, stat, sys
state = pathlib.Path(os.environ['GATE_SYSTEM_STATE'])
show_count_path = pathlib.Path(os.environ['GATE_SHOW_COUNT'])
log = pathlib.Path(os.environ['GATE_COMMAND_LOG'])
with log.open('a') as output: output.write('systemctl ' + ' '.join(sys.argv[1:]) + '\\n')
args = sys.argv[1:]
command = args[0]
unit = (args[1] if command == 'show' else args[-1]) if len(args) > 1 else ''
active = state.read_text().strip() == 'active'
if command == 'is-active':
    ok = unit == 'claude-api-gemini@8795.service' or (unit == 'claude-api-gemini-3-7-admission.service' and active)
    raise SystemExit(0 if ok else 3)
if command == 'is-enabled':
    print('static'); raise SystemExit(0)
if command == 'show':
    property_name = next((value.split('=', 1)[1] for value in args if value.startswith('--property=')), '')
    show_count = int(show_count_path.read_text())
    if unit == 'claude-api-gemini-3-7-admission.service' and property_name in {'FragmentPath', 'DropInPaths', 'NeedDaemonReload'}:
        show_count += 1
        show_count_path.write_text(str(show_count) + '\\n')
    if property_name == 'FragmentPath': print(os.environ['GATE_UNIT_FILE'])
    elif property_name == 'DropInPaths':
        after = int(os.environ.get('GATE_DROPINS_AFTER_SHOW_COUNT', '0'))
        print('/run/systemd/system/foreign.conf' if after and show_count > after else os.environ.get('GATE_DROPINS', ''))
    elif property_name == 'NeedDaemonReload': print('no')
    else: print('2001' if unit == 'claude-api-gemini-3-7-admission.service' else '1001')
    raise SystemExit(0)
if command == 'start':
    snapshot = pathlib.Path(os.environ['GATE_RUNTIME_PARENT']) / 'apitoken-gemini-3-7-admission'
    roster = json.loads((snapshot / 'profiles.json').read_text())
    credentials = snapshot / 'credentials'
    files = sorted(path for path in credentials.iterdir())
    capture = {
        'roster': roster,
        'credential_files': [path.name for path in files],
        'credential_sha256': [hashlib.sha256(path.read_bytes()).hexdigest() for path in files],
        'credential_modes': [format(stat.S_IMODE(path.stat().st_mode), 'o') for path in files],
        'credential_directory_mode': format(stat.S_IMODE(credentials.stat().st_mode), 'o'),
        'roster_mode': format(stat.S_IMODE((snapshot / 'profiles.json').stat().st_mode), 'o'),
    }
    pathlib.Path(os.environ['GATE_SNAPSHOT_CAPTURE']).write_text(json.dumps(capture))
    state.write_text('active\\n'); raise SystemExit(0)
if command in {'stop', 'kill'}:
    state.write_text('inactive\\n'); raise SystemExit(0)
raise SystemExit(1)
""",
        )
        self.stat = self.executable(
            "stat",
            """#!/usr/bin/env python3
import os, stat, sys
fmt = sys.argv[sys.argv.index('-c') + 1]
path = sys.argv[-1]
value = os.lstat(path)
fields = {
    '%u': str(value.st_uid),
    '%g': str(value.st_gid),
    '%a': format(stat.S_IMODE(value.st_mode), 'o'),
    '%h': str(value.st_nlink),
}
for key, replacement in fields.items(): fmt = fmt.replace(key, replacement)
print(fmt)
""",
        )
        self.readlink = self.executable(
            "readlink",
            """#!/usr/bin/env python3
from pathlib import Path
import sys
print(Path(sys.argv[-1]).resolve(strict=True))
""",
        )
        self.curl = self.executable(
            "curl",
            """#!/usr/bin/env python3
import os, pathlib, sys
with pathlib.Path(os.environ['GATE_COMMAND_LOG']).open('a') as output:
    output.write('curl ' + ' '.join(sys.argv[1:]) + '\\n')
print('{"ready":true}')
""",
        )
        self.ss = self.executable(
            "ss",
            """#!/usr/bin/env python3
import os, pathlib, sys
with pathlib.Path(os.environ['GATE_COMMAND_LOG']).open('a') as output:
    output.write('ss ' + ' '.join(sys.argv[1:]) + '\\n')
if pathlib.Path(os.environ['GATE_SYSTEM_STATE']).read_text().strip() == 'active':
    print('LISTEN 0 128 127.0.0.1:' + os.environ['GATE_CANARY_PORT'])
""",
        )
        self.timeout = self.executable(
            "timeout",
            """#!/usr/bin/env python3
import os, sys
os.execv(sys.argv[2], sys.argv[2:])
""",
        )

    def environment(self) -> dict[str, str]:
        commands = {
            "SHA256SUM": shutil.which("sha256sum"),
            "PYTHON": shutil.which("python3"),
            "SLEEP": shutil.which("true"),
            "RM": shutil.which("rm"),
            "RMDIR": shutil.which("rmdir"),
        }
        if any(value is None for value in commands.values()):
            raise unittest.SkipTest("required local coreutils are unavailable")
        env = os.environ.copy()
        env.update(
            {
                "GEMINI_3_7_ADMISSION_GATE_TESTING": "1",
                "GEMINI_3_7_ADMISSION_GATE_TEST_LIB": str(WATCHDOG_LIB),
                "GEMINI_3_7_ADMISSION_GATE_TEST_CONTROLLER_ROOT": str(self.controller),
                "GEMINI_3_7_ADMISSION_GATE_TEST_UNIT_FILE": str(self.unit_file),
                "GEMINI_3_7_ADMISSION_GATE_TEST_RELEASE_ROOT": str(self.release_root),
                "GEMINI_3_7_ADMISSION_GATE_TEST_PRODUCER_ROOT": str(self.producer),
                "GEMINI_3_7_ADMISSION_GATE_TEST_STATE_TRUST_ROOT": str(self.root),
                "GEMINI_3_7_ADMISSION_GATE_TEST_STATE_PARENT": str(self.state_parent),
                "GEMINI_3_7_ADMISSION_GATE_TEST_PROC_ROOT": str(self.proc),
                "GEMINI_3_7_ADMISSION_GATE_TEST_RUNTIME_PARENT": str(self.runtime),
                "GEMINI_3_7_ADMISSION_GATE_TEST_PROFILES_SOURCE": str(self.profiles_source),
                "GEMINI_3_7_ADMISSION_GATE_TEST_SYSTEMCTL": str(self.systemctl),
                "GEMINI_3_7_ADMISSION_GATE_TEST_CURL": str(self.curl),
                "GEMINI_3_7_ADMISSION_GATE_TEST_SS": str(self.ss),
                "GEMINI_3_7_ADMISSION_GATE_TEST_STAT": str(self.stat),
                "GEMINI_3_7_ADMISSION_GATE_TEST_SHA256SUM": str(commands["SHA256SUM"]),
                "GEMINI_3_7_ADMISSION_GATE_TEST_READLINK": str(self.readlink),
                "GEMINI_3_7_ADMISSION_GATE_TEST_PYTHON": str(commands["PYTHON"]),
                "GEMINI_3_7_ADMISSION_GATE_TEST_SLEEP": str(commands["SLEEP"]),
                "GEMINI_3_7_ADMISSION_GATE_TEST_TIMEOUT": str(self.timeout),
                "GEMINI_3_7_ADMISSION_GATE_TEST_RM": str(commands["RM"]),
                "GEMINI_3_7_ADMISSION_GATE_TEST_RMDIR": str(commands["RMDIR"]),
                "GEMINI_3_7_ADMISSION_GATE_TEST_STABLE_PORT": str(
                    self.server.server_port
                ),
                "GEMINI_3_7_ADMISSION_GATE_TEST_CANARY_PORT": str(self.server.server_port),
                "GATE_SYSTEM_STATE": str(self.system_state),
                "GATE_SHOW_COUNT": str(self.show_count),
                "GATE_COMMAND_LOG": str(self.command_log),
                "GATE_RUNTIME_PARENT": str(self.runtime),
                "GATE_SNAPSHOT_CAPTURE": str(self.snapshot_capture),
                "GATE_CANARY_PORT": str(self.server.server_port),
                "GATE_UNIT_FILE": str(self.unit_file),
                "GATE_DROPINS": "",
            }
        )
        return env

    def run(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(GATE), SHA],
            cwd=ROOT,
            env=self.environment(),
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )


class GateTests(unittest.TestCase):
    def assert_private_output(self, result: subprocess.CompletedProcess[str]) -> None:
        combined = result.stdout + result.stderr
        self.assertNotIn(PROFILE, combined)
        self.assertNotIn(ADMIN_KEY, combined)
        self.assertNotIn(PANEL_KEY, combined)
        self.assertNotIn(ENVELOPE_SECRET, combined)
        self.assertNotIn(UNUSED_ENVELOPE_SECRET, combined)

    def assert_reentry_is_command_free(self, fixture: GateFixture, before: str) -> None:
        self.assertEqual(fixture.command_log.read_text(), before)

    def assert_permanent_pre_dispatch_withdrawal(
        self,
        fixture: GateFixture,
        result: subprocess.CompletedProcess[str],
        *,
        canary_started: bool = False,
    ) -> None:
        self.assertNotEqual(result.returncode, 0)
        self.assert_private_output(result)
        self.assertTrue((fixture.state_parent / SHA).is_dir())
        self.assertEqual(
            [path for method, path in fixture.server.requests if method == "POST"],
            [],
        )
        if not canary_started:
            self.assertNotIn(
                "systemctl start claude-api-gemini-3-7-admission.service",
                fixture.command_log.read_text(),
            )
        command_log = fixture.command_log.read_text()
        request_count = len(fixture.server.requests)
        second = fixture.run()
        self.assertNotEqual(second.returncode, 0)
        self.assert_private_output(second)
        self.assertEqual(len(fixture.server.requests), request_count)
        self.assert_reentry_is_command_free(fixture, command_log)

    def test_static_unit_uses_transient_identity_and_systemd_flat_credentials(self) -> None:
        unit = UNIT.read_text(encoding="utf-8")
        lines = unit.splitlines()
        self.assertIn("DynamicUser=yes", lines)
        self.assertFalse(any(line.startswith("User=") for line in lines))
        self.assertFalse(any(line.startswith("Group=") for line in lines))
        self.assertIn(
            "LoadCredential=gemini-profiles:"
            "/run/apitoken-gemini-3-7-admission/profiles.json",
            lines,
        )
        self.assertIn(
            "LoadCredential=gemini-credential:"
            "/run/apitoken-gemini-3-7-admission/credentials",
            lines,
        )
        self.assertIn("InaccessiblePaths=/srv/claude-api/data/gemini", lines)
        self.assertIn("CLAUDE_API_GEMINI_CREDENTIAL_LAYOUT=systemd-flat", unit)
        self.assertIn("CLAUDE_API_GEMINI_MODELS=gemini-3.7-flash", unit)
        self.assertIn(SHA, unit)
        self.assertNotIn("[Install]", lines)

    def test_gate_is_direct_root_only_without_sudo_dispatch_authority(self) -> None:
        gate = GATE.read_text(encoding="utf-8")
        self.assertIn("EUID -ne 0", gate)
        self.assertIn("must run directly as root", gate)
        self.assertIn("BUDGET_NANOUSD=1574784000", gate)
        self.assertNotIn("BUDGET_NANOUSD=787392000", gate)
        for forbidden in (
            "SUDO=",
            "SUDOERS",
            "GEMINI_3_7_ADMISSION_GATE_TEST_SUDO",
            "/usr/bin/sudo",
            "trigger-authorized",
            "fixed root bridge",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, gate)

    def test_production_mode_rejects_non_root_before_paths_or_network(self) -> None:
        if os.geteuid() == 0:
            raise unittest.SkipTest("non-root production-entry check requires non-root")
        fixture = GateFixture()
        try:
            environment = fixture.environment()
            environment.pop("GEMINI_3_7_ADMISSION_GATE_TESTING")
            result = subprocess.run(
                [str(GATE), SHA],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must run directly as root", result.stderr)
            self.assert_private_output(result)
            self.assertFalse((fixture.state_parent / SHA).exists())
            self.assertEqual(fixture.server.requests, [])
            self.assertEqual(fixture.command_log.read_text(), "")
        finally:
            fixture.close()

    def test_wrong_producer_sha_stops_before_commands_or_network(self) -> None:
        fixture = GateFixture()
        try:
            result = subprocess.run(
                [str(GATE), "a" * 40],
                cwd=ROOT,
                env=fixture.environment(),
                text=True,
                capture_output=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"admission is pinned to {SHA}", result.stderr)
            self.assert_private_output(result)
            self.assertFalse((fixture.state_parent / SHA).exists())
            self.assertEqual(fixture.server.requests, [])
            self.assertEqual(fixture.command_log.read_text(), "")
        finally:
            fixture.close()

    def test_success_is_exactly_once_and_reentry_is_offline_inspection(self) -> None:
        fixture = GateFixture()
        try:
            result = fixture.run()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_private_output(result)
            summary = json.loads(result.stdout)
            self.assertEqual(summary["state"], "success")
            self.assertEqual(summary["actual_nanousd"], "9000")
            self.assertEqual(summary["budget_nanousd"], "1574784000")
            self.assertEqual(summary["upper_bound_nanousd"], "1574784000")
            self.assertGreater(summary["count_dispatch_ms"], 0)
            self.assertLess(summary["count_dispatch_ms"], 1_798_761_600_000)
            self.assertGreater(summary["generation_dispatch_ms"], 0)
            self.assertLess(summary["generation_dispatch_ms"], 1_798_761_600_000)
            self.assertTrue(
                summary["response_evidence"]["raw_upstream_model_version"]
            )
            posts = [path for method, path in fixture.server.requests if method == "POST"]
            self.assertEqual(len(posts), 2)
            self.assertTrue(posts[0].endswith(":countTokens"))
            self.assertTrue(posts[1].endswith(":streamGenerateContent?alt=sse"))
            self.assertEqual(fixture.system_state.read_text().strip(), "inactive")
            capture = json.loads(fixture.snapshot_capture.read_text())
            self.assertEqual(
                capture["roster"],
                {
                    "profiles": [
                        {
                            "id": PROFILE,
                            "credential_file": (
                                "/run/credentials/"
                                "claude-api-gemini-3-7-admission.service/"
                                f"gemini-credential_{PROFILE}.json"
                            ),
                        }
                    ]
                },
            )
            self.assertEqual(capture["credential_files"], [f"{PROFILE}.json"])
            self.assertEqual(
                capture["credential_sha256"],
                [hashlib.sha256(fixture.credential_source.read_bytes()).hexdigest()],
            )
            self.assertEqual(capture["credential_modes"], ["400"])
            self.assertEqual(capture["credential_directory_mode"], "500")
            self.assertEqual(capture["roster_mode"], "400")

            command_log = fixture.command_log.read_text()
            request_count = len(fixture.server.requests)
            second = fixture.run()
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assert_private_output(second)
            self.assertEqual(json.loads(second.stdout)["state"], "success")
            self.assert_reentry_is_command_free(fixture, command_log)
            self.assertEqual(len(fixture.server.requests), request_count)
        finally:
            fixture.close()

    def test_invalid_test_mode_cannot_reach_commands_or_network(self) -> None:
        fixture = GateFixture()
        try:
            environment = fixture.environment()
            environment["GEMINI_3_7_ADMISSION_GATE_TESTING"] = "unexpected"
            result = subprocess.run(
                [str(GATE), SHA],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assert_private_output(result)
            self.assertEqual(fixture.command_log.read_text(), "")
            self.assertEqual(fixture.server.requests, [])
        finally:
            fixture.close()

    def test_sourced_library_digest_drift_is_rejected_before_commands_or_network(self) -> None:
        fixture = GateFixture()
        try:
            library_root = fixture.root / "trusted-lib"
            library_root.mkdir(mode=0o755)
            library_root.chmod(0o755)
            tampered = library_root / "watchdog-lib.sh"
            tampered.write_bytes(WATCHDOG_LIB.read_bytes() + b"\n# digest drift\n")
            tampered.chmod(0o755)
            environment = fixture.environment()
            environment["GEMINI_3_7_ADMISSION_GATE_TEST_LIB"] = str(tampered)
            result = subprocess.run(
                [str(GATE), SHA],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("watchdog library content drifted", result.stderr)
            self.assert_private_output(result)
            self.assertEqual(fixture.command_log.read_text(), "")
            self.assertEqual(fixture.server.requests, [])
        finally:
            fixture.close()

    def test_kernel_lock_blocks_concurrent_gate_before_commands_or_network(self) -> None:
        fixture = GateFixture()
        try:
            with (fixture.state_parent / "gate.lock").open("r+b") as lock_file:
                fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                result = fixture.run()
            self.assertNotEqual(result.returncode, 0)
            self.assert_private_output(result)
            self.assertEqual(fixture.command_log.read_text(), "")
            self.assertEqual(fixture.server.requests, [])
        finally:
            fixture.close()

    def test_effective_unit_dropin_is_rejected_before_canary_start(self) -> None:
        fixture = GateFixture()
        try:
            environment = fixture.environment()
            environment["GATE_DROPINS"] = "/run/systemd/system/foreign.conf"
            result = subprocess.run(
                [str(GATE), SHA],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                timeout=15,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assert_private_output(result)
            self.assertNotIn(
                "systemctl start claude-api-gemini-3-7-admission.service",
                fixture.command_log.read_text(),
            )
            self.assertEqual(fixture.server.requests, [])
        finally:
            fixture.close()

    def test_profile_roster_symlink_is_rejected_before_canary_start(self) -> None:
        fixture = GateFixture()
        try:
            alternate = fixture.root / "alternate-profiles.json"
            alternate.write_text('{"profiles":[]}\n', encoding="utf-8")
            fixture.profiles_source.unlink()
            fixture.profiles_source.symlink_to(alternate)
            result = fixture.run()
            self.assert_permanent_pre_dispatch_withdrawal(fixture, result)
        finally:
            fixture.close()

    def test_noncanonical_or_escaping_credential_path_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture()
        try:
            fixture.profiles_source.write_text(
                json.dumps(
                    {
                        "profiles": [
                            {
                                "id": PROFILE,
                                "credential_file": str(
                                    fixture.credentials_source / ".." / f"{PROFILE}.json"
                                ),
                            }
                        ]
                    },
                    separators=(",", ":"),
                )
                + "\n",
                encoding="utf-8",
            )
            fixture.profiles_source.chmod(0o600)
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_source_mode_drift_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture()
        try:
            fixture.credential_source.chmod(0o640)
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_envelope_symlink_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture()
        try:
            alternate = fixture.root / "alternate-envelope.json"
            shutil.copyfile(fixture.credential_source, alternate)
            alternate.chmod(0o600)
            fixture.credential_source.unlink()
            fixture.credential_source.symlink_to(alternate)
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_duplicate_profile_id_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture()
        try:
            profile = {"id": PROFILE, "credential_file": str(fixture.credential_source)}
            fixture.profiles_source.write_text(
                json.dumps({"profiles": [profile, profile]}, separators=(",", ":")) + "\n",
                encoding="utf-8",
            )
            fixture.profiles_source.chmod(0o600)
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_duplicate_credential_path_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture()
        try:
            fixture.profiles_source.write_text(
                json.dumps(
                    {
                        "profiles": [
                            {"id": PROFILE, "credential_file": str(fixture.credential_source)},
                            {
                                "id": "second-profile",
                                "credential_file": str(fixture.credential_source),
                            },
                        ]
                    },
                    separators=(",", ":"),
                )
                + "\n",
                encoding="utf-8",
            )
            fixture.profiles_source.chmod(0o600)
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_oversized_roster_or_envelope_is_permanently_withdrawn(self) -> None:
        for target in ("roster", "envelope"):
            with self.subTest(target=target):
                fixture = GateFixture()
                try:
                    path = (
                        fixture.profiles_source
                        if target == "roster"
                        else fixture.credential_source
                    )
                    path.write_bytes(b"{" + b" " * (1024 * 1024) + b"}")
                    path.chmod(0o600)
                    self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
                finally:
                    fixture.close()

    def test_stable_digest_change_during_snapshot_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture(credential_digests=[CREDENTIAL_DIGEST_A, CREDENTIAL_DIGEST_B])
        try:
            self.assert_permanent_pre_dispatch_withdrawal(fixture, fixture.run())
        finally:
            fixture.close()

    def test_canary_digest_mismatch_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture(
            credential_digests=[
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_B,
            ]
        )
        try:
            self.assert_permanent_pre_dispatch_withdrawal(
                fixture,
                fixture.run(),
                canary_started=True,
            )
        finally:
            fixture.close()

    def test_stable_digest_change_after_canary_start_is_permanently_withdrawn(self) -> None:
        fixture = GateFixture(
            credential_digests=[
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_B,
            ]
        )
        try:
            self.assert_permanent_pre_dispatch_withdrawal(
                fixture,
                fixture.run(),
                canary_started=True,
            )
        finally:
            fixture.close()

    def test_stable_digest_change_before_paid_dispatch_blocks_generation(self) -> None:
        fixture = GateFixture(
            credential_digests=[
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_A,
                CREDENTIAL_DIGEST_B,
            ]
        )
        try:
            result = fixture.run()
            self.assertNotEqual(result.returncode, 0)
            self.assert_private_output(result)
            posts = [path for method, path in fixture.server.requests if method == "POST"]
            self.assertEqual(len(posts), 1)
            self.assertTrue(posts[0].endswith(":countTokens"))
            self.assertTrue((fixture.state_parent / SHA).is_dir())
            command_log = fixture.command_log.read_text()
            request_count = len(fixture.server.requests)
            second = fixture.run()
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(len(fixture.server.requests), request_count)
            self.assert_reentry_is_command_free(fixture, command_log)
        finally:
            fixture.close()

    def test_effective_unit_drift_after_count_stops_before_paid_generation(self) -> None:
        fixture = GateFixture()
        try:
            environment = fixture.environment()
            environment["GATE_DROPINS_AFTER_SHOW_COUNT"] = "9"
            result = subprocess.run(
                [str(GATE), SHA],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                timeout=20,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assert_private_output(result)
            posts = [path for method, path in fixture.server.requests if method == "POST"]
            self.assertEqual(len(posts), 1)
            self.assertTrue(posts[0].endswith(":countTokens"))
        finally:
            fixture.close()

    def test_success_without_cleanup_attestation_cannot_be_reaccepted(self) -> None:
        fixture = GateFixture()
        try:
            first = fixture.run()
            self.assertEqual(first.returncode, 0, first.stderr)
            marker = fixture.state_parent / SHA / "cleanup.complete"
            self.assertTrue(marker.is_file())
            marker.unlink()
            command_log = fixture.command_log.read_text()
            request_count = len(fixture.server.requests)

            second = fixture.run()
            self.assertNotEqual(second.returncode, 0)
            self.assert_private_output(second)
            self.assert_reentry_is_command_free(fixture, command_log)
            self.assertEqual(len(fixture.server.requests), request_count)
        finally:
            fixture.close()

    def test_failed_generation_is_permanent_and_never_replayed(self) -> None:
        fixture = GateFixture(fail_generation=True)
        try:
            first = fixture.run()
            self.assertNotEqual(first.returncode, 0)
            self.assert_private_output(first)
            posts = [path for method, path in fixture.server.requests if method == "POST"]
            self.assertEqual(len(posts), 2)
            request_count = len(fixture.server.requests)
            command_log = fixture.command_log.read_text()

            second = fixture.run()
            self.assertNotEqual(second.returncode, 0)
            self.assert_private_output(second)
            self.assertEqual(len(fixture.server.requests), request_count)
            self.assert_reentry_is_command_free(fixture, command_log)
        finally:
            fixture.close()


if __name__ == "__main__":
    unittest.main()
