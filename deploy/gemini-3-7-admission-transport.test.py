import contextlib
import importlib.util
import io
import json
import os
import socket
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("gemini-3-7-admission-transport.py")
SOURCE_PACKAGE = SCRIPT.parents[1] / "tools" / "gemini_calibration"
SPEC = importlib.util.spec_from_file_location("gemini_3_7_admission_transport", SCRIPT)
transport = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = transport
SPEC.loader.exec_module(transport)


ADMIN_KEY = "admin-secret-never-print"
PANEL_KEY = "panel-secret-never-print"
PROFILE = "profile-opaque-a"
PLAN = "google_ai_pro"
MODEL = "gemini-3.7-flash"
COUNT_ID = "123e4567-e89b-42d3-a456-426614174000"
PAID_ID = "223e4567-e89b-42d3-a456-426614174000"
SHA = "0123456789abcdef0123456789abcdef01234567"
PROMO_EPOCH = 1_786_665_600
NOT_AFTER = 1_798_761_600
DISPATCH_MS = PROMO_EPOCH * 1000 + 123
PROMPT = "Output the integers 1 through 64, separated by single spaces, and nothing else."
EXPECTED_OUTPUT = " ".join(str(value) for value in range(1, 65))


def event():
    return {
        "request_id": PAID_ID,
        "profile_id": PROFILE,
        "model": MODEL,
        "tariff_schedule_id": "google/gemini-test/2026-08-14",
        "priced_ts": str(PROMO_EPOCH),
        "completed_at": str(PROMO_EPOCH),
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
        "api_input_nanousd": "2",
        "api_audio_input_nanousd": "0",
        "api_cache_read_nanousd": "0",
        "api_cached_audio_input_nanousd": "0",
        "api_cache_write_5m_nanousd": "0",
        "api_cache_write_1h_nanousd": "0",
        "api_output_nanousd": "2",
        "api_image_output_nanousd": "0",
        "api_search_nanousd": "0",
        "api_total_nanousd": "4",
    }


def capacity_payload(with_event=False):
    return {
        "calibration_authority_available": True,
        "calibration_delivery": {
            "pending_events": 0,
            "dropped_events": 0,
            "persistence_ok": True,
        },
        "calibration_recent_turn_limit": 512,
        "calibration_recent_turns": [event()] if with_event else [],
        "profiles": [
            {
                "id": PROFILE,
                "plan": PLAN,
                "authenticated": True,
                "cooling_until": 0,
                "calibration_persistence_ok": True,
                "windows": [],
            }
        ],
        "conversion_models": [],
    }


def sse_body():
    split = len(EXPECTED_OUTPUT) // 2
    frames = [
        {
            "modelVersion": MODEL,
            "candidates": [
                {"content": {"parts": [{"text": EXPECTED_OUTPUT[:split]}]}}
            ],
        },
        {
            "modelVersion": MODEL,
            "candidates": [
                {
                    "content": {"parts": [{"text": EXPECTED_OUTPUT[split:]}]},
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 2},
        },
    ]
    return b"".join(
        b"data: " + json.dumps(frame, separators=(",", ":")).encode() + b"\n\n"
        for frame in frames
    )


class ScenarioServer(ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), ScenarioHandler)
        self.requests = []
        self.responses = {}
        self.truncated_responses = {}
        self.raw_responses = {}
        self.capacity_calls = 0


class ScenarioHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        return

    def _capture(self):
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length) if length else b""
        self.server.requests.append(
            (self.command, self.path, dict(self.headers.items()), body)
        )
        return body

    def _respond(self, status, body, content_type="application/json", extra=None):
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        if self.command == "POST" and status == 200:
            self.send_header(transport.CALIBRATION_DISPATCH_HEADER, str(DISPATCH_MS))
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._capture()
        configured = self.server.raw_responses.get(self.path)
        if configured is not None:
            status, headers, wire_body = configured
            self.send_response(status)
            for name, value in headers:
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(wire_body)
            self.wfile.flush()
            self.close_connection = True
            return
        if self.path == "/ready":
            self._respond(200, b'{"ready":true}')
            return
        if self.path == "/gemini-subs":
            self.server.capacity_calls += 1
            payload = capacity_payload(with_event=self.server.capacity_calls > 1)
            self._respond(200, json.dumps(payload).encode())
            return
        self._respond(404, b'{"error":"not found"}')

    def do_POST(self):
        self._capture()
        configured_raw = self.server.raw_responses.get(self.path)
        if configured_raw is not None:
            status, headers, wire_body = configured_raw
            self.send_response(status)
            for name, value in headers:
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(wire_body)
            self.wfile.flush()
            self.close_connection = True
            return
        truncated = self.server.truncated_responses.get(self.path)
        if truncated is not None:
            status, body, content_type, advertised_length = truncated
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(advertised_length))
            self.send_header("connection", "close")
            self.end_headers()
            self.wfile.write(body)
            self.wfile.flush()
            self.close_connection = True
            return
        configured = self.server.responses.get(self.path)
        if configured is not None:
            self._respond(*configured)
            return
        if self.path.endswith(":countTokens"):
            self._respond(200, b'{"totalTokens":2}')
            return
        if self.path.endswith(":streamGenerateContent?alt=sse"):
            self._respond(200, sse_body(), "text/event-stream")
            return
        self._respond(404, b'{"error":"not found"}')


