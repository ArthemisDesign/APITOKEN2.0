import unittest
import dataclasses
import json
import os
import subprocess
import tempfile
from unittest import mock

from tools.gemini_calibration import run_live


def event(request_id: str = "req-1", profile: str = "profile-a", model: str = "gemini-2.5-flash"):
    value = {
        "request_id": request_id,
        "profile_id": profile,
        "model": model,
        "service_tier": "standard",
        "inference_geo": "global",
        "tariff_schedule_id": "google/test/v1",
        "completed_at": "100",
    }
    value.update({field: "0" for field in run_live.EVENT_TOKEN_FIELDS})
    value.update({field: "0" for field in run_live.EVENT_MONEY_FIELDS})
    value["input_tokens"] = "10"
    value["api_input_nanousd"] = "100"
    value["api_total_nanousd"] = "100"
    return value


def capacity(events=None):
    return {
        "calibration_authority_available": True,
        "calibration_delivery": {
            "pending_events": 0,
            "dropped_events": 0,
            "persistence_ok": True,
            "queue_limit": 4096,
        },
        "calibration_recent_turn_limit": 512,
        "calibration_recent_turns": events or [],
    }


def report_record(
    request_id: str = "req-1",
    profile: str = "profile-a",
    model: str = "gemini-2.5-flash",
    leg: str = "thinking:gemini-2.5-flash:default",
):
    immutable = event(request_id, profile, model)
    return {
        "profile_id": profile,
        "plan": "google_ai_pro",
        "leg": leg,
        "kind": "thinking",
        "model": model,
        "request_id": request_id,
        "tariff_schedule_id": immutable["tariff_schedule_id"],
        "actual_nanousd": immutable["api_total_nanousd"],
        "usage": {field: immutable[field] for field in run_live.EVENT_TOKEN_FIELDS},
        "api_cost": {field: immutable[field] for field in run_live.EVENT_MONEY_FIELDS},
    }


