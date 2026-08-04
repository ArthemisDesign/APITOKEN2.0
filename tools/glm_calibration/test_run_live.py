import contextlib
import copy
import io
import json
import os
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch

from tools.glm_calibration import run_live as rl


SECRET = "glm-test-secret-key-0123456789abcdef"
BASE_URL = "https://api.z.ai"
PEAK_MONDAY_15H = 4 * 86_400 + 15 * 3_600 - 8 * 3_600
OFF_PEAK_MONDAY_12H = 4 * 86_400 + 12 * 3_600 - 8 * 3_600
SATURDAY_15H = 2 * 86_400 + 15 * 3_600 - 8 * 3_600


def quota_payload(usage_5h=100, usage_week=100, number_5h=2_000, number_week=10_000):
    return {
        "code": 200,
        "msg": "ok",
        "success": True,
        "data": {
            "limits": [
                {
                    "type": "TIME_LIMIT",
                    "unit": 3,
                    "number": number_5h,
                    "usage": usage_5h,
                    "currentValue": usage_5h,
                    "remaining": number_5h - usage_5h,
                    "percentage": 5.0,
                    "nextResetTime": 1_800_000_000_000,
                    "usageDetails": [],
                },
                {
                    "type": "TOKENS_LIMIT",
                    "unit": 6,
                    "number": number_week,
                    "usage": usage_week,
                    "currentValue": usage_week,
                    "remaining": number_week - usage_week,
                    "percentage": 1.0,
                    "nextResetTime": 1_800_100_000_000,
                    "usageDetails": [],
                },
            ]
        },
    }


def moved_quota(base, delta):
    moved = copy.deepcopy(base)
    for entry in moved["data"]["limits"]:
        entry["usage"] += delta
        entry["currentValue"] += delta
        entry["remaining"] -= delta
    return moved


def gen_payload(model="glm-4.7", input_tokens=25, output_tokens=3, usage_extra=None):
    usage = {"input_tokens": input_tokens, "output_tokens": output_tokens}
    usage.update(usage_extra or {})
    return {
        "id": "msg-test",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": "CALIBRATION_OK"}],
        "stop_reason": "end_turn",
        "usage": usage,
    }


def sse_lines(model="glm-4.7", input_tokens=25, output_tokens=3, delta_texts=("CALIBRATION", "_OK")):
    events = [
        {
            "type": "message_start",
            "message": {
                "id": "msg-stream",
                "type": "message",
                "model": model,
                "usage": {"input_tokens": input_tokens, "output_tokens": 1},
            },
        }
    ]
    for text in delta_texts:
        events.append(
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text},
            }
        )
    events.append(
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": output_tokens},
        }
    )
    events.append({"type": "message_stop"})
    return [f"data: {json.dumps(event)}\n".encode() for event in events]


class FakeResponse:
    def __init__(self, body=b"", lines=None):
        self._body = body
        self._lines = list(lines) if lines is not None else None

    def read(self, *args):
        return self._body

    def readline(self):
        if self._lines:
            return self._lines.pop(0)
        return b""

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def http_error(url, code, payload):
    body = json.dumps(payload).encode() if isinstance(payload, dict) else payload
    return urllib.error.HTTPError(url, code, "error", {}, io.BytesIO(body))


class FakeProvider:
    """Routes urlopen calls by URL/method; quota responses are popped from a queue."""

    def __init__(self):
        self.quota_queue = []
        self.generate_handler = None
        self.requests = []

    def queue_quota(self, payload, count=1):
        for _ in range(count):
            self.quota_queue.append(copy.deepcopy(payload))

    def __call__(self, request, timeout=None):
        self.requests.append(request)
        path = request.full_url.split("?", 1)[0]
        if request.get_method() == "GET" and path.endswith(rl.QUOTA_PATH):
            item = self.quota_queue.pop(0)
            if isinstance(item, Exception):
                raise item
            return FakeResponse(json.dumps(item).encode())
        if request.get_method() == "POST" and path.endswith(rl.MESSAGES_PATH):
            return self.generate_handler(request)
        raise AssertionError(f"unexpected request: {request.full_url}")

    @property
    def posts(self):
        return [request for request in self.requests if request.get_method() == "POST"]

    @property
    def quota_gets(self):
        return [request for request in self.requests if request.get_method() == "GET"]


