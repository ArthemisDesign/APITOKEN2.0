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


def minimal_zero_thinking_false_negative_report():
    profile = "profile-a"
    model = "gemini-3-flash-preview"
    leg = f"thinking:{model}:minimal"
    reason = run_live.THINKING_TOKENS_NOT_OBSERVED
    record = report_record(
        request_id="req-minimal-complete",
        profile=profile,
        model=model,
        leg=leg,
    )
    record.update({
        "kind": "thinking",
        "thinking_level": "minimal",
        "stream": False,
        "actual_nanousd": "556500",
        "coverage_error": reason,
        "response_evidence": {
            "model_version": model,
            "visible_text_chars": 336,
            "terminal_finish": True,
            "terminal_usage": True,
            "usage_matches_immutable_event": True,
            "response_frames": 1,
        },
    })
    record["usage"]["output_tokens"] = "182"
    record["usage"]["thinking_output_tokens"] = "0"
    record["api_cost"].update({
        "api_input_nanousd": "556500",
        "api_total_nanousd": "556500",
    })
    miss = {
        "profile_id": profile,
        "model": model,
        "capability": leg,
        "reason": reason,
        "blocking": True,
    }
    return {
        "schema": "gemini-live-calibration/v2",
        "run_id": "gemini-cal-12345678-deadbeef",
        "complete": False,
        "failure": f"{profile}/{leg}: paid response proof failed: {reason}",
        "resume_safe": False,
        "resume_proof": None,
        "budget_nanousd_total": "21000000000",
        "spent_nanousd_total": "556500",
        "spent_nanousd_per_profile": {profile: "556500"},
        "profiles": [profile],
        "models": [model],
        "records": [record],
        "unavailable_capabilities": [dict(miss)],
        "blocking_unavailable_capabilities": [dict(miss)],
        "pending_legs": [{
            "profile_id": profile,
            "model": model,
            "capability": f"thinking:{model}:low",
        }],
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

    def test_gemini_37_current_tariff_requires_the_exact_minimal_ceiling(self):
        rates = run_live.ModelRates(
            tariff_schedule_id="google/gemini/gemini-3.7-flash/epoch-0/v1",
            input_token_limit=1_048_576,
            input=750,
            audio_input=750,
            cached_input=75,
            cached_audio_input=75,
            output=3_750,
            image_output=0,
            long_threshold=(1 << 64) - 1,
            long_input=750,
            long_audio_input=750,
            long_cached_input=75,
            long_cached_audio_input=75,
            long_output=3_750,
            search_unit="query",
            search=14_000_000,
            max_output_tokens=65_536,
        )
        self.assertEqual(
            rates.upper_bound(
                1,
                run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
                "fresh",
            ),
            788_352_000,
        )

    def test_gemini_37_admission_cli_is_closed_to_one_canary_contract(self):
        sha = "a" * 40
        args = run_live.parse_args([
            "--gemini-37-admission",
            "--admission-profile",
            "profile-a",
            "--implementation-sha",
            sha,
            "--production-capacity-port",
            "18895",
            "--production-api-port",
            "18895",
            "--budget-usd",
            "0.788352",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(args.http_timeout, run_live.DEFAULT_HTTP_TIMEOUT_SECONDS)
        self.assertEqual(args.http_timeout, 600)
        self.assertEqual(plan["schema"], "gemini-3.7-admission-plan/v1")
        self.assertEqual(plan["planned_count_requests"], 1)
        self.assertEqual(plan["planned_paid_generation_requests"], 1)
        self.assertEqual(plan["model"], run_live.GEMINI_37_ADMISSION_MODEL)
        self.assertEqual(plan["implementation_sha"], sha)
        self.assertIn("no-resume-retry-reconnect-or-replay", plan["guards"])

        invalid_sets = (
            ["--resume-report", "/tmp/old.json"],
            ["--models", run_live.GEMINI_37_ADMISSION_MODEL],
            ["--production-capacity-port", "18895", "--production-api-port", "18896"],
        )
        for extra in invalid_sets:
            with self.subTest(extra=extra), self.assertRaises(SystemExit):
                run_live.parse_args([
                    "--gemini-37-admission",
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    sha,
                    *extra,
                ])

    def test_gemini_37_thinking_levels_cli_plans_one_paid_generation_per_level(self):
        sha = "b" * 40
        args = run_live.parse_args([
            "--gemini-37-thinking-levels",
            "--admission-profile",
            "profile-a",
            "--implementation-sha",
            sha,
            "--production-capacity-port",
            "18898",
            "--production-api-port",
            "18898",
            "--budget-usd",
            "2.365056",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(plan["schema"], "gemini-3.7-admission-plan/v1")
        self.assertEqual(plan["planned_count_requests"], 3)
        self.assertEqual(plan["planned_paid_generation_requests"], 3)
        self.assertEqual(
            plan["thinking_levels"],
            list(run_live.GEMINI_37_THINKING_LEVELS),
        )
        self.assertEqual(plan["implementation_sha"], sha)
        self.assertIn("no-resume-retry-reconnect-or-replay", plan["guards"])
        # minimal is never a Gemini 3.7 product effort: the model rejects it and omission
        # already means medium.
        self.assertNotIn("minimal", run_live.GEMINI_37_THINKING_LEVELS)

        with self.assertRaises(SystemExit):
            run_live.parse_args([
                "--gemini-37-admission",
                "--gemini-37-thinking-levels",
                "--admission-profile",
                "profile-a",
                "--implementation-sha",
                sha,
            ])
        for extra in (
            ["--resume-report", "/tmp/old.json"],
            ["--models", run_live.GEMINI_37_ADMISSION_MODEL],
        ):
            with self.subTest(extra=extra), self.assertRaises(SystemExit):
                run_live.parse_args([
                    "--gemini-37-thinking-levels",
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    sha,
                    *extra,
                ])

    def test_gemini_37_capabilities_cli_plans_one_paid_generation_per_control(self):
        sha = "c" * 40
        args = run_live.parse_args([
            "--gemini-37-capabilities",
            "--admission-profile",
            "profile-a",
            "--implementation-sha",
            sha,
            "--production-capacity-port",
            "18898",
            "--production-api-port",
            "18898",
            "--budget-usd",
            "7",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(plan["schema"], "gemini-3.7-capabilities-plan/v1")
        self.assertEqual(
            plan["planned_paid_generation_requests"],
            len(run_live.GEMINI_37_CAPABILITY_KINDS),
        )
        self.assertEqual(
            plan["capabilities"],
            list(run_live.GEMINI_37_CAPABILITY_KINDS),
        )
        # Search stays undispatched: per-query billing has no hard fanout ceiling.
        self.assertEqual(plan["skipped"][0]["capability"], "search")
        for other in ("--gemini-37-admission", "--gemini-37-thinking-levels"):
            with self.subTest(other=other), self.assertRaises(SystemExit):
                run_live.parse_args([
                    "--gemini-37-capabilities",
                    other,
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    sha,
                ])

    def test_gemini_37_capability_bodies_match_the_closed_matrix(self):
        run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:sse", "gemini-3.7-flash", "fresh",
                         stream=True, max_output_tokens=512),
            "run",
        )
        structured = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:structured", "gemini-3.7-flash", "fresh",
                         max_output_tokens=1024),
            "run",
        )
        self.assertEqual(
            structured["generationConfig"]["responseMimeType"],
            "application/json",
        )
        tool = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:tool-prompt", "gemini-3.7-flash", "tool",
                         max_output_tokens=512),
            "run",
        )
        self.assertEqual(
            tool["tools"][0]["functionDeclarations"][0]["name"],
            "calibration_probe",
        )
        image = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:image-input", "gemini-3.7-flash", "fresh",
                         max_output_tokens=1024),
            "run",
        )
        self.assertEqual(
            image["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "image/png",
        )
        write = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:cache-write", "gemini-3.7-flash", "cache",
                         max_output_tokens=1024),
            "run",
        )
        read = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:cache-read", "gemini-3.7-flash", "cache",
                         max_output_tokens=1024),
            "run",
        )
        self.assertEqual(write, read)
        with self.assertRaises(run_live.CalibrationError):
            run_live.body_for_gemini37_capability(
                run_live.Leg("admission:gemini-3.7-flash:other", "gemini-3.7-flash", "fresh"),
                "run",
            )

    def test_gemini_37_search_leg_body_and_budget_reserve(self):
        body = run_live.body_for_gemini37_capability(
            run_live.Leg("admission:gemini-3.7-flash:search", "gemini-3.7-flash", "search",
                         max_output_tokens=512),
            "run",
        )
        self.assertEqual(body["tools"], [{"googleSearch": {}}])
        rates = run_live.ModelRates(
            "google/test/v1", 1_000, 10, 10, 1, 1, 10, 0,
            1_000, 10, 10, 1, 1, 10, "query", 14_000_000, 1_000,
        )
        # Per-query billing stays unbounded in the generic matrix...
        with self.assertRaises(run_live.UnboundedCostError):
            rates.upper_bound(10, 5, "search")
        # ...while the closed 3.7 search admission substitutes the explicit query reserve.
        args = run_live.parse_args([
            "--gemini-37-search",
            "--admission-profile",
            "profile-a",
            "--implementation-sha",
            "d" * 40,
            "--production-capacity-port",
            "18898",
            "--production-api-port",
            "18898",
            "--budget-usd",
            "1",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(plan["schema"], "gemini-3.7-search-plan/v1")
        self.assertEqual(plan["planned_paid_generation_requests"], 1)
        self.assertEqual(
            plan["search_query_reserve"],
            run_live.GEMINI_37_SEARCH_QUERY_RESERVE,
        )
        self.assertGreater(run_live.GEMINI_37_SEARCH_QUERY_RESERVE, 0)

    def test_gemini_37_media_legs_carry_real_bounded_payloads(self):
        bodies = {
            name: run_live.body_for_gemini37_capability(
                run_live.Leg(
                    f"admission:gemini-3.7-flash:{name}",
                    "gemini-3.7-flash",
                    "fresh",
                    max_output_tokens=1024,
                ),
                "run",
            )
            for name in run_live.GEMINI_37_MEDIA_KINDS
        }
        self.assertEqual(
            bodies["audio-input"]["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "audio/wav",
        )
        self.assertEqual(
            bodies["video-input"]["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "video/mp4",
        )
        self.assertEqual(
            bodies["pdf-input"]["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "application/pdf",
        )
        # Every admission leg must demand a perception marker, so a silently dropped
        # attachment cannot pass as support.
        for name in run_live.GEMINI_37_MEDIA_KINDS:
            self.assertIn(name, run_live.GEMINI_37_MEDIA_EXPECTED_TEXT)

    def test_gemini_37_media_response_requires_the_perception_marker(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:pdf-input",
            model,
            "fresh",
            max_output_tokens=1024,
        )
        response = run_live.GenerationResponse(
            frames=({
                "modelVersion": "gemini-3.7-flash-tiered",
                "candidates": [{
                    "content": {"parts": [{"text": "CALIBRATION-BEACON-7734"}]},
                    "finishReason": "STOP",
                }],
                "usageMetadata": {"promptTokenCount": 300, "candidatesTokenCount": 9},
            },),
            stream=False,
        )
        immutable = event(model=model)
        immutable.update({"input_tokens": 300, "output_tokens": 9})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        _evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)

        wrong = dataclasses.replace(
            response,
            frames=({
                **response.frames[0],
                "candidates": [{
                    "content": {"parts": [{"text": "I cannot read documents."}]},
                    "finishReason": "STOP",
                }],
            },),
        )
        _evidence, error = run_live.verify_generation_response(leg, wrong, immutable)
        self.assertIn("perception marker", error)

    def test_gemini_37_tool_result_final_turn_cli_is_a_single_closed_leg(self):
        sha = "e" * 40
        args = run_live.parse_args([
            "--gemini-37-tool-result-final-turn",
            "--admission-profile",
            "profile-a",
            "--implementation-sha",
            sha,
            "--production-capacity-port",
            "18897",
            "--production-api-port",
            "18897",
            "--budget-usd",
            "1",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(plan["schema"], "gemini-3.7-tool-result-final-turn-plan/v1")
        self.assertEqual(plan["planned_count_requests"], 1)
        self.assertEqual(plan["planned_paid_generation_requests"], 1)
        self.assertEqual(plan["final_turn_shape"], "tool-result-only")
        self.assertEqual(plan["model"], run_live.GEMINI_37_ADMISSION_MODEL)
        self.assertEqual(
            plan["max_output_tokens"],
            run_live.GEMINI_37_TOOL_RESULT_FINAL_TURN_OUTPUT_TOKENS,
        )
        self.assertIn("no-resume-retry-reconnect-or-replay", plan["guards"])

        for other in (
            "--gemini-37-admission",
            "--gemini-37-thinking-levels",
            "--gemini-37-capabilities",
            "--gemini-37-search",
            "--gemini-37-media",
        ):
            with self.subTest(other=other), self.assertRaises(SystemExit):
                run_live.parse_args([
                    "--gemini-37-tool-result-final-turn",
                    other,
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    sha,
                ])
        for extra in (
            ["--resume-report", "/tmp/old.json"],
            ["--models", run_live.GEMINI_37_ADMISSION_MODEL],
        ):
            with self.subTest(extra=extra), self.assertRaises(SystemExit):
                run_live.parse_args([
                    "--gemini-37-tool-result-final-turn",
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    sha,
                    *extra,
                ])

    def test_gemini_37_tool_result_final_turn_body_has_no_final_text(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:tool-result-final-turn",
            model,
            "tool-result-final-turn",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_TOOL_RESULT_FINAL_TURN_OUTPUT_TOKENS,
        )
        body = run_live.body_for_gemini37_tool_result_final_turn(leg, "run")
        contents = body["contents"]
        self.assertEqual([content["role"] for content in contents], ["user", "model", "user"])
        # The wire contract under test: the FINAL turn carries only a functionResponse,
        # never a text part. Adding text would make the leg prove nothing.
        final_parts = contents[-1]["parts"]
        self.assertEqual(len(final_parts), 1)
        self.assertIn("functionResponse", final_parts[0])
        self.assertNotIn("text", final_parts[0])
        self.assertEqual(final_parts[0]["functionResponse"]["name"], "calibration_probe")
        # The replayed model call must carry the accepted stateless context-engineering
        # thought signature marker: portable clients do not retain opaque signatures.
        call_part = contents[1]["parts"][0]
        self.assertIn("functionCall", call_part)
        self.assertEqual(
            call_part["thoughtSignature"],
            "context_engineering_is_the_way_to_go",
        )
        # countTokens consumes the same contents/tools shape.
        counted = run_live.count_body(body)
        self.assertEqual(set(counted), {"contents", "tools"})
        self.assertEqual(counted["contents"], contents)

    def test_gemini_37_tool_result_final_turn_response_requires_the_marker(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:tool-result-final-turn",
            model,
            "tool-result-final-turn",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_TOOL_RESULT_FINAL_TURN_OUTPUT_TOKENS,
        )
        frames = (
            {"modelVersion": "gemini-3.7-flash-tiered",
             "candidates": [{"content": {"parts": [{"text": "CALIBRATION_"}]}}]},
            {"modelVersion": "gemini-3.7-flash-tiered",
             "candidates": [{
                 "content": {"parts": [{"text": "OK"}]},
                 "finishReason": "STOP",
             }],
             "usageMetadata": {"promptTokenCount": 60, "candidatesTokenCount": 3}},
        )
        response = run_live.GenerationResponse(frames=frames, stream=True)
        immutable = event(model=model)
        immutable.update({"input_tokens": 60, "output_tokens": 3})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertTrue(evidence["terminal_finish"])
        self.assertTrue(evidence["terminal_usage"])

        reinvoked = dataclasses.replace(
            response,
            frames=(
                frames[0],
                {**frames[1],
                 "candidates": [{
                     "content": {"parts": [
                         {"text": "OK"},
                         {"functionCall": {"name": "calibration_probe", "args": {}}},
                     ]},
                     "finishReason": "STOP",
                 }]},
            ),
        )
        _evidence, error = run_live.verify_generation_response(leg, reinvoked, immutable)
        self.assertIn("re-invoked a functionCall", error)

        wrong_text = dataclasses.replace(
            response,
            frames=(
                frames[0],
                {**frames[1],
                 "candidates": [{
                     "content": {"parts": [{"text": "I ran the tool."}]},
                     "finishReason": "STOP",
                 }]},
            ),
        )
        _evidence, error = run_live.verify_generation_response(leg, wrong_text, immutable)
        self.assertIn("did not match the exact", error)


    def test_gemini_media_matrix_cli_covers_every_model_exactly_once(self):
        matrix_args = []
        for index, model in enumerate(sorted(run_live.MEDIA_MATRIX_MODELS), start=1):
            matrix_args += ["--media-profile", f"{model}=profile-{index}"]
        args = run_live.parse_args([
            "--gemini-media-matrix",
            *matrix_args,
            "--production-capacity-port",
            "18898",
            "--production-api-port",
            "18898",
            "--budget-usd",
            "26.5518592",
        ])
        plan = run_live.dry_run_plan(args, run_live.usd_to_nano(args.budget_usd))
        self.assertEqual(plan["schema"], "gemini-media-matrix-plan/v1")
        legs_planned = sum(len(kinds) for kinds in run_live.MEDIA_MATRIX_MODELS.values())
        self.assertEqual(plan["planned_paid_generation_requests"], legs_planned)
        self.assertEqual(set(plan["models"]), set(run_live.MEDIA_MATRIX_MODELS))
        # The image-generation model admits only the PDF leg (its official input surface
        # is Text/Image/PDF); 2.5-flash has no official PDF claim; 3.7-flash stays out of
        # the matrix because its media evidence is already recorded.
        self.assertEqual(
            run_live.MEDIA_MATRIX_MODELS["gemini-3.1-flash-image"],
            ("pdf-input",),
        )
        self.assertNotIn("pdf-input", run_live.MEDIA_MATRIX_MODELS["gemini-2.5-flash"])
        self.assertNotIn("gemini-3.7-flash", run_live.MEDIA_MATRIX_MODELS)

        with self.assertRaises(SystemExit):
            run_live.parse_args(["--gemini-media-matrix"])
        with self.assertRaises(SystemExit):
            run_live.parse_args([
                "--gemini-media-matrix",
                "--gemini-37-media",
                *matrix_args,
            ])

    def test_gemini_media_matrix_generic_leg_carries_marker_and_exact_target(self):
        leg = run_live.Leg(
            "media:gemini-3.6-flash:video-input",
            "gemini-3.6-flash",
            "video",
            max_output_tokens=1024,
        )
        body = run_live.body_for_media_leg(leg)
        self.assertEqual(
            body["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "video/mp4",
        )
        response = run_live.GenerationResponse(
            frames=({
                "modelVersion": "gemini-3.6-flash",
                "candidates": [{
                    "content": {"parts": [{"text": "red"}]},
                    "finishReason": "STOP",
                }],
                "usageMetadata": {"promptTokenCount": 90, "candidatesTokenCount": 8},
            },),
            stream=False,
        )
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 90, "output_tokens": 8})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        _evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)

    def test_media_matrix_leg_accepts_the_private_wire_model_version(self):
        leg = run_live.Leg(
            "media:gemini-3.5-flash:video-input",
            "gemini-3.5-flash",
            "video",
            max_output_tokens=1024,
        )
        response = run_live.GenerationResponse(
            frames=({
                "modelVersion": "gemini-default",
                "candidates": [{
                    "content": {"parts": [{"text": "red"}]},
                    "finishReason": "STOP",
                }],
                "usageMetadata": {"promptTokenCount": 81, "candidatesTokenCount": 1},
            },),
            stream=False,
        )
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 81, "output_tokens": 1})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertEqual(evidence["upstream_model_version"], "gemini-default")
        self.assertEqual(evidence["model_version"], "gemini-3.5-flash")

    def test_gemini_37_brief_sse_accepts_a_single_visible_frame(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:sse",
            model,
            "fresh",
            stream=True,
            max_output_tokens=4096,
        )
        response = run_live.GenerationResponse(
            frames=(
                {"candidates": [{"content": {"parts": [{"text": "CALIBRATION_OK"}]}}]},
                {
                    "responseId": "resp-1",
                    "modelVersion": "gemini-3.7-flash-tiered",
                    "candidates": [{
                        "content": {"parts": [{"text": ""}]},
                        "finishReason": "STOP",
                    }],
                    "usageMetadata": {
                        "promptTokenCount": 49,
                        "candidatesTokenCount": 5,
                        "thoughtsTokenCount": 140,
                    },
                },
            ),
            stream=True,
        )
        immutable = event(model=model)
        immutable.update({
            "input_tokens": 49,
            "output_tokens": 145,
            "thinking_output_tokens": 140,
        })
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertTrue(evidence["incremental_sse"])

    def test_gemini_37_structured_response_requires_valid_schema_json(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:structured",
            model,
            "fresh",
            max_output_tokens=1024,
        )
        response = run_live.GenerationResponse(
            frames=({
                "modelVersion": "gemini-3.7-flash-tiered",
                "candidates": [{
                    "content": {
                        "parts": [{"text": '{"marker": "CALIBRATION_OK", "answer": 42}'}]
                    },
                    "finishReason": "STOP",
                }],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 12},
            },),
            stream=False,
        )
        immutable = event(model=model)
        immutable.update({"input_tokens": 10, "output_tokens": 12})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        _evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)

        broken = dataclasses.replace(
            response,
            frames=({
                **response.frames[0],
                "candidates": [{
                    "content": {"parts": [{"text": '{"marker": "WRONG", "answer": 42}'}]},
                    "finishReason": "STOP",
                }],
            },),
        )
        _evidence, error = run_live.verify_generation_response(leg, broken, immutable)
        self.assertIn("schema contract", error)

    def test_gemini_37_withdrawn_implementation_cannot_be_retried(self):
        self.assertEqual(
            run_live.GEMINI_37_WITHDRAWN_IMPLEMENTATION_SHAS,
            {
                "20d945ce59e9dea749ec7c74b7d322525bc29a05",
                "2c8aca0d1230bbf774b7e82ef11d651c4b705864",
            },
        )
        for implementation_sha in run_live.GEMINI_37_WITHDRAWN_IMPLEMENTATION_SHAS:
            with (
                self.subTest(implementation_sha=implementation_sha),
                self.assertRaises(SystemExit),
            ):
                run_live.parse_args([
                    "--gemini-37-admission",
                    "--admission-profile",
                    "profile-a",
                    "--implementation-sha",
                    implementation_sha,
                    "--production-capacity-port",
                    "18895",
                    "--production-api-port",
                    "18895",
                    "--budget-usd",
                    "0.788352",
                ])

    def test_integer_contract_accepts_only_json_int_or_canonical_decimal_string(self):
        for raw, expected in ((0, 0), (12, 12), ("0", 0), ("12", 12)):
            with self.subTest(raw=raw):
                self.assertEqual(run_live.as_int(raw, "value"), expected)
        for raw in (
            True,
            False,
            -1,
            0.0,
            1.0,
            1.5,
            "-1",
            "+1",
            "01",
            " 1",
            "1 ",
            "1.0",
            "",
            "１",
            None,
        ):
            with self.subTest(raw=raw), self.assertRaises(run_live.CalibrationError):
                run_live.as_int(raw, "value")

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

    def test_recent_turn_usage_and_nanousd_reject_noncanonical_numbers(self):
        for field in ("input_tokens", "api_input_nanousd"):
            for raw in (1.0, -1, "-1", "01", "+1", " 1", "1 ", "1.0", True):
                with self.subTest(field=field, raw=raw):
                    broken = event()
                    broken[field] = raw
                    with self.assertRaises(run_live.CalibrationError):
                        run_live.recent_turn_events(capacity([broken]))

    def test_matrix_covers_models_levels_stream_cache_audio_tool_search_long_and_image_sizes(self):
        models = [
            "gemini-3-flash-preview",
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
            self.assertIn(f"thinking:gemini-3-flash-preview:{level}", names)
            self.assertIn(f"thinking:gemini-3.6-flash:{level}", names)
        self.assertTrue(any(leg.stream for leg in legs if leg.model == "gemini-2.5-flash"))
        self.assertTrue(all(leg.max_output_tokens == 256 for leg in legs if leg.stream))
        self.assertTrue(any(leg.kind == "cache" and leg.cache_phase == "write" for leg in legs))
        self.assertTrue(any(
            leg.model == "gemini-3-flash-preview"
            and leg.kind == "cache"
            and leg.cache_phase == "prime"
            for leg in legs
        ))
        self.assertTrue(any(leg.kind == "cache" and leg.cache_phase == "read" for leg in legs))
        self.assertTrue(any(leg.kind == "audio" and leg.cache_phase == "write" for leg in legs))
        self.assertTrue(any(leg.kind == "audio" and leg.cache_phase == "read" for leg in legs))
        self.assertTrue(all(
            leg.max_output_tokens == 512
            for leg in legs
            if leg.model == "gemini-3-flash-preview" and leg.kind in {"cache", "audio"}
        ))
        self.assertTrue(all(
            leg.max_output_tokens == 128
            for leg in legs
            if leg.model != "gemini-3-flash-preview" and leg.kind in {"cache", "audio"}
        ))
        self.assertTrue(any(leg.kind == "tool" for leg in legs))
        self.assertTrue(any(leg.kind == "search" for leg in legs))
        self.assertTrue(any(leg.kind == "long" for leg in legs))
        image_legs = [leg for leg in legs if leg.kind == "image"]
        self.assertEqual(
            {leg.image_size for leg in image_legs},
            {"1K", "2K", "4K"},
        )
        self.assertTrue(all(leg.max_output_tokens == 4096 for leg in image_legs))
        self.assertTrue(all(
            run_live.body_for_leg(leg, "run")["generationConfig"]["responseModalities"]
            == ["TEXT", "IMAGE"]
            for leg in image_legs
        ))

    def test_cache_and_audio_replays_are_identical_per_profile_but_isolated_between_profiles(self):
        legs = run_live.build_coverage_legs(["gemini-2.5-flash"], "run")
        cache = [leg for leg in legs if leg.kind == "cache"]
        audio = [leg for leg in legs if leg.kind == "audio"]
        scopes = run_live.profile_cache_scopes(["profile-a", "profile-b"])
        scope_a = scopes["profile-a"]
        scope_b = scopes["profile-b"]
        cache_a = run_live.body_for_leg(cache[0], "run", scope_a)
        audio_a = run_live.body_for_leg(audio[0], "run", scope_a)
        self.assertTrue(all(
            cache_a == run_live.body_for_leg(leg, "run", scope_a)
            for leg in cache[1:]
        ))
        self.assertTrue(all(
            audio_a == run_live.body_for_leg(leg, "run", scope_a)
            for leg in audio[1:]
        ))
        self.assertNotEqual(cache_a, run_live.body_for_leg(cache[0], "run", scope_b))
        self.assertNotEqual(audio_a, run_live.body_for_leg(audio[0], "run", scope_b))
        self.assertNotIn("profile-a", json.dumps(cache_a))
        self.assertNotIn("profile-a", json.dumps(audio_a))

    def test_replay_pairs_are_profile_local_and_adjacent_in_the_execution_schedule(self):
        legs = run_live.build_coverage_legs(["gemini-3-flash-preview"], "run")
        schedule = run_live.coverage_schedule(legs, ["profile-a", "profile-b"])
        cache = [
            (profile, leg.cache_phase)
            for profile, leg in schedule
            if leg.kind == "cache"
        ]
        audio = [
            (profile, leg.cache_phase)
            for profile, leg in schedule
            if leg.kind == "audio"
        ]
        self.assertEqual(cache, [
            ("profile-a", "write"),
            ("profile-a", "prime"),
            ("profile-a", "read"),
            ("profile-b", "write"),
            ("profile-b", "prime"),
            ("profile-b", "read"),
        ])
        self.assertEqual(audio, [
            ("profile-a", "write"),
            ("profile-a", "read"),
            ("profile-b", "write"),
            ("profile-b", "read"),
        ])
        thinking = [
            (profile, leg.thinking_level)
            for profile, leg in schedule
            if leg.kind == "thinking"
        ]
        self.assertEqual(thinking[:4], [
            ("profile-a", "minimal"),
            ("profile-b", "minimal"),
            ("profile-a", "low"),
            ("profile-b", "low"),
        ])

    def test_flash_preview_two_plan_matrix_needs_a_twenty_four_dollar_cap(self):
        rates = run_live.ModelRates(
            tariff_schedule_id="google/gemini-developer-api/2026-08-02",
            input_token_limit=1_048_576,
            input=500,
            audio_input=1_000,
            cached_input=50,
            cached_audio_input=100,
            output=3_000,
            image_output=0,
            long_threshold=(1 << 64) - 1,
            long_input=500,
            long_audio_input=1_000,
            long_cached_input=50,
            long_cached_audio_input=100,
            long_output=3_000,
            search_unit="query",
            search=14_000_000,
            max_output_tokens=65_536,
        )
        dispatchable = [
            leg
            for leg in run_live.build_coverage_legs(
                ["gemini-3-flash-preview"],
                "run",
                {"gemini-3-flash-preview": rates},
            )
            if leg.kind != "search"
        ]
        aggregate = 2 * sum(
            rates.upper_bound(1, leg.max_output_tokens, leg.kind, leg.image_size)
            for leg in dispatchable
        )
        self.assertEqual(aggregate, 23_099_392_000)
        self.assertGreater(aggregate, 23 * run_live.NANO_PER_USD)
        self.assertLessEqual(aggregate, 24 * run_live.NANO_PER_USD)

    def test_tool_leg_forces_the_declared_function_instead_of_accepting_plain_text(self):
        leg = run_live.Leg("tool", "gemini-3-flash-preview", "tool")
        body = run_live.body_for_leg(leg, "run")
        self.assertEqual(
            body["toolConfig"]["functionCallingConfig"],
            {
                "mode": "ANY",
                "allowedFunctionNames": ["calibration_probe"],
            },
        )
        self.assertIn("Call calibration_probe", body["contents"][0]["parts"][0]["text"])

    def test_non_stream_response_proves_public_identity_visible_output_terminal_usage_and_event_match(self):
        leg = run_live.Leg("fresh", "gemini-3-flash-preview", "fresh")
        response = run_live.decode_generation_response(
            json.dumps({
                "modelVersion": "gemini-3-flash-preview",
                "candidates": [{
                    "content": {"parts": [
                        {"text": "private reasoning", "thought": True},
                        {"text": "CALIBRATION_OK"},
                    ]},
                    "finishReason": "STOP",
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 2,
                    "thoughtsTokenCount": 3,
                },
            }).encode(),
            stream=False,
        )
        immutable = event(model=leg.model)
        immutable.update({
            "input_tokens": 10,
            "output_tokens": 5,
            "thinking_output_tokens": 3,
        })
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertEqual(evidence["model_version"], leg.model)
        self.assertGreater(evidence["visible_text_chars"], 0)
        self.assertTrue(evidence["terminal_finish"])
        self.assertTrue(evidence["terminal_usage"])
        self.assertTrue(evidence["usage_matches_immutable_event"])

    def test_response_proof_rejects_private_identity_thoughts_only_or_usage_mismatch(self):
        leg = run_live.Leg("thinking", "gemini-3-flash-preview", "thinking", "low")
        base = {
            "modelVersion": leg.model,
            "candidates": [{
                "content": {"parts": [{"text": "reasoning only", "thought": True}]},
                "finishReason": "STOP",
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 0,
                "thoughtsTokenCount": 3,
            },
        }
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 10, "output_tokens": 3, "thinking_output_tokens": 3})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]

        response = run_live.decode_generation_response(json.dumps(base).encode(), stream=False)
        _evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIn("visible non-thought text", error)

        wrong_model = dict(base)
        wrong_model["modelVersion"] = "gemini-3-flash"
        response = run_live.decode_generation_response(
            json.dumps(wrong_model).encode(), stream=False
        )
        _evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIn("modelVersion proof", error)

        visible = dict(base)
        visible["candidates"] = [{
            "content": {"parts": [{"text": "CALIBRATION_OK"}]},
            "finishReason": "STOP",
        }]
        visible["usageMetadata"] = {
            **base["usageMetadata"],
            "candidatesTokenCount": 1,
        }
        response = run_live.decode_generation_response(json.dumps(visible).encode(), stream=False)
        mismatched = dict(immutable)
        mismatched["output_tokens"] = 5
        _evidence, error = run_live.verify_generation_response(leg, response, mismatched)
        self.assertIn("does not match immutable event", error)

    def test_sse_response_requires_multiple_candidate_frames_and_terminal_usage(self):
        leg = run_live.Leg(
            "sse:gemini-3-flash-preview",
            "gemini-3-flash-preview",
            "fresh",
            stream=True,
            max_output_tokens=256,
        )
        first = {
            "modelVersion": leg.model,
            "candidates": [{"content": {"parts": [{"text": "CALIBRATION_"}]}}],
        }
        terminal = {
            "candidates": [{
                "content": {"parts": [{"text": "OK"}]},
                "finishReason": "STOP",
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2},
        }
        raw = (
            f"data: {json.dumps(first, separators=(',', ':'))}\n\n"
            f"data: {json.dumps(terminal, separators=(',', ':'))}\n\n"
        ).encode()
        response = run_live.decode_generation_response(raw, stream=True)
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 10, "output_tokens": 2})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertEqual(evidence["response_frames"], 2)
        self.assertEqual(evidence["candidate_frames"], 2)
        self.assertEqual(evidence["visible_text_frames"], 2)
        self.assertTrue(evidence["incremental_sse"])
        self.assertTrue(evidence["terminal_usage"])

        one_frame = run_live.decode_generation_response(
            json.dumps({**terminal, "modelVersion": leg.model}).encode(), stream=True
        )
        self.assertIsNotNone(one_frame.parse_error)
        self.assertIn("SSE", one_frame.parse_error)

        usage_before_end = run_live.GenerationResponse(
            frames=(
                {
                    "modelVersion": leg.model,
                    "candidates": [{"content": {"parts": [{"text": "early"}]}}],
                    "usageMetadata": terminal["usageMetadata"],
                },
                {
                    "candidates": [{
                        "content": {"parts": [{"text": "late"}]},
                        "finishReason": "STOP",
                    }]
                },
            ),
            stream=True,
        )
        _evidence, error = run_live.verify_generation_response(
            leg, usage_before_end, immutable
        )
        self.assertIn("terminal usageMetadata", error)

    def test_gemini_37_admission_accepts_only_the_confirmed_tiered_wire_alias(self):
        leg = run_live.Leg(
            "admission:gemini-3.7-flash:default-sse",
            "gemini-3.7-flash",
            "fresh",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        expected = run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT
        split_at = expected.index(" 33 ") + 1
        response = run_live.GenerationResponse(
            frames=(
                {
                    "modelVersion": "gemini-3.7-flash-tiered",
                    "candidates": [{"content": {"parts": [{"text": expected[:split_at]}]}}],
                },
                {
                    "candidates": [{
                        "content": {"parts": [{"text": expected[split_at:]}]},
                        "finishReason": "STOP",
                    }],
                    "usageMetadata": {
                        "promptTokenCount": 20,
                        "candidatesTokenCount": 182,
                        "thoughtsTokenCount": 296,
                    },
                },
            ),
            stream=True,
        )
        immutable = event(model=leg.model)
        immutable.update({
            "input_tokens": 20,
            "output_tokens": 478,
            "thinking_output_tokens": 296,
        })
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]

        evidence, error = run_live.verify_generation_response(leg, response, immutable)

        self.assertIsNone(error)
        self.assertEqual(evidence["model_version"], "gemini-3.7-flash")
        self.assertEqual(
            evidence["upstream_model_version"], "gemini-3.7-flash-tiered"
        )
        self.assertTrue(evidence["terminal_finish"])
        self.assertTrue(evidence["terminal_usage"])
        self.assertTrue(evidence["incremental_sse"])
        self.assertTrue(evidence["usage_matches_immutable_event"])

        unconfirmed = dataclasses.replace(
            response,
            frames=(
                {**response.frames[0], "modelVersion": "gemini-3.7-flash-preview"},
                response.frames[1],
            ),
        )
        _evidence, error = run_live.verify_generation_response(leg, unconfirmed, immutable)
        self.assertIn("modelVersion proof", error)

        ordinary_leg = dataclasses.replace(leg, name="sse:gemini-3.7-flash")
        _evidence, error = run_live.verify_generation_response(
            ordinary_leg, response, immutable
        )
        self.assertIn("modelVersion proof", error)

    def test_plain_text_sse_rejects_nonvisible_preterminal_candidate_frames(self):
        leg = run_live.Leg(
            "sse:gemini-3-flash-preview",
            "gemini-3-flash-preview",
            "fresh",
            stream=True,
            max_output_tokens=256,
        )
        terminal = {
            "candidates": [{
                "content": {"parts": [{"text": "OK"}]},
                "finishReason": "STOP",
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2},
        }
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 10, "output_tokens": 2})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]

        preterminal_candidates = {
            "empty": [{}],
            "thought-only": [{
                "content": {
                    "parts": [{"text": "private reasoning", "thought": True}]
                }
            }],
        }
        for name, candidates in preterminal_candidates.items():
            response = run_live.GenerationResponse(
                frames=(
                    {"modelVersion": leg.model, "candidates": candidates},
                    terminal,
                ),
                stream=True,
            )
            with self.subTest(preterminal=name):
                evidence, error = run_live.verify_generation_response(
                    leg, response, immutable
                )
                self.assertFalse(evidence["incremental_sse"])
                self.assertEqual(evidence["candidate_frames"], 2)
                self.assertEqual(evidence["visible_text_frames"], 1)
                self.assertIn("visible non-thought text", error)

    def test_plain_text_sse_rejects_blocked_mixed_or_inconsistent_response(self):
        leg = run_live.Leg(
            "sse:gemini-3-flash-preview",
            "gemini-3-flash-preview",
            "fresh",
            stream=True,
            max_output_tokens=256,
        )
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 10, "output_tokens": 2})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]

        def frames():
            return [
                {
                    "responseId": "response-a",
                    "modelVersion": leg.model,
                    "candidates": [
                        {"index": 0, "content": {"parts": [{"text": "CALIBRATION_"}]}}
                    ],
                },
                {
                    "responseId": "response-a",
                    "candidates": [
                        {
                            "index": 0,
                            "content": {"parts": [{"text": "OK"}]},
                            "finishReason": "STOP",
                        }
                    ],
                    "usageMetadata": {
                        "promptTokenCount": 10,
                        "candidatesTokenCount": 2,
                    },
                },
            ]

        cases = {}
        blocked = frames()
        blocked[1]["promptFeedback"] = {"blockReason": "SAFETY"}
        cases["blocked"] = blocked
        tool = frames()
        tool[1]["candidates"][0]["content"]["parts"].append(
            {"functionCall": {"name": "unexpected", "args": {}}}
        )
        cases["function-call"] = tool
        image = frames()
        image[1]["candidates"][0]["content"]["parts"].append(
            {"inlineData": {"mimeType": "image/png", "data": "AA=="}}
        )
        cases["inline-data"] = image
        for key in (
            "functionCall",
            "functionResponse",
            "inlineData",
            "fileData",
            "executableCode",
            "codeExecutionResult",
        ):
            empty_payload = frames()
            empty_payload[1]["candidates"][0]["content"]["parts"].append({key: {}})
            cases[f"empty-{key}"] = empty_payload
        multiple = frames()
        multiple[0]["candidates"].append({})
        cases["multiple-candidates"] = multiple
        wrong_index = frames()
        wrong_index[0]["candidates"][0]["index"] = 1
        cases["candidate-index"] = wrong_index
        changed_id = frames()
        changed_id[1]["responseId"] = "response-b"
        cases["response-id"] = changed_id

        for name, response_frames in cases.items():
            with self.subTest(name=name):
                _evidence, error = run_live.verify_generation_response(
                    leg,
                    run_live.GenerationResponse(tuple(response_frames), stream=True),
                    immutable,
                )
                self.assertIsNotNone(error)

    def test_visible_text_requires_positive_candidate_token_evidence(self):
        leg = run_live.Leg(
            "sse:gemini-3-flash-preview",
            "gemini-3-flash-preview",
            "fresh",
            stream=True,
            max_output_tokens=256,
        )

        def response(candidates_token_count):
            usage = {"promptTokenCount": 10, "thoughtsTokenCount": 1}
            if candidates_token_count is not None:
                usage["candidatesTokenCount"] = candidates_token_count
            return run_live.GenerationResponse(
                frames=(
                    {
                        "modelVersion": leg.model,
                        "candidates": [
                            {"content": {"parts": [{"text": "CALIBRATION_"}]}}
                        ],
                    },
                    {
                        "candidates": [
                            {
                                "content": {"parts": [{"text": "OK"}]},
                                "finishReason": "STOP",
                            }
                        ],
                        "usageMetadata": usage,
                    },
                ),
                stream=True,
            )

        immutable = event(model=leg.model)
        immutable.update(
            {"input_tokens": 10, "output_tokens": 1, "thinking_output_tokens": 1}
        )
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        for candidate_count in (None, 0):
            with self.subTest(candidate_count=candidate_count):
                _evidence, error = run_live.verify_generation_response(
                    leg,
                    response(candidate_count),
                    immutable,
                )
                self.assertIn("candidatesTokenCount", error)

        _evidence, error = run_live.verify_generation_response(
            leg,
            response(1),
            immutable,
        )
        self.assertIn("no billed non-thinking candidate tokens", error)

    def test_stream_decoder_rejects_raw_json_and_malformed_sse_framing(self):
        raw_json_values = (
            b'{"modelVersion":"gemini-3.7-flash"}',
            b'[{"modelVersion":"gemini-3.7-flash"}]',
        )
        for raw in raw_json_values:
            with self.subTest(raw=raw):
                decoded = run_live.decode_generation_response(raw, stream=True)
                self.assertEqual(decoded.frames, ())
                self.assertIsNotNone(decoded.parse_error)

        malformed = (
            b'data: {"modelVersion":"gemini-3.7-flash"}\n',
            b'data: {"modelVersion":"gemini-3.7-flash"}\r\r',
            b'database: {"modelVersion":"gemini-3.7-flash"}\n\n',
            b'event: error\ndata: {"modelVersion":"gemini-3.7-flash"}\n\n',
            b'data: {"modelVersion":"gemini-3.7-flash","modelVersion":"spoof"}\n\n',
            b'data: {"value":NaN}\n\n',
            b'data: [1]\n\n',
        )
        for raw in malformed:
            with self.subTest(raw=raw):
                decoded = run_live.decode_generation_response(raw, stream=True)
                self.assertEqual(decoded.frames, ())
                self.assertIsNotNone(decoded.parse_error)

        crlf = run_live.decode_generation_response(
            b'data: {"modelVersion":"gemini-3.7-flash"}\r\n\r\n',
            stream=True,
        )
        self.assertIsNone(crlf.parse_error)
        self.assertEqual(len(crlf.frames), 1)

    def test_generation_requires_stop_in_the_final_frame(self):
        leg = run_live.Leg("fresh", "gemini-3-flash-preview", "fresh")
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": "10", "output_tokens": "2"})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]

        for reason in ("SAFETY", "MAX_TOKENS", "MALFORMED_FUNCTION_CALL", None):
            candidate = {"content": {"parts": [{"text": "OK"}]}}
            if reason is not None:
                candidate["finishReason"] = reason
            response = run_live.GenerationResponse(
                frames=({
                    "modelVersion": leg.model,
                    "candidates": [candidate],
                    "usageMetadata": {
                        "promptTokenCount": "10",
                        "candidatesTokenCount": "2",
                    },
                },),
                stream=False,
            )
            with self.subTest(reason=reason):
                evidence, error = run_live.verify_generation_response(
                    leg, response, immutable
                )
                self.assertFalse(evidence["terminal_finish"])
                self.assertIsNotNone(error)
                self.assertIn("STOP", error)

        response = run_live.GenerationResponse(
            frames=(
                {
                    "modelVersion": leg.model,
                    "candidates": [{
                        "content": {"parts": [{"text": "O"}]},
                        "finishReason": "STOP",
                    }],
                },
                {
                    "candidates": [{"content": {"parts": [{"text": "K"}]}}],
                    "usageMetadata": {
                        "promptTokenCount": 10,
                        "candidatesTokenCount": 2,
                    },
                },
            ),
            stream=True,
        )
        stream_leg = dataclasses.replace(leg, stream=True)
        evidence, error = run_live.verify_generation_response(
            stream_leg, response, immutable
        )
        self.assertFalse(evidence["terminal_finish"])
        self.assertIn("terminal STOP", error)

    def test_response_usage_accepts_canonical_decimal_strings_but_never_coercions(self):
        self.assertEqual(run_live._response_int("12", "tokens"), 12)
        for raw in (12.0, -1, "-1", "01", "+1", " 1", "1.0", True):
            with self.subTest(raw=raw), self.assertRaises(run_live.CalibrationError):
                run_live._response_int(raw, "tokens")

    def test_tool_response_requires_function_call_and_accepts_optional_subset_usage(self):
        leg = run_live.Leg("tool", "gemini-3-flash-preview", "tool")
        response = run_live.GenerationResponse(
            frames=({
                "modelVersion": leg.model,
                "candidates": [{
                    "content": {"parts": [{
                        "functionCall": {
                            "name": "calibration_probe",
                            "args": {"marker": "CALIBRATION_OK"},
                        }
                    }]},
                    "finishReason": "STOP",
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "toolUsePromptTokenCount": 3,
                    "candidatesTokenCount": 2,
                },
            },),
            stream=False,
        )
        immutable = event(model=leg.model)
        immutable.update({"input_tokens": 13, "tool_prompt_tokens": 3, "output_tokens": 2})
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        evidence, error = run_live.verify_generation_response(leg, response, immutable)
        self.assertIsNone(error)
        self.assertEqual(evidence["function_calls"], 1)

        response_without_subset = dataclasses.replace(
            response,
            frames=({
                **response.frames[0],
                "usageMetadata": {
                    "promptTokenCount": 65,
                    "candidatesTokenCount": 2,
                },
            },),
        )
        immutable_without_subset = event(model=leg.model)
        immutable_without_subset.update({
            "input_tokens": 65,
            "tool_prompt_tokens": 0,
            "output_tokens": 2,
        })
        immutable_without_subset = run_live.recent_turn_events(
            capacity([immutable_without_subset])
        )["req-1"]
        evidence, error = run_live.verify_generation_response(
            leg,
            response_without_subset,
            immutable_without_subset,
        )
        self.assertIsNone(error)
        self.assertEqual(evidence["function_calls"], 1)
        self.assertTrue(evidence["usage_matches_immutable_event"])
        self.assertIsNone(run_live.verify_leg_usage(leg, immutable_without_subset))

    def test_malformed_success_body_becomes_sanitized_response_proof_failure(self):
        response = run_live.decode_generation_response(b"data: {not-json}\n\n", stream=True)
        self.assertEqual(response.frames, ())
        self.assertIn("invalid JSON", response.parse_error)

    def test_usage_verification_distinguishes_billable_classes_and_optional_tool_subset(self):
        raw = event()
        raw["output_tokens"] = "2"
        parsed = run_live.recent_turn_events(capacity([raw]))["req-1"]
        audio = run_live.Leg("audio", "gemini-2.5-flash", "audio")
        self.assertIn("audio", run_live.verify_leg_usage(audio, parsed))
        parsed["audio_input_tokens"] = 5
        parsed["output_tokens"] = 2
        self.assertIsNone(run_live.verify_leg_usage(audio, parsed))
        tool = run_live.Leg("tool", "gemini-2.5-flash", "tool")
        self.assertIsNone(run_live.verify_leg_usage(tool, parsed))
        parsed["tool_prompt_tokens"] = 1
        self.assertIsNone(run_live.verify_leg_usage(tool, parsed))
        cache_prime = run_live.Leg(
            "cache-prime",
            "gemini-3-flash-preview",
            "cache",
            cache_phase="prime",
        )
        cache_read = dataclasses.replace(
            cache_prime,
            name="cache-read",
            cache_phase="read",
        )
        self.assertIsNone(run_live.verify_leg_usage(cache_prime, parsed))
        self.assertIn("cached input", run_live.verify_leg_usage(cache_read, parsed))
        parsed["cache_read_tokens"] = 1
        self.assertIsNone(run_live.verify_leg_usage(cache_read, parsed))

    def test_minimal_allows_zero_thinking_tokens_but_higher_levels_remain_strict(self):
        raw = event(model="gemini-3-flash-preview")
        raw["output_tokens"] = "2"
        parsed = run_live.recent_turn_events(capacity([raw]))["req-1"]

        minimal = run_live.Leg(
            "thinking:gemini-3-flash-preview:minimal",
            "gemini-3-flash-preview",
            "thinking",
            "minimal",
        )
        self.assertIsNone(run_live.verify_leg_usage(minimal, parsed))

        for level in ("low", "medium", "high"):
            with self.subTest(level=level):
                leg = dataclasses.replace(
                    minimal,
                    name=f"thinking:gemini-3-flash-preview:{level}",
                    thinking_level=level,
                )
                self.assertEqual(
                    run_live.verify_leg_usage(leg, parsed),
                    run_live.THINKING_TOKENS_NOT_OBSERVED,
                )

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

    def test_production_paths_can_target_an_isolated_loopback_canary(self):
        command = run_live.remote_capacity_command("deploy@84.32.48.2", 18895)
        self.assertIn("ssh deploy@84.32.48.2", command)
        self.assertIn("127.0.0.1:18895/gemini-subs", command)

        client = run_live.ProductionSshJsonHttpClient(
            timeout=10,
            ssh_target="deploy@84.32.48.2",
            api_port=18895,
        )
        succeeded = subprocess.CompletedProcess(
            [], 0, stdout=b'{"totalTokens":10}\n__CALIBRATION_HTTP__200\n', stderr=b""
        )
        with mock.patch.object(run_live.subprocess, "run", return_value=succeeded) as invoked:
            result = client.request(
                "/v1beta/models/gemini-3-flash-preview:countTokens",
                "POST",
                {"contents": []},
                "profile-a",
            )
        self.assertEqual(result, {"totalTokens": 10})
        self.assertEqual(invoked.call_args.args[0][:2], ["ssh", "deploy@84.32.48.2"])
        self.assertIn("127.0.0.1:18895", invoked.call_args.args[0][2])

    def test_production_capacity_and_billable_api_ports_are_independent(self):
        args = run_live.parse_args([
            "--production-capacity-over-ssh",
            "--production-api-over-ssh",
            "--production-capacity-port",
            "8794",
            "--production-api-port",
            "8802",
        ])
        self.assertEqual(args.production_capacity_port, 8794)
        self.assertEqual(args.production_api_port, 8802)
        self.assertIn(
            "127.0.0.1:8794/gemini-subs",
            run_live.remote_capacity_command(api_port=args.production_capacity_port),
        )
        self.assertEqual(
            run_live.ProductionSshJsonHttpClient(
                timeout=10,
                api_port=args.production_api_port,
            ).api_port,
            8802,
        )

    def test_production_canary_target_rejects_ssh_options_and_invalid_ports(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.ProductionSshJsonHttpClient(timeout=10, ssh_target="-oProxyCommand=bad")
        for port in (0, 65_536):
            with self.assertRaises(run_live.CalibrationError):
                run_live.remote_capacity_command(api_port=port)
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--production-capacity-port", "0"])

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

    def test_gemini_37_count_is_attested_and_never_retried(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        failed = subprocess.CompletedProcess([], 255, stdout=b"", stderr=b"ambiguous")
        with mock.patch.object(run_live.subprocess, "run", return_value=failed) as invoked:
            with self.assertRaises(run_live.CalibrationError):
                client.request(
                    "/v1beta/models/gemini-3.7-flash:countTokens",
                    "POST",
                    {"contents": []},
                    "profile-a",
                    calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                    calibration_not_after=2_000_000_000,
                    capture_dispatch=True,
                    allow_safe_retry=False,
                )
        self.assertEqual(invoked.call_count, 1)

        succeeded = subprocess.CompletedProcess(
            [],
            0,
            stdout=(
                b'{"totalTokens":10}\n__CALIBRATION_HTTP__200\n\n'
                b"1999999999000"
            ),
            stderr=b"",
        )
        with mock.patch.object(run_live.subprocess, "run", return_value=succeeded) as invoked:
            response = client.request(
                "/v1beta/models/gemini-3.7-flash:countTokens",
                "POST",
                {"contents": []},
                "profile-a",
                calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                calibration_not_after=2_000_000_000,
                capture_dispatch=True,
                allow_safe_retry=False,
            )
        self.assertEqual(response.payload, {"totalTokens": 10})
        self.assertEqual(response.dispatch_ms, 1_999_999_999_000)
        remote = invoked.call_args.args[0][2]
        self.assertIn("x-apitoken-calibration-not-after: 2000000000", remote)
        self.assertIn("x-apitoken-calibration-dispatch-ms", remote)

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

    def test_resume_reclassifies_only_the_completed_minimal_zero_thinking_false_negative(self):
        payload = minimal_zero_thinking_false_negative_report()
        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with open(report, "w", encoding="utf-8") as report_file:
                json.dump(payload, report_file)
            state = run_live.load_resume_report(
                report,
                21_000_000_000,
                ["gemini-3-flash-preview"],
            )

        self.assertEqual(state.spent_nano, 556_500)
        self.assertEqual(state.spent_by_profile, {"profile-a": 556_500})
        self.assertEqual(len(state.records), 1)
        self.assertEqual(state.records[0]["coverage_error"], None)
        self.assertTrue(
            state.records[0]["response_evidence"]["minimal_zero_thinking_accepted"]
        )
        self.assertEqual(state.unavailable, [])
        self.assertEqual(
            {(record["profile_id"], record["leg"]) for record in state.records},
            {("profile-a", "thinking:gemini-3-flash-preview:minimal")},
        )

    def test_resume_rejects_tampered_minimal_zero_thinking_evidence(self):
        cases = []

        payload = minimal_zero_thinking_false_negative_report()
        payload["failure"] = "different failure"
        cases.append(("failure", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0].pop("response_evidence")
        cases.append(("missing response evidence", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0]["response_evidence"]["model_version"] = "gemini-3-flash"
        cases.append(("private model identity", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["unavailable_capabilities"].append({
            "profile_id": "profile-a",
            "model": "gemini-3-flash-preview",
            "capability": "thinking:gemini-3-flash-preview:low",
            "reason": run_live.THINKING_TOKENS_NOT_OBSERVED,
            "blocking": True,
        })
        cases.append(("multiple unavailable outcomes", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0]["thinking_level"] = "low"
        cases.append(("wrong thinking level", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0]["model"] = "gemini-3-flash"
        cases.append(("wrong record model", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["models"] = ["gemini-3.6-flash"]
        payload["records"][0]["model"] = "gemini-3.6-flash"
        payload["records"][0]["response_evidence"]["model_version"] = "gemini-3.6-flash"
        payload["unavailable_capabilities"][0]["model"] = "gemini-3.6-flash"
        payload["blocking_unavailable_capabilities"][0]["model"] = "gemini-3.6-flash"
        payload["pending_legs"][0]["model"] = "gemini-3.6-flash"
        cases.append(("different model", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0]["leg"] = "thinking:gemini-3-flash-preview:other"
        payload["unavailable_capabilities"][0]["capability"] = (
            "thinking:gemini-3-flash-preview:other"
        )
        payload["blocking_unavailable_capabilities"][0]["capability"] = (
            "thinking:gemini-3-flash-preview:other"
        )
        cases.append(("wrong capability", payload))

        payload = minimal_zero_thinking_false_negative_report()
        payload["records"][0]["stream"] = True
        cases.append(("streaming record", payload))

        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            for name, payload in cases:
                with self.subTest(name=name), open(report, "w", encoding="utf-8") as report_file:
                    json.dump(payload, report_file)
                with self.assertRaises(run_live.CalibrationError):
                    run_live.load_resume_report(
                        report,
                        21_000_000_000,
                        ["gemini-3-flash-preview"],
                    )

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

    def test_paid_response_proof_failure_is_terminal_and_stops_the_remaining_matrix(self):
        conversion_model = {
            "id": "gemini-3-flash-preview",
            "tariff_schedule_id": "google/test/v1",
            "input_token_limit": "1048576",
            "output_token_limit": "65536",
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
            "search": {"billing_unit": "query", "nanousd_per_unit": "1"},
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
        calls = []

        def execute(runner, leg, profile):
            calls.append((profile, leg.name))
            record = report_record(
                request_id="req-response-miss",
                profile=profile,
                model=leg.model,
                leg=leg.name,
            )
            record["coverage_error"] = "generation returned no visible non-thought text"
            record["response_evidence"] = {"visible_text_chars": 0}
            runner.budget.charge(profile, 100, 1_000)
            runner.records.append(record)
            return record

        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
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
                        "--models",
                        "gemini-3-flash-preview",
                        "--capacity-command",
                        "unused",
                        "--report",
                        report,
                    ])
            with open(report, encoding="utf-8") as report_file:
                payload = json.load(report_file)
        self.assertEqual(len(calls), 1)
        self.assertFalse(payload["complete"])
        self.assertFalse(payload["resume_safe"])
        self.assertEqual(payload["spent_nanousd_total"], "100")
        self.assertEqual(len(payload["blocking_unavailable_capabilities"]), 1)
        self.assertIn("paid response proof failed", payload["failure"])

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
                return run_live.GenerationResponse(
                    frames=({
                        "modelVersion": "gemini-2.5-flash",
                        "candidates": [{
                            "content": {"parts": [{"text": "CALIBRATION_OK"}]},
                            "finishReason": "STOP",
                        }],
                        "usageMetadata": {
                            "promptTokenCount": 10,
                            "candidatesTokenCount": 1,
                        },
                    },),
                    stream=False,
                )

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
            cache_scopes={"profile-a": "profile-1"},
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

    def test_gemini_37_runner_sends_exactly_one_count_and_one_paid_sse(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL

        class FakeApi:
            def __init__(self):
                self.calls = []
                self.generation_request_id = None

            def request(self, path, method="GET", body=None, target_profile=None, **options):
                self.calls.append((path, target_profile, dict(options)))
                if path.endswith(":countTokens"):
                    return run_live.JsonResponse(
                        {"totalTokens": 10},
                        1_000_100,
                    )
                self.generation_request_id = options["calibration_request_id"]
                split = len(run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT) // 2
                return run_live.GenerationResponse(
                    frames=(
                        {
                            "modelVersion": model,
                            "candidates": [{
                                "content": {"parts": [{
                                    "text": run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT[:split]
                                }]},
                            }],
                        },
                        {
                            "candidates": [{
                                "content": {"parts": [{
                                    "text": run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT[split:]
                                }]},
                                "finishReason": "STOP",
                            }],
                            "usageMetadata": {
                                "promptTokenCount": 10,
                                "candidatesTokenCount": 2,
                            },
                        },
                    ),
                    stream=True,
                    dispatch_ms=1_000_200,
                )

        class FakeCapacity:
            def __init__(self, api):
                self.api = api

            def read(self):
                events = []
                if self.api.generation_request_id:
                    turn = event(
                        self.api.generation_request_id,
                        profile="profile-a",
                        model=model,
                    )
                    turn.update({"output_tokens": "2", "completed_at": "100"})
                    events = [turn]
                payload = capacity(events)
                payload["profiles"] = [{
                    "id": "profile-a",
                    "plan": "google_ai_pro",
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "quota_updated_at": 101 if events else 99,
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
        upper = rates.upper_bound(
            10,
            run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
            "fresh",
        )
        api = FakeApi()
        budget = run_live.Budget(upper)
        runner = run_live.Runner(
            api,
            FakeCapacity(api),
            {model: rates},
            budget,
            timeout=1,
            delay=0,
            run_id="run",
            cache_scopes={"profile-a": "profile-1"},
            admission=run_live.Gemini37Admission("profile-a", "a" * 40),
        )
        leg = run_live.Leg(
            f"admission:{model}:default-sse",
            model,
            "fresh",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        with mock.patch.object(run_live.time, "time", return_value=1_000), mock.patch.object(
            run_live.time, "sleep", return_value=None
        ):
            record = runner.execute_leg(leg, "profile-a")

        self.assertEqual(len(api.calls), 2)
        count_call, generation_call = api.calls
        self.assertTrue(count_call[0].endswith(":countTokens"))
        self.assertFalse(count_call[2]["allow_safe_retry"])
        self.assertFalse(generation_call[2]["allow_safe_retry"])
        self.assertEqual(count_call[2]["calibration_not_after"], 1_600)
        self.assertEqual(generation_call[2]["calibration_not_after"], 1_600)
        self.assertNotEqual(
            count_call[2]["calibration_request_id"],
            generation_call[2]["calibration_request_id"],
        )
        self.assertTrue(record["response_evidence"]["incremental_sse"])
        self.assertEqual(record["admission"]["implementation_sha"], "a" * 40)
        self.assertEqual(record["admission"]["count_dispatch_ms"], "1000100")
        self.assertEqual(record["admission"]["generation_dispatch_ms"], "1000200")
        self.assertEqual(
            [attempt["kind"] for attempt in runner.admission_attempts],
            ["countTokens", "paid_generation"],
        )
        self.assertEqual(runner.admission_attempts[0]["input_tokens"], "10")
        self.assertEqual(
            runner.admission_attempts[1]["outcome"],
            "immutable_event_reconciled",
        )
        self.assertEqual(budget.total_nano, 100)

    def test_gemini_37_budget_mismatch_stops_after_the_free_count(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL

        class CountOnlyApi:
            def __init__(self):
                self.calls = 0

            def request(self, path, method="GET", body=None, target_profile=None, **options):
                self.calls += 1
                self.assert_count = path.endswith(":countTokens")
                return run_live.JsonResponse({"totalTokens": 10}, 1_000_100)

        class StaticCapacity:
            def read(self):
                payload = capacity()
                payload["profiles"] = [{
                    "id": "profile-a",
                    "plan": "google_ai_pro",
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "quota_updated_at": 99,
                    "windows": [],
                }]
                return payload

        rates = run_live.ModelRates(
            "google/test/v1", 1_000, 10, 10, 1, 1, 10, 0,
            1_000, 10, 10, 1, 1, 10, "prompt", 1, 1_000,
        )
        upper = rates.upper_bound(
            10,
            run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
            "fresh",
        )
        api = CountOnlyApi()
        runner = run_live.Runner(
            api,
            StaticCapacity(),
            {model: rates},
            run_live.Budget(upper + 1),
            timeout=1,
            delay=0,
            run_id="run",
            cache_scopes={"profile-a": "profile-1"},
            admission=run_live.Gemini37Admission("profile-a", "a" * 40),
        )
        leg = run_live.Leg(
            f"admission:{model}:default-sse",
            model,
            "fresh",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        with mock.patch.object(run_live.time, "time", return_value=1_000):
            with self.assertRaises(run_live.CalibrationError) as caught:
                runner.execute_leg(leg, "profile-a")
        self.assertIn("must equal the worst-case exact current-tariff ceiling", str(caught.exception))
        self.assertEqual(api.calls, 1)
        self.assertTrue(api.assert_count)

    def test_gemini_37_non_positive_count_stops_before_generation(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL

        class ZeroCountApi:
            def __init__(self):
                self.calls = 0

            def request(self, path, method="GET", body=None, target_profile=None, **options):
                self.calls += 1
                return run_live.JsonResponse({"totalTokens": 0}, 1_000_100)

        class StaticCapacity:
            def read(self):
                payload = capacity()
                payload["profiles"] = [{
                    "id": "profile-a",
                    "plan": "google_ai_pro",
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
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
        api = ZeroCountApi()
        runner = run_live.Runner(
            api,
            StaticCapacity(),
            {model: rates},
            run_live.Budget(12_560),
            timeout=1,
            delay=0,
            run_id="run",
            cache_scopes={"profile-a": "profile-1"},
            admission=run_live.Gemini37Admission("profile-a", "a" * 40),
        )
        leg = run_live.Leg(
            f"admission:{model}:default-sse",
            model,
            "fresh",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        with mock.patch.object(run_live.time, "time", return_value=1_000), self.assertRaisesRegex(
            run_live.CalibrationError,
            "non-positive totalTokens",
        ):
            runner.execute_leg(leg, "profile-a")

        self.assertEqual(api.calls, 1)
        self.assertEqual(runner.admission_attempts[0]["outcome"], "terminal_failure")

    def test_gemini_37_thinking_level_leg_sends_thinking_config_and_requires_thoughts(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL

        class FakeApi:
            def __init__(self, case):
                self.case = case
                self.calls = []
                self.generation_request_id = None

            def request(self, path, method="GET", body=None, target_profile=None, **options):
                self.calls.append((path, body, dict(options)))
                if path.endswith(":countTokens"):
                    return run_live.JsonResponse({"totalTokens": 10}, 1_000_100)
                self.generation_request_id = options["calibration_request_id"]
                self.case.assertEqual(
                    body["generationConfig"]["thinkingConfig"],
                    {"thinkingLevel": "high"},
                )
                split = len(run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT) // 2
                return run_live.GenerationResponse(
                    frames=(
                        {
                            "modelVersion": "gemini-3.7-flash-tiered",
                            "candidates": [{
                                "content": {"parts": [{
                                    "text": run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT[:split]
                                }]},
                            }],
                        },
                        {
                            "candidates": [{
                                "content": {"parts": [{
                                    "text": run_live.GEMINI_37_ADMISSION_EXPECTED_TEXT[split:]
                                }]},
                                "finishReason": "STOP",
                            }],
                            "usageMetadata": {
                                "promptTokenCount": 10,
                                "candidatesTokenCount": 3,
                                "thoughtsTokenCount": 7,
                            },
                        },
                    ),
                    stream=True,
                    dispatch_ms=1_000_200,
                )

        class FakeCapacity:
            def __init__(self, api):
                self.api = api

            def read(self):
                events = []
                if self.api.generation_request_id:
                    turn = event(
                        self.api.generation_request_id,
                        profile="profile-a",
                        model=model,
                    )
                    turn.update({
                        "input_tokens": "10",
                        "output_tokens": "10",
                        "thinking_output_tokens": "7",
                        "completed_at": "100",
                    })
                    events = [turn]
                payload = capacity(events)
                payload["profiles"] = [{
                    "id": "profile-a",
                    "plan": "google_ai_ultra",
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "quota_updated_at": 101 if events else 99,
                    "windows": [],
                }]
                return payload

        rates = run_live.ModelRates(
            "google/test/v1", 1_000, 10, 10, 1, 1, 10, 0,
            1_000, 10, 10, 1, 1, 10, "prompt", 1, 1_000,
        )
        upper = rates.upper_bound(
            10,
            run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
            "fresh",
        )
        api = FakeApi(self)
        runner = run_live.Runner(
            api,
            FakeCapacity(api),
            {model: rates},
            run_live.Budget(upper * 3),
            timeout=1,
            delay=0,
            run_id="run",
            cache_scopes={"profile-a": "profile-1"},
            admission=run_live.Gemini37Admission(
                "profile-a",
                "b" * 40,
                thinking_levels=run_live.GEMINI_37_THINKING_LEVELS,
            ),
        )
        leg = run_live.Leg(
            f"admission:{model}:thinking-high",
            model,
            "fresh",
            thinking_level="high",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        with mock.patch.object(run_live.time, "time", return_value=1_000), mock.patch.object(
            run_live.time, "sleep", return_value=None
        ):
            record = runner.execute_leg(leg, "profile-a")

        self.assertEqual(len(api.calls), 2)
        self.assertTrue(api.calls[0][0].endswith(":countTokens"))
        self.assertTrue(record["response_evidence"]["incremental_sse"])
        self.assertIsNone(record["coverage_error"])
        self.assertEqual(record["admission"]["thinking_level"], "high")
        self.assertEqual(
            [attempt["kind"] for attempt in runner.admission_attempts],
            ["countTokens", "paid_generation"],
        )

        wrong_leg = dataclasses.replace(leg, name=f"admission:{model}:default-sse")
        with self.assertRaisesRegex(run_live.CalibrationError, "exact contract"):
            runner.execute_leg(wrong_leg, "profile-a")
        minimal_leg = dataclasses.replace(
            leg,
            name=f"admission:{model}:thinking-minimal",
            thinking_level="minimal",
        )
        with self.assertRaisesRegex(run_live.CalibrationError, "exact contract"):
            runner.execute_leg(minimal_leg, "profile-a")

    def test_gemini_37_explicit_level_fails_closed_without_thinking_tokens(self):
        model = run_live.GEMINI_37_ADMISSION_MODEL
        leg = run_live.Leg(
            f"admission:{model}:thinking-medium",
            model,
            "fresh",
            thinking_level="medium",
            stream=True,
            max_output_tokens=run_live.GEMINI_37_ADMISSION_OUTPUT_TOKENS,
        )
        immutable = event(model=model)
        immutable.update({
            "input_tokens": 10,
            "output_tokens": 3,
            "thinking_output_tokens": 0,
        })
        immutable = run_live.recent_turn_events(capacity([immutable]))["req-1"]
        self.assertEqual(
            run_live.verify_leg_usage(leg, immutable),
            run_live.THINKING_TOKENS_NOT_OBSERVED,
        )
        # The live-proven low level is the single accepted zero-thinking admission effort.
        low_leg = dataclasses.replace(
            leg,
            name=f"admission:{model}:thinking-low",
            thinking_level="low",
        )
        self.assertIsNone(run_live.verify_leg_usage(low_leg, immutable))


if __name__ == "__main__":
    unittest.main()