class FakeCalibrationError(RuntimeError):
    pass


class FakeGenerationResponse:
    def __init__(self, frames=(), parse_error=None):
        self.frames = tuple(frames)
        self.parse_error = parse_error


class FakeRunLive:
    CalibrationError = FakeCalibrationError

    @staticmethod
    def decode_generation_response(raw, stream):
        try:
            frames = []
            for block in raw.decode().split("\n\n"):
                if block.startswith("data: "):
                    frames.append(json.loads(block[6:]))
            if not stream or not frames:
                raise ValueError
            return FakeGenerationResponse(frames)
        except (UnicodeError, json.JSONDecodeError, ValueError):
            return FakeGenerationResponse(parse_error="invalid SSE")

    @staticmethod
    def require_healthy_delivery(payload):
        if payload.get("calibration_authority_available") is not True:
            raise FakeCalibrationError("unhealthy")

    @staticmethod
    def exact_new_turn(_before, payload, request_id, profile_id, model):
        matches = [
            value
            for value in payload.get("calibration_recent_turns", [])
            if value.get("request_id") == request_id
            and value.get("profile_id") == profile_id
            and value.get("model") == model
        ]
        return matches[0] if len(matches) == 1 else None


class FakeAdmission:
    COUNT_REQUEST = "count-request.json"
    GENERATION_REQUEST = "generation-request.json"
    COUNT_OBSERVATION_SCHEMA = "gemini-3.7-admission-count-observation/v3"
    OBSERVATION_SCHEMA = "gemini-3.7-admission-observation/v2"
    MODEL = MODEL
    run_live = FakeRunLive

    def __init__(self, evidence, stream=True):
        self.evidence = evidence
        self.stream = stream
        self.state = "awaiting_count_tokens"
        self.actions = []
        self.count_observation = None
        self.outcome_observation = None
        self.count_request = {
            "schema": "gemini-3.7-admission-request/v3",
            "kind": "count_tokens",
            "method": "POST",
            "path": f"/v1beta/models/{MODEL}:countTokens",
            "target_profile": PROFILE,
            "request_id": COUNT_ID,
            "not_after": NOT_AFTER,
            "body": {
                "contents": [{
                    "role": "user",
                    "parts": [{"text": PROMPT}],
                }]
            },
        }
        self.generation_request = {
            "schema": "gemini-3.7-admission-request/v3",
            "kind": "generation",
            "method": "POST",
            "path": f"/v1beta/models/{MODEL}:streamGenerateContent?alt=sse",
            "target_profile": PROFILE,
            "calibration_request_id": PAID_ID,
            "not_after": NOT_AFTER,
            "body": {
                "contents": [{
                    "role": "user",
                    "parts": [{"text": PROMPT}],
                }],
            "generationConfig": {"maxOutputTokens": 256},
            },
        }
        self.journal = {
            "state": self.state,
            "model": MODEL,
            "stream": stream,
            "profile_id": PROFILE,
            "plan": PLAN,
            "count_request_id": COUNT_ID,
            "count_request_sha256": "1" * 64,
            "request_id": PAID_ID,
            "generation_request_sha256": "2" * 64,
            "not_after": NOT_AFTER,
        }
        self._write(self.evidence / self.COUNT_REQUEST, self.count_request)
        self._write(self.evidence / self.GENERATION_REQUEST, self.generation_request)

    @staticmethod
    def _write(path, value):
        path.write_text(json.dumps(value) + "\n", encoding="utf-8")
        os.chmod(path, 0o600)

    def _load_journal(self, _directory):
        self.journal["state"] = self.state
        return dict(self.journal)

    def _count_request(self, _journal):
        return self.count_request

    def _generation_request(self, _journal):
        return self.generation_request

    @staticmethod
    def _read_json(path, required_mode=None):
        if required_mode is not None and (path.stat().st_mode & 0o777) != required_mode:
            raise AssertionError("bad mode")
        return json.loads(path.read_text())

    @staticmethod
    def _validate_canonical_request(_path, _expected, _digest, _label):
        return

    def claim_count(self, _directory):
        if self.state != "awaiting_count_tokens":
            raise FakeCalibrationError("closed")
        self.actions.append("claim_count")
        self.state = "count_tokens_claimed"

    def record_count(self, _directory, observation):
        if self.state != "count_tokens_claimed":
            raise FakeCalibrationError("closed")
        self.count_observation = json.loads(observation.read_text())
        self.actions.append("record_count")
        self.state = "counted" if self.count_observation["http_status"] == 200 else "withdrawn_count_tokens"
        if self.state != "counted":
            raise FakeCalibrationError("withdrawn")
        return self.inspect(_directory)

    def claim_generation(self, _directory):
        if self.state != "generation_armed":
            raise FakeCalibrationError("closed")
        self.actions.append("claim_generation")
        self.state = "generation_claimed"

    def record_outcome(self, _directory, observation):
        if self.state != "generation_claimed":
            raise FakeCalibrationError("closed")
        self.outcome_observation = json.loads(observation.read_text())
        self.actions.append("record_outcome")
        response = self.outcome_observation.get("response")
        self.state = (
            "success"
            if self.outcome_observation["http_status"] == 200
            and isinstance(response, list)
            and bool(response)
            else "withdrawn_evidence"
        )
        if self.state != "success":
            raise FakeCalibrationError("withdrawn")
        return self.inspect(_directory)

    def inspect(self, _directory):
        return {
            "schema": "gemini-3.7-admission-summary/v3",
            "state": self.state,
            "model": MODEL,
            "implementation_sha": SHA,
            "release_sha": SHA,
        }


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        os.chmod(self.root, 0o700)
        self.evidence = self.root / "evidence"
        self.evidence.mkdir(mode=0o700)
        self.library = self.root / "library"
        (self.library / "gemini_calibration").mkdir(parents=True)
        (self.library / "gemini_calibration" / "admission.py").write_text("# fake\n")
        self.server = ScenarioServer()
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.env = mock.patch.dict(
            os.environ,
            {
                transport.TESTING_ENV: "1",
                transport.TEST_PORT_ENV: str(self.server.server_port),
            },
            clear=False,
        )
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        self.temp.cleanup()

    def run_main(self, argv, fake=None, secrets=None):
        stdout, stderr = io.StringIO(), io.StringIO()
        patches = [
            mock.patch.object(transport, "_load_admission", return_value=fake),
            mock.patch.dict(os.environ, secrets or {}, clear=False),
        ]
        with patches[0], patches[1], contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = transport.main(argv)
        output = stdout.getvalue() + stderr.getvalue()
        self.assertNotIn(ADMIN_KEY, output)
        self.assertNotIn(PANEL_KEY, output)
        self.assertNotIn(PROFILE, output)
        return code, stdout.getvalue(), stderr.getvalue()

    def exact_library(self, name):
        library = self.root / name
        package = library / "gemini_calibration"
        package.mkdir(parents=True)
        for filename in ("__init__.py", "admission.py", "run_live.py"):
            target = package / filename
            target.write_bytes((SOURCE_PACKAGE / filename).read_bytes())
            os.chmod(target, 0o644)
        return library, package

    @contextlib.contextmanager
    def unloaded_admission_modules(self):
        names = (
            "gemini_calibration",
            "gemini_calibration.admission",
            "gemini_calibration.run_live",
        )
        missing = object()
        previous = {name: sys.modules.pop(name, missing) for name in names}
        old_path = list(sys.path)
        try:
            yield
        finally:
            sys.path[:] = old_path
            for name in names:
                sys.modules.pop(name, None)
            for name, module in previous.items():
                if module is not missing:
                    sys.modules[name] = module

    @contextlib.contextmanager
    def tracked_sockets(self):
        real_socket = socket.socket
        opened = []

        def open_socket(*args, **kwargs):
            wrapped = mock.Mock(wraps=real_socket(*args, **kwargs))
            opened.append(wrapped)
            return wrapped

        with mock.patch.object(transport.socket, "socket", side_effect=open_socket):
            yield opened

    def test_capacity_is_one_strict_get_and_mode_0600(self):
        output = self.root / transport.CAPACITY_OUTPUT
        code, stdout, _ = self.run_main(
            [
                "capacity",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
            ],
            secrets={transport.PANEL_KEY_ENV: PANEL_KEY},
        )
        self.assertEqual(code, 0)
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        self.assertTrue(json.loads(output.read_text())["calibration_authority_available"])
        self.assertEqual([(m, p) for m, p, _, _ in self.server.requests], [("GET", "/gemini-subs")])
        self.assertNotIn(PROFILE, stdout)

    def test_event_stream_content_type_requires_exact_mime_token_and_valid_parameters(self):
        for value in (
            "text/event-stream",
            "Text/Event-Stream; charset=utf-8",
            'text/event-stream ; charset = "utf-8"',
        ):
            with self.subTest(value=value):
                self.assertTrue(transport._is_event_stream_content_type(value))
        for value in (
            "text/event-stream-evil",
            "text/event-streamfoo",
            "text/event-stream, application/json",
            "text/event-stream;",
            "text/event-stream charset=utf-8",
            "text/event-stream; charset",
            "text/event-stream; charset=",
            "application/json",
            "",
        ):
            with self.subTest(value=value):
                self.assertFalse(transport._is_event_stream_content_type(value))

    def test_response_framing_rejects_malformed_ambiguous_or_missing_headers(self):
        body = b'{"ready":true}'
        cases = {
            "nondigit": [("Content-Length", "wat"), ("Connection", "close")],
            "negative": [("Content-Length", "-1"), ("Connection", "close")],
            "conflicting": [
                ("Content-Length", str(len(body))),
                ("Content-Length", str(len(body) + 1)),
                ("Connection", "close"),
            ],
            "unsupported-transfer": [
                ("Transfer-Encoding", "identity"),
                ("Connection", "close"),
            ],
            "content-encoding": [
                ("Content-Length", str(len(body))),
                ("Content-Encoding", "gzip"),
                ("Connection", "close"),
            ],
            "both": [
                ("Content-Length", str(len(body))),
                ("Transfer-Encoding", "chunked"),
                ("Connection", "close"),
            ],
            "missing": [("Connection", "close")],
        }
        for name, headers in cases.items():
            with self.subTest(name=name):
                path = f"/invalid-framing-{name}"
                self.server.raw_responses[path] = (200, headers, body)
                with self.assertRaisesRegex(
                    transport.TransportError,
                    "single loopback request failed",
                ):
                    transport._OneConnection(self.server.server_port, 2).request(
                        "GET",
                        path,
                        {"accept": "application/json"},
                        None,
                        transport.MAX_COUNT_BYTES,
                    )

    def test_response_framing_accepts_one_complete_chunked_body(self):
        body = b'{"ready":true}'
        wire_body = (
            f"{len(body):X}\r\n".encode("ascii")
            + body
            + b"\r\n0\r\n\r\n"
        )
        path = "/valid-chunked"
        self.server.raw_responses[path] = (
            200,
            [("Transfer-Encoding", "chunked"), ("Connection", "close")],
            wire_body,
        )

        response = transport._OneConnection(self.server.server_port, 2).request(
            "GET",
            path,
            {"accept": "application/json"},
            None,
            transport.MAX_COUNT_BYTES,
        )

        self.assertEqual(response.status, 200)
        self.assertEqual(response.execution_state, "completed")
        self.assertEqual(response.body, body)
        self.assertIsNone(response.calibration_dispatch_ms)

    def test_ordinary_response_rejects_private_dispatch_attestation(self):
        body = b'{"ready":true}'
        path = "/ordinary-with-private-attestation"
        self.server.raw_responses[path] = (
            200,
            [
                ("Content-Type", "application/json"),
                ("Content-Length", str(len(body))),
                ("Connection", "close"),
                (transport.CALIBRATION_DISPATCH_HEADER, str(DISPATCH_MS)),
            ],
            body,
        )
        with self.assertRaisesRegex(
            transport.TransportError,
            "single loopback request failed",
        ):
            transport._OneConnection(self.server.server_port, 2).request(
                "GET",
                path,
                {"accept": "application/json"},
                None,
                transport.MAX_COUNT_BYTES,
            )

    def test_deadline_bound_success_rejects_invalid_dispatch_attestation(self):
        body = b'{"totalTokens":2}'
        cases = {
            "missing": [],
            "duplicate": [
                (transport.CALIBRATION_DISPATCH_HEADER, str(DISPATCH_MS)),
                (transport.CALIBRATION_DISPATCH_HEADER, str(DISPATCH_MS)),
            ],
            "noncanonical": [(transport.CALIBRATION_DISPATCH_HEADER, "01")],
            "equal-cutoff": [
                (transport.CALIBRATION_DISPATCH_HEADER, str(NOT_AFTER * 1000))
            ],
            "after-cutoff": [
                (transport.CALIBRATION_DISPATCH_HEADER, str(NOT_AFTER * 1000 + 1))
            ],
        }
        for name, dispatch_headers in cases.items():
            with self.subTest(name=name):
                path = f"/invalid-dispatch-{name}:countTokens"
                headers = [
                    ("Content-Type", "application/json"),
                    ("Content-Length", str(len(body))),
                    ("Connection", "close"),
                    *dispatch_headers,
                ]
                self.server.raw_responses[path] = (200, headers, body)
                with self.assertRaisesRegex(
                    transport.TransportError,
                    "single loopback request failed",
                ):
                    transport._OneConnection(
                        self.server.server_port,
                        2,
                        not_after=NOT_AFTER,
                    ).request(
                        "POST",
                        path,
                        {"content-type": "application/json"},
                        b"{}",
                        transport.MAX_COUNT_BYTES,
                    )

    def test_count_claim_precedes_exactly_one_post_and_records_bound_v3(self):
        fake = FakeAdmission(self.evidence)
        output = self.root / transport.COUNT_OUTPUT
        original_request = transport._OneConnection.request

        def ordered(instance, method, path, headers, body, maximum):
            if method == "POST":
                self.assertEqual(fake.actions, ["claim_count"])
            return original_request(instance, method, path, headers, body, maximum)

        with mock.patch.object(transport._OneConnection, "request", autospec=True, side_effect=ordered):
            code, stdout, _ = self.run_main(
                [
                    "count",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                ],
                fake,
                {transport.ADMIN_KEY_ENV: ADMIN_KEY},
            )
        self.assertEqual(code, 0)
        posts = [request for request in self.server.requests if request[0] == "POST"]
        self.assertEqual(len(posts), 1)
        _, path, headers, _ = posts[0]
        self.assertEqual(path, fake.count_request["path"])
        self.assertEqual(headers["x-apitoken-calibration-profile"], PROFILE)
        self.assertEqual(headers["x-apitoken-calibration-request-id"], COUNT_ID)
        self.assertEqual(headers["x-apitoken-calibration-not-after"], str(NOT_AFTER))
        self.assertEqual(fake.actions, ["claim_count", "record_count"])
        self.assertEqual(fake.count_observation["schema"], fake.COUNT_OBSERVATION_SCHEMA)
        self.assertEqual(fake.count_observation["response"], {"totalTokens": 2})
        self.assertEqual(fake.count_observation["dispatch_ms"], DISPATCH_MS)
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        self.assertNotIn(PROFILE, stdout)

    def test_count_redirect_is_not_followed_or_replayed(self):
        fake = FakeAdmission(self.evidence)
        path = fake.count_request["path"]
        self.server.responses[path] = (
            307,
            b"",
            "application/json",
            {"location": "/redirected"},
        )
        output = self.root / transport.COUNT_OUTPUT
        code, _, _ = self.run_main(
            [
                "count",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {transport.ADMIN_KEY_ENV: ADMIN_KEY},
        )
        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertFalse(any(request[1] == "/redirected" for request in self.server.requests))
        self.assertEqual(fake.state, "withdrawn_count_tokens")
        self.assertEqual(fake.count_observation["http_status"], 307)

    def test_count_connection_failure_is_one_attempt_and_terminal(self):
        fake = FakeAdmission(self.evidence)
        output = self.root / transport.COUNT_OUTPUT
        attempts = 0
        original = transport._NumericLoopbackHTTPConnection.request

        def fail_count(connection, method, path, *args, **kwargs):
            nonlocal attempts
            if method == "POST":
                attempts += 1
                raise socket.error("count connection failed")
            return original(connection, method, path, *args, **kwargs)

        with mock.patch.object(transport._NumericLoopbackHTTPConnection, "request", new=fail_count):
            code, _, _ = self.run_main(
                [
                    "count",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                ],
                fake,
                {transport.ADMIN_KEY_ENV: ADMIN_KEY},
            )
        self.assertEqual(code, 1)
        self.assertEqual(attempts, 1)
        self.assertEqual(fake.actions, ["claim_count", "record_count"])
        self.assertEqual(fake.count_observation["http_status"], 0)
        self.assertEqual(fake.count_observation["execution_state"], "unknown")

    def test_count_cutoff_crossing_before_connect_never_opens_post_and_terminalizes(self):
        fake = FakeAdmission(self.evidence)
        output = self.root / transport.COUNT_OUTPUT
        cutoff_ns = fake.journal["not_after"] * 1_000_000_000

        with (
            self.tracked_sockets() as opened,
            mock.patch.object(
                transport.time,
                "time_ns",
                side_effect=[cutoff_ns - 1, cutoff_ns],
            ),
        ):
            code, _, _ = self.run_main(
                [
                    "count",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                ],
                fake,
                {transport.ADMIN_KEY_ENV: ADMIN_KEY},
            )

        self.assertEqual(code, 1)
        self.assertEqual(
            [(method, path) for method, path, _, _ in self.server.requests],
            [("GET", "/ready")],
        )
        connect_calls = [
            call
            for opened_socket in opened
            for call in opened_socket.connect.call_args_list
        ]
        self.assertEqual(
            connect_calls,
            [mock.call((transport.LOOPBACK_HOST, self.server.server_port))],
        )
        self.assertEqual(fake.actions, ["claim_count", "record_count"])
        self.assertEqual(fake.state, "withdrawn_count_tokens")
        self.assertEqual(fake.count_observation["http_status"], 0)
        self.assertEqual(fake.count_observation["execution_state"], "unknown")
        self.assertEqual(json.loads(output.read_text()), fake.count_observation)

    def test_generation_claim_precedes_one_paid_post_and_terminalizes(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        output = self.root / transport.OUTCOME_OUTPUT
        original_request = transport._OneConnection.request

        def ordered(instance, method, path, headers, body, maximum):
            if method == "POST":
                self.assertEqual(fake.actions, ["claim_generation"])
            return original_request(instance, method, path, headers, body, maximum)

        with mock.patch.object(transport._OneConnection, "request", autospec=True, side_effect=ordered):
            code, stdout, _ = self.run_main(
                [
                    "generate",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                    "--evidence-timeout-seconds",
                    "2",
                    "--poll-interval-seconds",
                    "0",
                ],
                fake,
                {
                    transport.ADMIN_KEY_ENV: ADMIN_KEY,
                    transport.PANEL_KEY_ENV: PANEL_KEY,
                },
            )
        self.assertEqual(code, 0)
        paid = [request for request in self.server.requests if request[1] == fake.generation_request["path"]]
        self.assertEqual(len(paid), 1)
        _, _, headers, _ = paid[0]
        self.assertEqual(headers["x-apitoken-calibration-profile"], PROFILE)
        self.assertEqual(headers["x-apitoken-calibration-request-id"], PAID_ID)
        self.assertEqual(headers["x-apitoken-calibration-not-after"], str(NOT_AFTER))
        self.assertEqual(fake.actions, ["claim_generation", "record_outcome"])
        self.assertEqual(len(fake.outcome_observation["response"]), 2)
        self.assertEqual(fake.outcome_observation["dispatch_ms"], DISPATCH_MS)
        self.assertEqual(fake.outcome_observation["event_request_id"], PAID_ID)
        self.assertEqual(fake.outcome_observation["event_plan"], PLAN)
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        self.assertNotIn(PROFILE, stdout)

    def test_generation_accepts_exact_mime_with_well_formed_parameter(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        self.server.responses[path] = (
            200,
            sse_body(),
            'Text/Event-Stream; charset="utf-8"',
            None,
        )
        output = self.root / transport.OUTCOME_OUTPUT
        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
                "--evidence-timeout-seconds",
                "2",
                "--poll-interval-seconds",
                "0",
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )
        self.assertEqual(code, 0)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertEqual(fake.state, "success")

    def test_generation_rejects_mime_prefix_spoof_after_exactly_one_paid_post(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        self.server.responses[path] = (
            200,
            sse_body(),
            "text/event-stream-evil",
            None,
        )
        output = self.root / transport.OUTCOME_OUTPUT
        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )
        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertIsNone(fake.outcome_observation["response"])

    def test_generation_rejects_execution_header_on_200_without_capacity_poll(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        self.server.responses[path] = (
            200,
            sse_body(),
            "text/event-stream",
            {"x-apitoken-execution-state": "not_started"},
        )
        output = self.root / transport.OUTCOME_OUTPUT
        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )

        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertEqual(fake.outcome_observation["execution_state"], "unknown")
        self.assertIsNone(fake.outcome_observation["response"])
        self.assertIsNone(fake.outcome_observation["immutable_capacity"])
        self.assertEqual(self.server.capacity_calls, 0)

    def test_generation_rejects_raw_json_array_as_stream_without_replay(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        self.server.responses[path] = (
            200,
            json.dumps([{"modelVersion": MODEL}]).encode(),
            "text/event-stream; charset=utf-8",
            None,
        )
        output = self.root / transport.OUTCOME_OUTPUT
        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )
        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertIsNone(fake.outcome_observation["response"])

    def test_generation_redirect_is_not_followed_or_replayed(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        self.server.responses[path] = (
            308,
            b"",
            "application/json",
            {"location": "/paid-again"},
        )
        output = self.root / transport.OUTCOME_OUTPUT
        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )
        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertFalse(any(request[1] == "/paid-again" for request in self.server.requests))
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertEqual(fake.outcome_observation["http_status"], 308)

    def test_generation_connection_failure_is_one_attempt_and_terminal(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        output = self.root / transport.OUTCOME_OUTPUT
        attempts = 0
        original = transport._NumericLoopbackHTTPConnection.request

        def fail_paid(connection, method, path, *args, **kwargs):
            nonlocal attempts
            if method == "POST":
                attempts += 1
                raise socket.error("secret-shaped transport failure")
            return original(connection, method, path, *args, **kwargs)

        with mock.patch.object(transport._NumericLoopbackHTTPConnection, "request", new=fail_paid):
            code, _, _ = self.run_main(
                [
                    "generate",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                ],
                fake,
                {
                    transport.ADMIN_KEY_ENV: ADMIN_KEY,
                    transport.PANEL_KEY_ENV: PANEL_KEY,
                },
            )
        self.assertEqual(code, 1)
        self.assertEqual(attempts, 1)
        self.assertEqual(fake.actions, ["claim_generation", "record_outcome"])
        self.assertEqual(fake.outcome_observation["http_status"], 0)
        self.assertEqual(fake.outcome_observation["execution_state"], "unknown")

    def test_generation_truncated_content_length_is_terminal_without_replay(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        path = fake.generation_request["path"]
        body = sse_body()
        self.server.truncated_responses[path] = (
            200,
            body,
            "text/event-stream",
            len(body) + 100,
        )
        output = self.root / transport.OUTCOME_OUTPUT

        code, _, _ = self.run_main(
            [
                "generate",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(output),
                "--library-root",
                str(self.library),
            ],
            fake,
            {
                transport.ADMIN_KEY_ENV: ADMIN_KEY,
                transport.PANEL_KEY_ENV: PANEL_KEY,
            },
        )

        self.assertEqual(code, 1)
        self.assertEqual([request[1] for request in self.server.requests].count(path), 1)
        self.assertEqual(fake.actions, ["claim_generation", "record_outcome"])
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertEqual(fake.outcome_observation["http_status"], 0)
        self.assertEqual(fake.outcome_observation["execution_state"], "unknown")
        self.assertIsNone(fake.outcome_observation["response"])
        self.assertIsNone(fake.outcome_observation["immutable_capacity"])
        self.assertEqual(self.server.capacity_calls, 0)

    def test_generation_cutoff_crossing_before_connect_never_opens_post_and_terminalizes(self):
        fake = FakeAdmission(self.evidence)
        fake.state = "generation_armed"
        output = self.root / transport.OUTCOME_OUTPUT
        cutoff_ns = fake.journal["not_after"] * 1_000_000_000

        with (
            self.tracked_sockets() as opened,
            mock.patch.object(
                transport.time,
                "time_ns",
                side_effect=[cutoff_ns - 1, cutoff_ns],
            ),
        ):
            code, _, _ = self.run_main(
                [
                    "generate",
                    "--evidence-dir",
                    str(self.evidence),
                    "--output",
                    str(output),
                    "--library-root",
                    str(self.library),
                ],
                fake,
                {
                    transport.ADMIN_KEY_ENV: ADMIN_KEY,
                    transport.PANEL_KEY_ENV: PANEL_KEY,
                },
            )

        self.assertEqual(code, 1)
        self.assertEqual(
            [(method, path) for method, path, _, _ in self.server.requests],
            [("GET", "/ready")],
        )
        connect_calls = [
            call
            for opened_socket in opened
            for call in opened_socket.connect.call_args_list
        ]
        self.assertEqual(
            connect_calls,
            [mock.call((transport.LOOPBACK_HOST, self.server.server_port))],
        )
        self.assertEqual(fake.actions, ["claim_generation", "record_outcome"])
        self.assertEqual(fake.state, "withdrawn_evidence")
        self.assertEqual(fake.outcome_observation["http_status"], 0)
        self.assertEqual(fake.outcome_observation["execution_state"], "unknown")
        self.assertIsNone(fake.outcome_observation["response"])
        self.assertIsNone(fake.outcome_observation["immutable_capacity"])
        self.assertEqual(json.loads(output.read_text()), fake.outcome_observation)

    def test_load_admission_authenticates_exact_package_chain(self):
        library, package = self.exact_library("exact-library")

        with self.unloaded_admission_modules():
            admission = transport._load_admission(library)

            self.assertEqual(
                Path(sys.modules["gemini_calibration"].__file__).resolve(),
                (package / "__init__.py").resolve(),
            )
            self.assertEqual(
                Path(admission.__file__).resolve(),
                (package / "admission.py").resolve(),
            )
            self.assertEqual(
                Path(admission.run_live.__file__).resolve(),
                (package / "run_live.py").resolve(),
            )

    def test_load_admission_rejects_substituted_package_member(self):
        for filename in ("__init__.py", "admission.py", "run_live.py"):
            with self.subTest(filename=filename):
                library, package = self.exact_library(f"substituted-{filename}")
                target = package / filename
                target.write_bytes(target.read_bytes() + b"\n# substituted\n")

                with self.assertRaisesRegex(
                    transport.TransportError,
                    "differs from the pinned producer",
                ):
                    transport._load_admission(library)

    def test_load_admission_rejects_hardlinked_run_live(self):
        library, package = self.exact_library("hardlinked-run-live")
        os.link(package / "run_live.py", library / "run-live-alias.py")

        with self.assertRaisesRegex(
            transport.TransportError,
            "admission parser library is not an immutable regular file",
        ):
            transport._load_admission(library)

    def test_load_admission_rejects_unchecked_bytecode_cache(self):
        library, package = self.exact_library("bytecode-cache")
        cache = package / "__pycache__"
        cache.mkdir()
        (cache / "admission.cpython-313.pyc").write_bytes(b"unchecked bytecode")

        with self.assertRaisesRegex(
            transport.TransportError,
            "package has an unauthenticated entry",
        ):
            transport._load_admission(library)

    def test_load_admission_rejects_writable_run_live(self):
        library, package = self.exact_library("writable-run-live")
        os.chmod(package / "run_live.py", 0o666)

        # The temporary parent is intentionally user-owned. Bypass only the two production
        # root-directory checks, then exercise real non-testing mode validation for all three files.
        with (
            mock.patch.object(
                transport,
                "_testing",
                side_effect=[True, True, False, False, False],
            ),
            self.assertRaisesRegex(
                transport.TransportError,
                "admission parser library is not an immutable regular file",
            ),
        ):
            transport._load_admission(library)

    def test_output_must_be_fixed_private_sibling(self):
        fake = FakeAdmission(self.evidence)
        outside = self.root / "other.json"
        code, _, _ = self.run_main(
            [
                "count",
                "--evidence-dir",
                str(self.evidence),
                "--output",
                str(outside),
                "--library-root",
                str(self.library),
            ],
            fake,
            {transport.ADMIN_KEY_ENV: ADMIN_KEY},
        )
        self.assertEqual(code, 1)
        self.assertFalse(outside.exists())
        self.assertEqual(self.server.requests, [])


if __name__ == "__main__":
    unittest.main()