def default_generate(request):
    body = json.loads(request.data.decode())
    if body.get("stream"):
        return FakeResponse(lines=sse_lines(model=body["model"]))
    return FakeResponse(json.dumps(gen_payload(model=body["model"])).encode())


def happy_provider(quota_reads=64):
    provider = FakeProvider()
    provider.queue_quota(quota_payload(), count=quota_reads)
    provider.generate_handler = default_generate
    return provider


class MainCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.report = str(Path(self.tmp.name) / "report.json")
        self.checkpoint = str(Path(self.tmp.name) / "checkpoint.json")

    def base_argv(self):
        return [
            "--profile", "sub-1",
            "--base-url", BASE_URL,
            "--report", self.report,
            "--checkpoint", self.checkpoint,
        ]

    def invoke(self, argv, provider=None, key=SECRET):
        env = {rl.KEY_ENV: key} if key else {rl.KEY_ENV: ""}
        stdout = io.StringIO()
        stderr = io.StringIO()

        def forbidden(request, timeout=None):
            raise AssertionError(f"unexpected network call: {request.full_url}")

        # time.time is pinned to a peak SGT moment so expected-credit rounding is
        # deterministic; sleeps are patched out.
        with patch.object(rl.time, "sleep"), patch.object(
            rl.time, "time", return_value=PEAK_MONDAY_15H
        ), patch.dict(os.environ, env), patch(
            "urllib.request.urlopen", provider if provider is not None else forbidden
        ), contextlib.redirect_stdout(
            stdout
        ), contextlib.redirect_stderr(
            stderr
        ):
            code = rl.main(argv)
        return code, stdout.getvalue(), stderr.getvalue()

    def invoke_error(self, argv, provider=None, key=SECRET):
        with self.assertRaises(rl.CalibrationError) as raised:
            self.invoke(argv, provider=provider, key=key)
        return str(raised.exception)


class ParserAndGuardTests(unittest.TestCase):
    def test_usd_parser_is_integer_only_and_exact(self):
        self.assertEqual(rl.usd_to_nano("0.05"), 50_000_000)
        self.assertEqual(rl.usd_to_nano("5"), 5_000_000_000)
        self.assertEqual(rl.usd_to_nano("0.000000001"), 1)
        for bad in ("1e2", "0.1.2", "", "abc", "-1", "1e-2"):
            with self.assertRaises(rl.CalibrationError):
                rl.usd_to_nano(bad)

    def test_budget_guard_blocks_dispatch_and_overcharge(self):
        budget = rl.Budget(100)
        budget.require_room(100)
        with self.assertRaisesRegex(rl.CalibrationError, "before dispatch"):
            budget.require_room(101)
        budget.charge(60)
        with self.assertRaisesRegex(rl.CalibrationError, "exceeded the run budget"):
            budget.charge(41)
        with self.assertRaisesRegex(rl.CalibrationError, "positive"):
            budget.charge(0)

    def test_worst_case_hold_is_accounted_against_the_budget(self):
        budget = rl.Budget(100)
        budget.hold(70)
        with self.assertRaisesRegex(rl.CalibrationError, "before dispatch"):
            budget.require_room(31)
        with self.assertRaisesRegex(rl.CalibrationError, "worst-case hold"):
            budget.hold(31)

    def test_base_url_is_an_exact_two_host_allowlist(self):
        self.assertEqual(rl.normalize_base_url("https://api.z.ai/"), "https://api.z.ai")
        self.assertEqual(
            rl.normalize_base_url("https://open.bigmodel.cn"), "https://open.bigmodel.cn"
        )
        for bad in ("https://example.com", "http://api.z.ai", "https://api.z.ai.evil.com"):
            with self.assertRaises(rl.CalibrationError):
                rl.normalize_base_url(bad)

    def test_profile_label_is_bounded(self):
        self.assertEqual(rl.validate_profile("sub-1_OK.x"), "sub-1_OK.x")
        for bad in ("", "has space", "x" * 65, "-leading-dash"):
            with self.assertRaises(rl.CalibrationError):
                rl.validate_profile(bad)