class GeminiLiveCalibrationTests(unittest.TestCase):
    def test_dry_run_is_the_default_and_sends_nothing(self):
        with mock.patch.object(run_live, "CapacityReader") as capacity_reader, mock.patch.object(
            run_live, "JsonHttpClient"
        ) as api_client:
            self.assertEqual(run_live.main([]), 0)
        capacity_reader.assert_not_called()
        api_client.assert_not_called()

    def test_budget_parser_is_exact_and_hard_caps_at_forty_dollars(self):
        self.assertEqual(run_live.usd_to_nano("40"), 40_000_000_000)
        self.assertEqual(run_live.usd_to_nano("0.000000001"), 1)
        with self.assertRaises(run_live.CalibrationError):
            run_live.usd_to_nano("0.0000000001")
        budget = run_live.Budget(run_live.MAX_BUDGET_NANO)
        budget.require(run_live.MAX_BUDGET_NANO)
        budget.charge("profile-a", 1, run_live.MAX_BUDGET_NANO)
        with self.assertRaises(run_live.CalibrationError):
            budget.require(run_live.MAX_BUDGET_NANO)

    def test_delivery_baseline_fails_closed_on_pending_dropped_or_missing_authority(self):
        run_live.require_healthy_delivery(capacity())
        pending = capacity()
        pending["calibration_delivery"]["pending_events"] = 1
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(pending)
        dropped = capacity()
        dropped["calibration_delivery"]["dropped_events"] = 1
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(dropped, require_empty=False)
        missing = capacity()
        missing["calibration_authority_available"] = False
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(missing)
        profile_failure = capacity()
        profile_failure["profiles"] = [{"calibration_persistence_ok": False}]
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(profile_failure)

    def test_recent_turn_requires_complete_cost_vector_and_exact_attribution(self):
        payload = capacity([event()])
        parsed = run_live.recent_turn_events(payload)
        self.assertEqual(parsed["req-1"]["api_total_nanousd"], 100)
        self.assertEqual(
            run_live.exact_new_turn(
                set(), payload, "req-1", "profile-a", "gemini-2.5-flash"
            )["request_id"],
            "req-1",
        )
        concurrent = capacity([
            event("req-1"),
            event("req-2", profile="profile-b", model="gemini-3.6-flash"),
        ])
        self.assertEqual(
            run_live.exact_new_turn(
                set(), concurrent, "req-1", "profile-a", "gemini-2.5-flash"
            )["request_id"],
            "req-1",
        )
        rebound = capacity([event("req-1", profile="profile-b")])
        with self.assertRaises(run_live.CalibrationError):
            run_live.exact_new_turn(
                set(), rebound, "req-1", "profile-a", "gemini-2.5-flash"
            )
        with self.assertRaises(run_live.CalibrationError):
            run_live.exact_new_turn(
                {"req-1"}, payload, "req-1", "profile-a", "gemini-2.5-flash"
            )
        broken = event()
        broken["api_total_nanousd"] = "101"
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(capacity([broken]))
        incomplete = event()
        incomplete.pop("thinking_output_tokens")
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(capacity([incomplete]))

    def test_matrix_covers_models_levels_stream_cache_audio_tool_search_long_and_image_sizes(self):
        models = [
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-3.1-pro-preview",
            "gemini-3.1-flash-image",
            "gemini-2.5-flash",
        ]
        long_rates = run_live.ModelRates(
            tariff_schedule_id="google/test/v1",
            input_token_limit=1_000_000,
            input=1,
            audio_input=1,
            cached_input=1,
            cached_audio_input=1,
            output=1,
            image_output=0,
            long_threshold=200_000,
            long_input=2,
            long_audio_input=2,
            long_cached_input=2,
            long_cached_audio_input=2,
            long_output=2,
            search_unit="query",
            search=1,
            max_output_tokens=1_000,
        )
        legs = run_live.build_coverage_legs(
            models,
            "run",
            {"gemini-3.1-pro-preview": long_rates},
        )
        names = {leg.name for leg in legs}
        for level in ("minimal", "low", "medium", "high"):
            self.assertIn(f"thinking:gemini-3.6-flash:{level}", names)
        self.assertTrue(any(leg.stream for leg in legs if leg.model == "gemini-2.5-flash"))
        self.assertTrue(any(leg.kind == "cache" and leg.cache_phase == "write" for leg in legs))
        self.assertTrue(any(leg.kind == "cache" and leg.cache_phase == "read" for leg in legs))
        self.assertTrue(any(leg.kind == "audio" and leg.cache_phase == "write" for leg in legs))
        self.assertTrue(any(leg.kind == "audio" and leg.cache_phase == "read" for leg in legs))
        self.assertTrue(any(leg.kind == "tool" for leg in legs))
        self.assertTrue(any(leg.kind == "search" for leg in legs))
        self.assertTrue(any(leg.kind == "long" for leg in legs))
        self.assertEqual(
            {leg.image_size for leg in legs if leg.kind == "image"},
            {"1K", "2K", "4K"},
        )

    def test_cache_and_audio_replays_use_byte_identical_provider_content(self):
        legs = run_live.build_coverage_legs(["gemini-2.5-flash"], "run")
        cache = [leg for leg in legs if leg.kind == "cache"]
        audio = [leg for leg in legs if leg.kind == "audio"]
        self.assertEqual(run_live.body_for_leg(cache[0], "run"), run_live.body_for_leg(cache[1], "run"))
        self.assertEqual(run_live.body_for_leg(audio[0], "run"), run_live.body_for_leg(audio[1], "run"))

    def test_usage_verification_distinguishes_unavailable_token_classes(self):
        raw = event()
        raw["output_tokens"] = "2"
        parsed = run_live.recent_turn_events(capacity([raw]))["req-1"]
        audio = run_live.Leg("audio", "gemini-2.5-flash", "audio")
        self.assertIn("audio", run_live.verify_leg_usage(audio, parsed))
        parsed["audio_input_tokens"] = 5
        parsed["output_tokens"] = 2
        self.assertIsNone(run_live.verify_leg_usage(audio, parsed))
        tool = run_live.Leg("tool", "gemini-2.5-flash", "tool")
        self.assertIn("tool", run_live.verify_leg_usage(tool, parsed))
        parsed["tool_prompt_tokens"] = 1
        self.assertIsNone(run_live.verify_leg_usage(tool, parsed))

    def test_fraction_delta_rejects_window_reset_or_missing_reset_identity(self):
        before = {"used_5h": 10, "reset_5h": 100}
        self.assertEqual(
            run_live.fraction_delta(before, {"used_5h": 20, "reset_5h": 100}, "used_5h"),
            10,
        )
        self.assertIsNone(
            run_live.fraction_delta(before, {"used_5h": 20, "reset_5h": 200}, "used_5h")
        )
        self.assertIsNone(
            run_live.fraction_delta({"used_5h": 10}, {"used_5h": 20}, "used_5h")
        )

    def test_upper_bound_uses_proved_search_and_image_ceilings_only(self):
        rates = run_live.ModelRates(
            tariff_schedule_id="google/test/v1",
            input_token_limit=1_000,
            input=1,
            audio_input=2,
            cached_input=1,
            cached_audio_input=2,
            output=3,
            image_output=4,
            long_threshold=100,
            long_input=10,
            long_audio_input=20,
            long_cached_input=10,
            long_cached_audio_input=20,
            long_output=30,
            search_unit="query",
            search=100,
            max_output_tokens=1_000,
        )
        with self.assertRaises(run_live.UnboundedCostError):
            rates.upper_bound(101, 5, "search")
        grounded = dataclasses.replace(rates, search_unit="grounded_prompt")
        self.assertEqual(grounded.upper_bound(101, 5, "search"), 1_000 * 20 + 5 * 30 + 100)
        self.assertEqual(
            rates.upper_bound(10, 5, "image", "4K"),
            1_000 * 20 + 5 * 30 + 2_520 * 4,
        )
        with self.assertRaises(run_live.UnboundedCostError):
            rates.upper_bound(10, 5, "image", "8K")

    def test_every_generation_upper_bound_covers_hidden_provider_prompt_at_full_context_limit(self):
        rates = run_live.ModelRates(
            tariff_schedule_id="google/test/v1",
            input_token_limit=1_000,
            input=1,
            audio_input=2,
            cached_input=1,
            cached_audio_input=2,
            output=3,
            image_output=0,
            long_threshold=200,
            long_input=10,
            long_audio_input=20,
            long_cached_input=10,
            long_cached_audio_input=20,
            long_output=30,
            search_unit="query",
            search=100,
            max_output_tokens=1_000,
        )
        for kind in ("fresh", "thinking", "cache", "audio", "tool", "long"):
            with self.subTest(kind=kind):
                self.assertEqual(rates.upper_bound(10, 5, kind), 1_000 * 20 + 5 * 30)
        with self.assertRaises(run_live.UnboundedCostError):
            rates.upper_bound(1_001, 5, "fresh")

    def test_profitability_excludes_concurrent_or_unresolved_quota_attribution(self):
        records = [
            {
                "plan": "google_ai_pro",
                "model": "gemini-a",
                "kind": "fresh",
                "actual_nanousd": "100",
                "fraction_delta_5h": 10,
                "profitability_eligible": True,
                "quota_snapshot_resolved": True,
            },
            {
                "plan": "google_ai_pro",
                "model": "gemini-b",
                "kind": "fresh",
                "actual_nanousd": "300",
                "fraction_delta_5h": 10,
                "profitability_eligible": True,
                "quota_snapshot_resolved": True,
            },
            {
                "plan": "google_ai_pro",
                "model": "gemini-concurrent",
                "kind": "fresh",
                "actual_nanousd": "999999",
                "fraction_delta_5h": 1,
                "profitability_eligible": False,
                "quota_snapshot_resolved": True,
            },
            {
                "plan": "google_ai_pro",
                "model": "gemini-unresolved",
                "kind": "fresh",
                "actual_nanousd": "999999",
                "fraction_delta_5h": 0,
                "profitability_eligible": True,
                "quota_snapshot_resolved": False,
            },
        ]
        rows = run_live.model_profitability(records)
        self.assertEqual([row["model"] for row in rows], ["gemini-b", "gemini-a"])
        self.assertTrue(all(row["plan"] == "google_ai_pro" for row in rows))
        self.assertTrue(all(row["window"] == "5h" for row in rows))

    def test_production_paths_use_the_stable_gemini_plane(self):
        self.assertIn("127.0.0.1:8794/gemini-subs", run_live.remote_capacity_command())

    def test_production_ssh_keeps_secrets_remote_and_never_retries_paid_generation(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        failed = subprocess.CompletedProcess([], 255, stdout=b"", stderr=b"ambiguous")
        with mock.patch.dict(os.environ, {"CLAUDE_API_KEYS": "local-secret-must-not-leak"}), mock.patch.object(
            run_live.subprocess, "run", return_value=failed
        ) as invoked:
            with self.assertRaises(run_live.CalibrationError):
                client.request(
                    "/v1beta/models/gemini-2.5-flash:generateContent",
                    "POST",
                    {"contents": []},
                    "profile-a",
                    calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                )
        self.assertEqual(invoked.call_count, 1)
        remote_command = invoked.call_args.args[0][2]
        self.assertNotIn("local-secret-must-not-leak", remote_command)
        self.assertIn("$calibration_key", remote_command)
        self.assertIn("%header{x-apitoken-execution-state}", remote_command)

    def test_production_ssh_retries_only_quota_free_count(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        failed = subprocess.CompletedProcess([], 255, stdout=b"", stderr=b"temporary")
        succeeded = subprocess.CompletedProcess(
            [], 0, stdout=b'{"totalTokens":10}\n__CALIBRATION_HTTP__200\n', stderr=b""
        )
        with mock.patch.object(
            run_live.subprocess, "run", side_effect=[failed, succeeded]
        ) as invoked, mock.patch.object(run_live.time, "sleep", return_value=None):
            result = client.request(
                "/v1beta/models/gemini-2.5-flash:countTokens",
                "POST",
                {"contents": []},
                "profile-a",
            )
        self.assertEqual(result, {"totalTokens": 10})
        self.assertEqual(invoked.call_count, 2)

    def test_production_ssh_preserves_authoritative_not_started_header(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        refused = subprocess.CompletedProcess(
            [],
            0,
            stdout=(
                b'{"error":{"code":503,"status":"UNAVAILABLE"}}'
                b"\n__CALIBRATION_HTTP__503\nnot_started"
            ),
            stderr=b"",
        )
        with mock.patch.object(run_live.subprocess, "run", return_value=refused):
            with self.assertRaises(run_live.HttpCalibrationError) as caught:
                client.request(
                    "/v1beta/models/gemini-2.5-flash:generateContent",
                    "POST",
                    {"contents": []},
                    "profile-a",
                    calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                )
        self.assertTrue(caught.exception.execution_not_started)
        self.assertTrue(run_live.is_explicit_transient_stop(caught.exception))

    def test_only_explicit_provider_stops_are_resume_safe(self):
        self.assertTrue(
            run_live.is_explicit_transient_stop(
                run_live.HttpCalibrationError(
                    "/generate", 503, "generic sanitized failure", execution_not_started=True
                )
            )
        )
        self.assertTrue(
            run_live.is_explicit_transient_stop(
                run_live.HttpCalibrationError(
                    "/generate", 429, "quota reached", execution_not_started=True
                )
            )
        )
        self.assertFalse(
            run_live.is_explicit_transient_stop(
                run_live.HttpCalibrationError("/generate", 503, "generic proxy failure")
            )
        )
        self.assertFalse(
            run_live.is_explicit_transient_stop(
                run_live.HttpCalibrationError(
                    "/generate", 502, "bad gateway", execution_not_started=True
                )
            )
        )

    def test_resume_rehydrates_exact_spend_and_rejects_ambiguous_paid_failure(self):
        payload = {
            "schema": "gemini-live-calibration/v2",
            "run_id": "gemini-cal-12345678-deadbeef",
            "complete": False,
            "failure": None,
            "resume_safe": True,
            "resume_proof": "x-apitoken-execution-state:not_started",
            "budget_nanousd_total": "1000000000",
            "spent_nanousd_total": "100",
            "spent_nanousd_per_profile": {"profile-a": "100", "profile-b": "0"},
            "profiles": ["profile-a", "profile-b"],
            "models": ["gemini-2.5-flash"],
            "records": [report_record()],
            "unavailable_capabilities": [{
                "profile_id": "profile-a",
                "model": "gemini-2.5-flash",
                "capability": "thinking:gemini-2.5-flash:default",
                "reason": "thinking token class was not observed",
            }],
        }
        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with open(report, "w", encoding="utf-8") as report_file:
                json.dump(payload, report_file)
            state = run_live.load_resume_report(report, 1_000_000_000, None)
            self.assertEqual(state.spent_nano, 100)
            self.assertEqual(state.spent_by_profile, {"profile-a": 100, "profile-b": 0})
            self.assertEqual(state.records[0]["request_id"], "req-1")
            self.assertEqual(len(state.unavailable), 1)

            payload["resume_safe"] = False
            payload["failure"] = "paid generation SSH transport failed: ambiguous"
            with open(report, "w", encoding="utf-8") as report_file:
                json.dump(payload, report_file)
            with self.assertRaises(run_live.CalibrationError):
                run_live.load_resume_report(report, 1_000_000_000, None)

    def test_legacy_retryinfo_503_partial_report_can_resume_but_keeps_budget_identity(self):
        payload = {
            "schema": "gemini-live-calibration/v1",
            "run_id": "gemini-cal-12345678-deadbeef",
            "complete": False,
            "failure": (
                '/generate returned HTTP 503: {"details":[{"@type":'
                '"type.googleapis.com/google.rpc.RetryInfo"}],"status":"UNAVAILABLE"}'
            ),
            "budget_nanousd_total": "1000000000",
            "spent_nanousd_total": "100",
            "spent_nanousd_per_profile": {"profile-a": "100"},
            "profiles": ["profile-a"],
            "models": ["gemini-2.5-flash"],
            "records": [report_record()],
            "unavailable_capabilities": [],
        }
        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with open(report, "w", encoding="utf-8") as report_file:
                json.dump(payload, report_file)
            state = run_live.load_resume_report(report, 1_000_000_000, ["gemini-2.5-flash"])
            self.assertEqual(state.run_id, payload["run_id"])
            with self.assertRaises(run_live.CalibrationError):
                run_live.load_resume_report(report, 999_999_999, None)

    def test_resume_skips_completed_leg_continues_healthy_profile_and_checkpoints_stop(self):
        model = {
            "id": "gemini-2.5-flash",
            "tariff_schedule_id": "google/test/v1",
            "input_token_limit": "1048576",
            "output_token_limit": "8192",
            "rates": {
                "input_nanousd_per_token": "1",
                "audio_input_nanousd_per_token": "1",
                "cached_input_nanousd_per_token": "1",
                "cached_audio_input_nanousd_per_token": "1",
                "output_nanousd_per_token": "1",
                "image_output_nanousd_per_token": "0",
                "long_context_threshold": str(2**64 - 1),
                "long_input_nanousd_per_token": "1",
                "long_audio_input_nanousd_per_token": "1",
                "long_cached_input_nanousd_per_token": "1",
                "long_cached_audio_input_nanousd_per_token": "1",
                "long_output_nanousd_per_token": "1",
            },
            "search": {"billing_unit": "grounded_prompt", "nanousd_per_unit": "1"},
        }
        baseline = capacity()
        baseline["profiles"] = [
            {
                "id": profile,
                "plan": "google_ai_pro",
                "authenticated": True,
                "cooling_until": 0,
                "calibration_persistence_ok": True,
                "windows": [],
            }
            for profile in ("profile-a", "profile-b")
        ]
        baseline["conversion_models"] = [model]
        payload = {
            "schema": "gemini-live-calibration/v2",
            "run_id": "gemini-cal-12345678-deadbeef",
            "complete": False,
            "failure": None,
            "resume_safe": True,
            "resume_proof": "x-apitoken-execution-state:not_started",
            "budget_nanousd_total": "1000000000",
            "spent_nanousd_total": "100",
            "spent_nanousd_per_profile": {"profile-a": "100", "profile-b": "0"},
            "profiles": ["profile-a", "profile-b"],
            "models": ["gemini-2.5-flash"],
            "records": [report_record()],
            "unavailable_capabilities": [],
        }
        calls = []

        def execute(runner, leg, profile):
            calls.append((profile, leg.name))
            if profile == "profile-b":
                raise run_live.HttpCalibrationError(
                    "/generate",
                    503,
                    "sanitized unavailable",
                    execution_not_started=True,
                )
            record = report_record(
                request_id=f"req-{len(calls) + 1}",
                profile=profile,
                leg=leg.name,
            )
            record["coverage_error"] = None
            runner.budget.charge(profile, 100, 1_000)
            runner.records.append(record)
            return record

        fake_capacity = mock.Mock()
        fake_capacity.read.return_value = baseline
        with tempfile.TemporaryDirectory() as directory:
            source = os.path.join(directory, "source.json")
            report = os.path.join(directory, "report.json")
            with open(source, "w", encoding="utf-8") as report_file:
                json.dump(payload, report_file)
            with mock.patch.dict(os.environ, {"APITOKEN_API_KEY": "test-key"}), mock.patch.object(
                run_live, "CapacityReader", return_value=fake_capacity
            ), mock.patch.object(run_live, "JsonHttpClient", return_value=mock.Mock()), mock.patch.object(
                run_live.Runner, "execute_leg", new=execute
            ):
                with self.assertRaises(run_live.CalibrationError):
                    run_live.main([
                        "--execute",
                        "--budget-usd",
                        "1",
                        "--capacity-command",
                        "unused",
                        "--resume-report",
                        source,
                        "--report",
                        report,
                    ])
            with open(report, encoding="utf-8") as report_file:
                result = json.load(report_file)
        self.assertNotIn(
            ("profile-a", "thinking:gemini-2.5-flash:default"),
            calls,
        )
        self.assertEqual(calls[0], ("profile-b", "thinking:gemini-2.5-flash:default"))
        self.assertTrue(result["resume_safe"])
        self.assertFalse(result["complete"])
        self.assertEqual(result["spent_nanousd_total"], "800")
        self.assertEqual({item["profile_id"] for item in result["pending_legs"]}, {"profile-b"})

    def test_partial_failure_writes_an_explicit_incomplete_report(self):
        conversion_model = {
            "id": "gemini-2.5-flash",
            "tariff_schedule_id": "google/test/v1",
            "input_token_limit": "1048576",
            "output_token_limit": "8192",
            "rates": {
                "input_nanousd_per_token": "1",
                "audio_input_nanousd_per_token": "1",
                "cached_input_nanousd_per_token": "1",
                "cached_audio_input_nanousd_per_token": "1",
                "output_nanousd_per_token": "1",
                "image_output_nanousd_per_token": "0",
                "long_context_threshold": str(2**64 - 1),
                "long_input_nanousd_per_token": "1",
                "long_audio_input_nanousd_per_token": "1",
                "long_cached_input_nanousd_per_token": "1",
                "long_cached_audio_input_nanousd_per_token": "1",
                "long_output_nanousd_per_token": "1",
            },
            "search": {"billing_unit": "grounded_prompt", "nanousd_per_unit": "1"},
        }
        baseline = capacity()
        baseline["profiles"] = [{
            "id": "profile-a",
            "plan": "google_ai_pro",
            "authenticated": True,
            "cooling_until": 0,
            "calibration_persistence_ok": True,
            "windows": [],
        }]
        baseline["conversion_models"] = [conversion_model]

        fake_capacity = mock.Mock()
        fake_capacity.read.return_value = baseline
        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with mock.patch.dict(os.environ, {"APITOKEN_API_KEY": "test-key"}), mock.patch.object(
                run_live, "CapacityReader", return_value=fake_capacity
            ), mock.patch.object(run_live, "JsonHttpClient", return_value=mock.Mock()), mock.patch.object(
                run_live.Runner,
                "execute_leg",
                side_effect=run_live.CalibrationError("simulated paid-stage failure"),
            ):
                with self.assertRaises(run_live.CalibrationError):
                    run_live.main([
                        "--execute",
                        "--budget-usd",
                        "1",
                        "--models",
                        "gemini-2.5-flash",
                        "--capacity-command",
                        "unused",
                        "--report",
                        report,
                    ])
            with open(report, encoding="utf-8") as report_file:
                payload = json.load(report_file)
            self.assertFalse(payload["complete"])
            self.assertFalse(payload["resume_safe"])
            self.assertIn("simulated paid-stage failure", payload["failure"])
            self.assertEqual(payload["spent_nanousd_total"], "0")

    def test_runner_supplies_one_exact_request_id_and_charges_actual_once(self):
        class FakeApi:
            request_id = None

            def request(
                self,
                path,
                method="GET",
                body=None,
                target_profile=None,
                raw_ok=False,
                calibration_request_id=None,
            ):
                if path.endswith(":countTokens"):
                    return {"totalTokens": 10}
                self.request_id = calibration_request_id
                return {}

        class FakeCapacity:
            def __init__(self, api):
                self.api = api

            def read(self):
                events = []
                if self.api.request_id:
                    turn = event(self.api.request_id)
                    turn["output_tokens"] = "1"
                    events = [turn]
                payload = capacity(events)
                payload["profiles"] = [{
                    "id": "profile-a",
                    "plan": "google_ai_pro",
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "quota_updated_at": 101 if self.api.request_id else 99,
                    "windows": [],
                }]
                return payload

        rates = run_live.ModelRates(
            tariff_schedule_id="google/test/v1",
            input_token_limit=1_000,
            input=10,
            audio_input=10,
            cached_input=1,
            cached_audio_input=1,
            output=10,
            image_output=0,
            long_threshold=1_000,
            long_input=10,
            long_audio_input=10,
            long_cached_input=1,
            long_cached_audio_input=1,
            long_output=10,
            search_unit="prompt",
            search=1,
            max_output_tokens=1_000,
        )
        api = FakeApi()
        budget = run_live.Budget(1_000_000)
        runner = run_live.Runner(
            api,
            FakeCapacity(api),
            {"gemini-2.5-flash": rates},
            budget,
            timeout=1,
            delay=0,
            run_id="run",
        )
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            record = runner.execute_leg(
                run_live.Leg("fresh", "gemini-2.5-flash", "fresh"),
                "profile-a",
            )
        self.assertRegex(api.request_id, r"^[0-9a-f-]{36}$")
        self.assertEqual(record["request_id"], api.request_id)
        self.assertTrue(record["quota_snapshot_resolved"])
        self.assertEqual(budget.total_nano, 100)
        self.assertEqual(budget.by_profile["profile-a"], 100)


if __name__ == "__main__":
    unittest.main()