class MoneyMathTests(unittest.TestCase):
    def test_api_cost_matches_metering_vectors(self):
        rates = rl.RATE_CARD["glm-5.2"]
        usage = {
            "input_tokens": 1_000,
            "cache_read_tokens": 2_000,
            "cache_write_tokens": 3_000,
            "output_tokens": 4_000,
            "reasoning_output_tokens": 0,
        }
        # crates/metering/src/glm.rs: 1000*1400 + 2000*260 + 3000*1400 + 4000*4400.
        self.assertEqual(rl.api_cost_nano(usage, rates), 23_720_000)
        turbo = rl.RATE_CARD["glm-5-turbo"]
        self.assertEqual(rl.api_cost_nano(usage, turbo), 21_280_000)
        g47 = rl.RATE_CARD["glm-4.7"]
        self.assertEqual(rl.api_cost_nano(usage, g47), 11_420_000)

    def test_reasoning_is_a_subset_and_never_an_extra_leg(self):
        rates = rl.RATE_CARD["glm-5.2"]
        base = {
            "input_tokens": 0,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "output_tokens": 100,
            "reasoning_output_tokens": 0,
        }
        with_reasoning = dict(base, reasoning_output_tokens=100)
        self.assertEqual(
            rl.api_cost_nano(base, rates), rl.api_cost_nano(with_reasoning, rates)
        )
        broken = dict(base, reasoning_output_tokens=101)
        with self.assertRaisesRegex(rl.CalibrationError, "subset invariant"):
            rl.api_cost_nano(broken, rates)

    def test_credit_formula_matches_metering_vectors(self):
        rates = rl.RATE_CARD["glm-5.2"]
        all_legs = {
            "input_tokens": 10_000,
            "cache_read_tokens": 10_000,
            "cache_write_tokens": 0,
            "output_tokens": 10_000,
            "reasoning_output_tokens": 0,
        }
        self.assertEqual(rl.credits_micro_expected(all_legs, rates, False), 32_600_000)
        self.assertEqual(rl.whole_credits(32_600_000), 33)
        one_input = {
            "input_tokens": 1,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "output_tokens": 0,
            "reasoning_output_tokens": 0,
        }
        self.assertEqual(rl.credits_micro_expected(one_input, rates, False), 690)
        out = dict(one_input, input_tokens=0, output_tokens=100_000)
        self.assertEqual(rl.credits_micro_expected(out, rates, False), 240_000_000)
        self.assertEqual(rl.credits_micro_expected(out, rates, True), 120_000_000)

    def test_served_id_without_credit_multipliers_fails_closed(self):
        usage = {
            "input_tokens": 1,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "output_tokens": 1,
            "reasoning_output_tokens": 0,
        }
        with self.assertRaisesRegex(rl.CalibrationError, "credit multipliers"):
            rl.credits_micro_expected(usage, rl.RATE_CARD["glm-5"], False)

    def test_peak_window_boundaries_match_the_metering_crate(self):
        # sgt(days, h, m, s) from the metering tests: day 4 after the epoch is a Monday.
        def sgt(days, h, m=0, s=0):
            return days * 86_400 + h * 3_600 + m * 60 + s - 8 * 3_600

        self.assertTrue(rl.is_peak_sgt(PEAK_MONDAY_15H))
        self.assertFalse(rl.is_peak_sgt(OFF_PEAK_MONDAY_12H))
        self.assertFalse(rl.is_peak_sgt(SATURDAY_15H))
        self.assertTrue(rl.is_peak_sgt(sgt(4, 14)))
        self.assertFalse(rl.is_peak_sgt(sgt(4, 13, 59, 59)))
        self.assertTrue(rl.is_peak_sgt(sgt(4, 17, 59, 59)))
        self.assertFalse(rl.is_peak_sgt(sgt(4, 18)))


class QuotaParserTests(unittest.TestCase):
    def test_http_200_with_code_401_means_invalid_key(self):
        payload = {"code": 401, "msg": "invalid api key", "success": False}
        with self.assertRaisesRegex(rl.QuotaKeyInvalid, "code 401"):
            rl.parse_quota_observation(payload)

    def test_valid_wrapper_parses_limits_and_counters(self):
        observation = rl.parse_quota_observation(quota_payload())
        self.assertEqual(len(observation["limits"]), 2)
        counters = rl.quota_counters(observation)
        self.assertEqual(counters[("limits", "TIME_LIMIT", "usage")], 100)
        self.assertEqual(counters[("limits", "TIME_LIMIT", "remaining")], 1_900)
        self.assertEqual(counters[("limits", "TOKENS_LIMIT", "usage")], 100)

    def test_unrecognized_or_legacy_shape_fails_closed(self):
        with self.assertRaises(rl.QuotaShapeError):
            rl.parse_quota_observation({"code": 200, "success": False, "msg": "legacy plan"})
        with self.assertRaises(rl.QuotaShapeError):
            rl.parse_quota_observation({"code": 200, "success": True})
        with self.assertRaises(rl.QuotaShapeError):
            rl.parse_quota_observation({"code": 200, "success": True, "data": {"limits": {}}})
        with self.assertRaises(rl.QuotaShapeError):
            rl.parse_quota_observation(
                {"code": 200, "success": True, "data": {"limits": [{"usage": 1}]}}
            )

    def test_per_model_usage_details_become_namespaced_counters(self):
        payload = quota_payload()
        payload["data"]["limits"][0]["usageDetails"] = [
            {"modelCode": "glm-5.2", "usage": 7}
        ]
        counters = rl.quota_counters(rl.parse_quota_observation(payload))
        self.assertEqual(counters[("details", "TIME_LIMIT", "glm-5.2", "usage")], 7)


class AttributionTests(unittest.TestCase):
    def test_exact_credit_movement_is_attributed(self):
        before = rl.parse_quota_observation(quota_payload())
        after = rl.parse_quota_observation(moved_quota(quota_payload(), 3))
        status, deltas, _ = rl.attribute_quota_delta(before, after, 3, "glm-5.2")
        self.assertEqual(status, "attributed")
        self.assertEqual(deltas["limits|TIME_LIMIT|usage"], 3)
        self.assertEqual(deltas["limits|TOKENS_LIMIT|remaining"], 3)

    def test_sub_credit_leg_with_no_movement_is_below_resolution(self):
        before = rl.parse_quota_observation(quota_payload())
        after = rl.parse_quota_observation(quota_payload())
        status, _, _ = rl.attribute_quota_delta(before, after, 0, "glm-4.7")
        self.assertEqual(status, "below-resolution")

    def test_foreign_traffic_movement_fails_closed_unattributed(self):
        before = rl.parse_quota_observation(quota_payload())
        after = rl.parse_quota_observation(moved_quota(quota_payload(), 5))
        status, deltas, reason = rl.attribute_quota_delta(before, after, 3, "glm-5.2")
        self.assertEqual(status, "unattributed")
        self.assertIn("foreign traffic", reason)
        self.assertEqual(deltas["limits|TIME_LIMIT|usage"], 5)

    def test_expected_movement_that_never_arrives_is_unattributed(self):
        before = rl.parse_quota_observation(quota_payload())
        after = rl.parse_quota_observation(quota_payload())
        status, _, reason = rl.attribute_quota_delta(before, after, 2, "glm-5.2")
        self.assertEqual(status, "unattributed")
        self.assertIn("did not move", reason)

    def test_movement_on_another_model_is_unattributed(self):
        payload = quota_payload()
        payload["data"]["limits"][0]["usageDetails"] = [
            {"modelCode": "glm-4.7", "usage": 10}
        ]
        before = rl.parse_quota_observation(payload)
        moved = moved_quota(payload, 2)
        moved["data"]["limits"][0]["usageDetails"] = [
            {"modelCode": "glm-4.7", "usage": 12}
        ]
        after = rl.parse_quota_observation(moved)
        status, _, reason = rl.attribute_quota_delta(before, after, 2, "glm-5.2")
        self.assertEqual(status, "unattributed")
        self.assertIn("glm-4.7", reason)


class UsageAndStreamTests(unittest.TestCase):
    def test_usage_parser_preserves_cache_and_reasoning_fields(self):
        parsed = rl.usage_from_value(
            {
                "input_tokens": 11,
                "cache_read_input_tokens": 12,
                "cache_creation": {"ephemeral_5m_input_tokens": 13, "other": 4},
                "output_tokens": 14,
                "reasoning_tokens": 9,
            }
        )
        self.assertEqual(
            parsed,
            {
                "input_tokens": 11,
                "cache_read_tokens": 12,
                "cache_write_tokens": 17,
                "output_tokens": 14,
                "reasoning_output_tokens": 9,
            },
        )

    def test_stream_usage_replaces_cumulative_output_instead_of_summing(self):
        events = rl.parse_sse_events([(0.0, line) for line in sse_lines(output_tokens=3)])
        extra = rl.parse_sse_events(
            [
                (
                    1.0,
                    b'data: {"type":"message_delta","usage":{"output_tokens":5}}\n',
                )
            ]
        )
        usage = rl.merge_stream_usage(events + extra)
        self.assertEqual(usage["input_tokens"], 25)
        self.assertEqual(usage["output_tokens"], 5)

    def test_stream_evidence_flags_real_incrementality(self):
        events = rl.parse_sse_events([(0.0, line) for line in sse_lines()])
        evidence = rl.stream_evidence(events, 0.0, 0.25)
        self.assertTrue(evidence["incremental_evidence"])
        self.assertEqual(evidence["text_delta_frames"], 2)
        single = rl.parse_sse_events(
            [(0.0, line) for line in sse_lines(delta_texts=("CALIBRATION_OK",))]
        )
        self.assertFalse(rl.stream_evidence(single, 0.0, 0.1)["incremental_evidence"])

    def test_malformed_stream_frame_fails_closed(self):
        with self.assertRaisesRegex(rl.CalibrationError, "malformed data frame"):
            rl.parse_sse_events([(0.0, b"data: {not json\n")])

    def test_leg_usage_requires_input_and_output_classes(self):
        with self.assertRaisesRegex(rl.CalibrationError, "output token class"):
            rl.validate_leg_usage(
                {
                    "input_tokens": 5,
                    "cache_read_tokens": 0,
                    "cache_write_tokens": 0,
                    "output_tokens": 0,
                    "reasoning_output_tokens": 0,
                }
            )


class DryRunTests(MainCase):
    def test_dry_run_without_key_prints_an_honest_plan_and_sends_nothing(self):
        # invoke() without a provider stubs urlopen with a forbidden() that raises on any
        # network call, so a passed test also proves dry-run sent nothing.
        code, stdout, stderr = self.invoke(self.base_argv(), key=None)
        self.assertEqual(code, 0)
        plan = json.loads(stdout)
        self.assertEqual(plan["schema"], "glm-live-calibration-plan/v1")
        self.assertFalse(plan["key_present"])
        self.assertFalse(plan["live_possible"])
        self.assertIn(rl.KEY_ENV, " ".join(plan["notes"]))
        self.assertIn("live legs are impossible", stderr)
        self.assertEqual(len(plan["legs"]), 6)
        self.assertNotIn(SECRET, stdout)

    def test_dry_run_with_a_key_fetches_only_the_free_quota_anchor(self):
        provider = happy_provider()
        code, stdout, _ = self.invoke(self.base_argv(), provider=provider)
        self.assertEqual(code, 0)
        plan = json.loads(stdout)
        self.assertTrue(plan["key_present"])
        self.assertIsNotNone(plan["quota_anchor"])
        self.assertEqual(provider.posts, [], "dry-run must never send a paid request")
        self.assertEqual(len(provider.quota_gets), 1)
        quota_request = provider.quota_gets[0]
        self.assertEqual(quota_request.headers.get("Authorization"), SECRET)
        self.assertNotIn("Bearer", quota_request.headers.get("Authorization"))

    def test_generation_headers_mirror_the_claude_code_fingerprint(self):
        provider = happy_provider()
        seen = {}

        def handler(request):
            seen.update(request.headers)
            return default_generate(request)

        provider.generate_handler = handler
        self.invoke(self.base_argv() + ["--execute", "--models", "glm-4.7"], provider=provider)
        self.assertEqual(seen["Authorization"], f"Bearer {SECRET}")
        self.assertEqual(seen["User-agent"], "claude-cli/2.1.195 (external, sdk-cli)")
        self.assertEqual(seen["Anthropic-version"], "2023-06-01")
        self.assertEqual(seen["Anthropic-beta"], "claude-code-20250219")

    def test_budget_cap_is_005_by_default(self):
        self.invoke_error(self.base_argv() + ["--budget-usd", "0.06"], key=None)
        code, _, _ = self.invoke(self.base_argv() + ["--budget-usd", "0.05"], key=None)
        self.assertEqual(code, 0)

    def test_acknowledged_cap_allows_more_but_still_has_a_hard_ceiling(self):
        argv = self.base_argv() + ["--budget-usd", "5", "--i-understand"]
        code, _, _ = self.invoke(argv, key=None)
        self.assertEqual(code, 0)
        error = self.invoke_error(
            self.base_argv() + ["--budget-usd", "5.000000001", "--i-understand"], key=None
        )
        self.assertIn("$5", error)
        error = self.invoke_error(self.base_argv() + ["--budget-usd", "0.06"], key=None)
        self.assertIn("--i-understand", error)

    def test_execute_requires_the_key(self):
        error = self.invoke_error(self.base_argv() + ["--execute"], key=None)
        self.assertIn(rl.KEY_ENV, error)


class ExecuteFlowTests(MainCase):
    def test_happy_path_run_is_complete_and_reports_everything(self):
        provider = happy_provider()
        code, stdout, _ = self.invoke(self.base_argv() + ["--execute"], provider=provider)
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertTrue(report["complete"])
        self.assertIsNone(report["failure"])
        self.assertEqual(report["schema"], "glm-live-calibration/v1")
        self.assertEqual(report["target"], {"profile": "sub-1", "base_url": BASE_URL})
        self.assertEqual(len(report["legs"]), 6)
        self.assertEqual(len(provider.posts), 6)
        for model in rl.MODEL_ORDER:
            self.assertEqual(report["coverage"][model]["non_stream"], "ok")
            self.assertEqual(report["coverage"][model]["stream"], "ok")
        for key in ("usage_form", "sse_incrementality", "quota_units", "quota_wall_codes"):
            self.assertIn(key, report["unknowns"])
        self.assertEqual(report["unknowns"]["usage_form"]["status"], "resolved")
        self.assertEqual(report["unknowns"]["sse_incrementality"]["status"], "resolved")
        self.assertEqual(report["unknowns"]["quota_units"]["status"], "unresolved")
        self.assertEqual(report["unknowns"]["quota_wall_codes"]["status"], "unresolved")
        self.assertIsNotNone(report["quota_anchor"])
        self.assertGreater(int(report["spent_nanousd"]), 0)
        self.assertEqual(report["unattributed_deltas"], [])
        self.assertTrue(Path(self.checkpoint).exists())

    def test_preflight_bound_covers_the_actual_priced_cost_of_every_leg(self):
        provider = happy_provider()
        self.invoke(self.base_argv() + ["--execute"], provider=provider)
        report = json.loads(Path(self.report).read_text())
        for record in report["legs"]:
            self.assertGreaterEqual(
                int(record["preflight"]["worst_case_nanousd"]), int(record["api_nanousd"])
            )
            self.assertGreaterEqual(
                record["preflight"]["input_token_bound"], record["usage"]["input_tokens"]
            )
        self.assertGreaterEqual(int(report["budget_nanousd"]), int(report["spent_nanousd"]))

    def test_paid_request_is_never_retried_after_transport_ambiguity(self):
        provider = happy_provider()
        calls = []

        def broken(request):
            calls.append(request)
            raise urllib.error.URLError("connection reset")

        provider.generate_handler = broken
        error = self.invoke_error(
            self.base_argv() + ["--execute", "--run-id", "test-run-fixed"], provider=provider
        )
        self.assertIn("held the worst-case bound", error)
        self.assertEqual(len(calls), 1, "the paid request must not be retried")
        report = json.loads(Path(self.report).read_text())
        self.assertFalse(report["complete"])
        self.assertEqual(report["leg_status"]["messages:glm-5.2"], "held-ambiguous")
        held = int(report["held_nanousd"])
        self.assertGreater(held, 0)
        leg = rl.build_legs(list(rl.MODEL_ORDER), 32)[0]
        bound = rl.input_token_bound(rl.body_for_leg(leg, "test-run-fixed"))
        self.assertEqual(held, rl.worst_case_nano(leg, rl.RATE_CARD[leg.model], bound))

    def test_quota_poll_retries_a_read_only_transport_failure(self):
        provider = happy_provider()
        provider.quota_queue = [
            urllib.error.URLError("blip"),
            urllib.error.URLError("blip"),
            quota_payload(),
        ] + [quota_payload() for _ in range(32)]
        code, _, _ = self.invoke(self.base_argv() + ["--execute", "--models", "glm-4.7"],
                                 provider=provider)
        self.assertEqual(code, 0)
        self.assertGreaterEqual(len(provider.quota_gets), 3)

    def test_unattributed_quota_movement_stops_the_matrix_fail_closed(self):
        provider = FakeProvider()
        settled = quota_payload()
        foreign = moved_quota(quota_payload(), 1)
        provider.quota_queue = (
            [copy.deepcopy(settled)]
            + [copy.deepcopy(settled), copy.deepcopy(foreign), copy.deepcopy(foreign)]
            + [copy.deepcopy(settled) for _ in range(32)]
        )
        provider.generate_handler = default_generate
        error = self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        self.assertIn("unattributed", error)
        self.assertEqual(len(provider.posts), 1, "the matrix must stop after ambiguity")
        report = json.loads(Path(self.report).read_text())
        self.assertFalse(report["complete"])
        self.assertEqual(len(report["unattributed_deltas"]), 1)
        self.assertEqual(report["legs"][0]["attribution"], "unattributed")

    def test_exact_credit_sized_movement_is_attributed_and_resolves_quota_units(self):
        provider = FakeProvider()
        base = quota_payload()
        moved = moved_quota(quota_payload(), 3)
        provider.quota_queue = (
            [copy.deepcopy(base)]
            + [copy.deepcopy(base), copy.deepcopy(moved), copy.deepcopy(moved)]
            + [copy.deepcopy(base), copy.deepcopy(moved), copy.deepcopy(moved)]
            + [copy.deepcopy(moved) for _ in range(16)]
        )

        def big(request):
            body = json.loads(request.data.decode())
            if body.get("stream"):
                return FakeResponse(
                    lines=sse_lines(model=body["model"], input_tokens=100, output_tokens=1_024)
                )
            return FakeResponse(
                json.dumps(
                    gen_payload(model=body["model"], input_tokens=100, output_tokens=1_024)
                ).encode()
            )

        provider.generate_handler = big
        code, _, _ = self.invoke(
            self.base_argv() + ["--execute", "--models", "glm-5.2", "--max-tokens", "1024"],
            provider=provider,
        )
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["legs"][0]["attribution"], "attributed")
        self.assertEqual(report["legs"][0]["credits_whole_expected"], 3)
        self.assertEqual(report["unknowns"]["quota_units"]["status"], "resolved")

    def test_quota_wall_business_code_is_recorded_and_stops_paid_traffic(self):
        provider = happy_provider()

        def walled(request):
            raise http_error(
                request.full_url,
                429,
                {"error": {"code": "1308", "message": "quota exhausted, reset at 18:00"}},
            )

        provider.generate_handler = walled
        error = self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        self.assertIn("1308", error)
        self.assertEqual(len(provider.posts), 1)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["quota_wall_evidence"]["business_code"], "1308")
        self.assertEqual(report["unknowns"]["quota_wall_codes"]["status"], "resolved")
        self.assertFalse(report["complete"])

    def test_model_not_in_plan_is_an_unavailable_capability_not_a_run_failure(self):
        provider = happy_provider()

        def partial(request):
            body = json.loads(request.data.decode())
            if body["model"] == "glm-5.2":
                raise http_error(
                    request.full_url,
                    429,
                    {"error": {"code": "1311", "message": "model not in plan"}},
                )
            return default_generate(request)

        provider.generate_handler = partial
        code, _, _ = self.invoke(self.base_argv() + ["--execute"], provider=provider)
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertTrue(report["complete"])
        self.assertEqual(report["coverage"]["glm-5.2"]["non_stream"], "unavailable")
        self.assertEqual(report["coverage"]["glm-5.2"]["stream"], "unavailable")
        self.assertEqual(len(report["unavailable_capabilities"]), 1)
        self.assertEqual(len(provider.posts), 5, "the other two glm-5.2 legs are skipped")

    def test_served_model_outside_the_rate_card_holds_the_bound(self):
        provider = happy_provider()

        def alien(request):
            return FakeResponse(json.dumps(gen_payload(model="glm-9")).encode())

        provider.generate_handler = alien
        error = self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        self.assertIn("outside the reviewed rate card", error)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["leg_status"]["messages:glm-5.2"], "held-ambiguous")
        self.assertGreater(int(report["held_nanousd"]), 0)
        self.assertEqual(int(report["spent_nanousd"]), 0)

    def test_invalid_key_fails_closed_before_any_paid_traffic(self):
        provider = FakeProvider()
        provider.quota_queue = [
            {"code": 401, "msg": "invalid api key", "success": False} for _ in range(8)
        ]
        provider.generate_handler = default_generate
        error = self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        self.assertIn("quota anchor failed", error)
        self.assertEqual(provider.posts, [])


class SecretContainmentTests(MainCase):
    def test_key_never_reaches_the_report_checkpoint_or_output(self):
        provider = happy_provider()
        code, stdout, stderr = self.invoke(self.base_argv() + ["--execute"], provider=provider)
        self.assertEqual(code, 0)
        for artifact in (
            stdout,
            stderr,
            Path(self.report).read_text(),
            Path(self.checkpoint).read_text(),
        ):
            self.assertNotIn(SECRET, artifact)

    def test_key_is_redacted_from_typed_provider_error_details(self):
        provider = happy_provider()

        def leaky(request):
            raise http_error(
                request.full_url,
                400,
                {"error": {"code": "1210", "message": f"bad request from key {SECRET}"}},
            )

        provider.generate_handler = leaky
        error = self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        self.assertNotIn(SECRET, error)
        self.assertNotIn(SECRET, Path(self.report).read_text())


class ResumeTests(MainCase):
    def test_resume_skips_completed_legs_and_never_resends_a_held_leg(self):
        provider = happy_provider()
        calls = []

        def flaky(request):
            calls.append(request)
            if len(calls) == 4:
                raise urllib.error.URLError("connection reset")
            return default_generate(request)

        provider.generate_handler = flaky
        self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        checkpoint = json.loads(Path(self.checkpoint).read_text())
        self.assertEqual(checkpoint["schema"], "glm-live-calibration-checkpoint/v1")
        self.assertEqual(len(checkpoint["records"]), 3)
        first_run_id = checkpoint["run_id"]

        provider2 = happy_provider()
        error = self.invoke_error(
            self.base_argv() + ["--execute", "--resume", self.checkpoint],
            provider=provider2,
        )
        # Leg 4 stays held-ambiguous, so the run can never become formally complete, but the
        # remaining legs must finish without re-sending it.
        self.assertIn("coverage incomplete", error)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["run_id"], first_run_id)
        self.assertEqual(len(provider2.posts), 2, "only the two never-started legs run")
        self.assertEqual(len(report["legs"]), 5)
        statuses = report["leg_status"]
        self.assertEqual(statuses["messages-stream:glm-5-turbo"], "held-ambiguous")
        self.assertEqual(statuses["messages:glm-4.7"], "ok")
        self.assertEqual(statuses["messages-stream:glm-4.7"], "ok")
        self.assertFalse(report["complete"])
        self.assertGreater(int(report["held_nanousd"]), 0)

    def test_resume_rejects_a_mismatched_run_identity(self):
        provider = happy_provider()
        self.invoke(self.base_argv() + ["--execute", "--models", "glm-4.7"], provider=provider)
        error = self.invoke_error(
            self.base_argv() + ["--execute", "--resume", self.checkpoint,
                                "--models", "glm-4.7", "--budget-usd", "0.04"],
            provider=happy_provider(),
        )
        self.assertIn("budget", error)
        error = self.invoke_error(
            ["--profile", "other-sub", "--base-url", BASE_URL,
             "--report", self.report, "--checkpoint", self.checkpoint,
             "--execute", "--resume", self.checkpoint, "--models", "glm-4.7"],
            provider=happy_provider(),
        )
        self.assertIn("profile", error)

    def test_fresh_run_refuses_to_overwrite_an_existing_checkpoint(self):
        provider = happy_provider()
        self.invoke(self.base_argv() + ["--execute", "--models", "glm-4.7"], provider=provider)
        error = self.invoke_error(
            self.base_argv() + ["--execute"], provider=happy_provider()
        )
        self.assertIn("--resume", error)


class ReportCompletenessTests(MainCase):
    def test_incomplete_run_still_writes_a_full_report_shape(self):
        provider = happy_provider()

        def broken(request):
            raise urllib.error.URLError("connection reset")

        provider.generate_handler = broken
        self.invoke_error(self.base_argv() + ["--execute"], provider=provider)
        report = json.loads(Path(self.report).read_text())
        for key in (
            "schema", "run_id", "complete", "failure", "target", "budget_nanousd",
            "spent_nanousd", "held_nanousd", "models", "legs", "leg_status", "coverage",
            "unavailable_capabilities", "unattributed_deltas", "quota_anchor",
            "quota_wall_evidence", "unknowns",
        ):
            self.assertIn(key, report)
        self.assertFalse(report["complete"])
        self.assertIsNotNone(report["failure"])
        self.assertNotIn("api_key", report["target"])
        self.assertNotIn("key", report["target"])


if __name__ == "__main__":
    unittest.main()
