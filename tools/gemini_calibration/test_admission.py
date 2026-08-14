import json
import multiprocessing
import os
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

from tools.gemini_calibration import admission


SHA = "0123456789abcdef0123456789abcdef01234567"
PROFILE = "profile-opaque-a"
PLAN = "google_ai_pro"
PROMO_TEST_EPOCH = 1_786_665_600
DISPATCH_MS = PROMO_TEST_EPOCH * 1000 + 123


def rate_row(*, input_limit=10, input_rate=1, output_rate=1):
    return {
        "id": admission.MODEL,
        "tariff_schedule_id": "google/gemini-test/2026-08-14",
        "input_token_limit": str(input_limit),
        "output_token_limit": "65536",
        "rates": {
            "input_nanousd_per_token": str(input_rate),
            "audio_input_nanousd_per_token": str(input_rate),
            "cached_input_nanousd_per_token": str(input_rate),
            "cached_audio_input_nanousd_per_token": str(input_rate),
            "output_nanousd_per_token": str(output_rate),
            "image_output_nanousd_per_token": "0",
            "long_context_threshold": str(2**64 - 1),
            "long_input_nanousd_per_token": str(input_rate),
            "long_audio_input_nanousd_per_token": str(input_rate),
            "long_cached_input_nanousd_per_token": str(input_rate),
            "long_cached_audio_input_nanousd_per_token": str(input_rate),
            "long_output_nanousd_per_token": str(output_rate),
        },
        "search": {"billing_unit": "query", "nanousd_per_unit": "14000000"},
    }


def test_rate():
    return admission.run_live.ModelRates(
        tariff_schedule_id="google/gemini-test/2026-08-14",
        input_token_limit=10,
        input=1,
        audio_input=1,
        cached_input=1,
        cached_audio_input=1,
        output=1,
        image_output=0,
        long_threshold=2**64 - 1,
        long_input=1,
        long_audio_input=1,
        long_cached_input=1,
        long_cached_audio_input=1,
        long_output=1,
        search_unit="query",
        search=14_000_000,
        max_output_tokens=65_536,
    )


def official_rate_row(epoch):
    rate = admission._expected_official_rate_at(epoch)
    return {
        "id": admission.MODEL,
        "tariff_schedule_id": rate.tariff_schedule_id,
        "input_token_limit": str(rate.input_token_limit),
        "output_token_limit": str(rate.max_output_tokens),
        "rates": {
            "input_nanousd_per_token": str(rate.input),
            "audio_input_nanousd_per_token": str(rate.audio_input),
            "cached_input_nanousd_per_token": str(rate.cached_input),
            "cached_audio_input_nanousd_per_token": str(rate.cached_audio_input),
            "output_nanousd_per_token": str(rate.output),
            "image_output_nanousd_per_token": str(rate.image_output),
            "long_context_threshold": str(rate.long_threshold),
            "long_input_nanousd_per_token": str(rate.long_input),
            "long_audio_input_nanousd_per_token": str(rate.long_audio_input),
            "long_cached_input_nanousd_per_token": str(rate.long_cached_input),
            "long_cached_audio_input_nanousd_per_token": str(rate.long_cached_audio_input),
            "long_output_nanousd_per_token": str(rate.long_output),
        },
        "search": {
            "billing_unit": rate.search_unit,
            "nanousd_per_unit": str(rate.search),
        },
    }


def capacity(row=None):
    return {
        "calibration_authority_available": True,
        "calibration_delivery": {
            "pending_events": 0,
            "dropped_events": 0,
            "persistence_ok": True,
        },
        "calibration_recent_turn_limit": 512,
        "calibration_recent_turns": [],
        "profiles": [
            {
                "id": PROFILE,
                "plan": PLAN,
                "authenticated": True,
                "cooling_until": 0,
                "calibration_persistence_ok": True,
                "windows": [],
            },
            {
                "id": "other-opaque-profile",
                "plan": "google_ai_ultra",
                "authenticated": True,
                "cooling_until": 0,
                "calibration_persistence_ok": True,
                "windows": [],
            },
        ],
        "conversion_models": [row or rate_row()],
    }


def immutable_event(
    request_id,
    *,
    output_tokens=1,
    total=3,
    priced_ts=PROMO_TEST_EPOCH,
    completed_at=PROMO_TEST_EPOCH,
):
    return {
        "request_id": request_id,
        "profile_id": PROFILE,
        "model": admission.MODEL,
        "tariff_schedule_id": "google/gemini-test/2026-08-14",
        "input_tokens": "2",
        "audio_input_tokens": "0",
        "cache_read_tokens": "0",
        "cached_audio_input_tokens": "0",
        "cache_write_5m_tokens": "0",
        "cache_write_1h_tokens": "0",
        "output_tokens": str(output_tokens),
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
        "api_output_nanousd": str(total - 2),
        "api_image_output_nanousd": "0",
        "api_search_nanousd": "0",
        "api_total_nanousd": str(total),
        "priced_ts": str(priced_ts),
        "completed_at": str(completed_at),
    }


def response(*, output_tokens=1, model=admission.MODEL):
    return {
        "modelVersion": model,
        "candidates": [
            {
                "content": {"parts": [{"text": "OK"}]},
                "finishReason": "STOP",
            }
        ],
        "usageMetadata": {
            "promptTokenCount": 2,
            "candidatesTokenCount": output_tokens,
        },
    }


def stream_response(*, output_tokens=1, model=admission.MODEL):
    split = len(admission.EXPECTED_OUTPUT) // 2
    return [
        {
            "modelVersion": model,
            "candidates": [
                {"content": {"parts": [{"text": admission.EXPECTED_OUTPUT[:split]}]}}
            ],
        },
        {
            "modelVersion": model,
            "candidates": [
                {
                    "content": {
                        "parts": [{"text": admission.EXPECTED_OUTPUT[split:]}]
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 2,
                "candidatesTokenCount": output_tokens,
            },
        },
    ]


def immutable_capacity(event, *, plan=PLAN, include_profile=True):
    return {
        "calibration_authority_available": True,
        "calibration_delivery": {
            "pending_events": 0,
            "dropped_events": 0,
            "persistence_ok": True,
        },
        "calibration_recent_turn_limit": 512,
        "calibration_recent_turns": [event],
        "profiles": (
            [
                {
                    "id": PROFILE,
                    "plan": plan,
                    "authenticated": True,
                    "cooling_until": 0,
                    "calibration_persistence_ok": True,
                    "windows": [],
                }
            ]
            if include_profile
            else []
        ),
    }


def count_observation(evidence, *, total_tokens=2, **overrides):
    journal = json.loads((evidence / admission.JOURNAL).read_text())
    value = {
        "schema": admission.COUNT_OBSERVATION_SCHEMA,
        "request_id": journal["count_request_id"],
        "request_sha256": journal["count_request_sha256"],
        "target_profile": PROFILE,
        "model": admission.MODEL,
        "http_status": 200,
        "execution_state": "completed",
        "dispatch_ms": DISPATCH_MS,
        "response": {"totalTokens": total_tokens},
    }
    value.update(overrides)
    if "dispatch_ms" not in overrides and value["http_status"] != 200:
        value["dispatch_ms"] = None
    return value


def outcome_observation(evidence, event, *, observed_response=None, capacity_value=None, **overrides):
    journal = json.loads((evidence / admission.JOURNAL).read_text())
    value = {
        "schema": admission.OBSERVATION_SCHEMA,
        "request_id": journal["request_id"],
        "request_sha256": journal["generation_request_sha256"],
        "target_profile": PROFILE,
        "plan": PLAN,
        "http_status": 200,
        "execution_state": "completed",
        "dispatch_ms": DISPATCH_MS,
        "response": (
            stream_response() if journal["stream"] else response()
        ) if observed_response is None else observed_response,
        "immutable_capacity": (
            immutable_capacity(event) if capacity_value is None else capacity_value
        ),
        "event_request_id": journal["request_id"],
        "event_plan": PLAN,
    }
    value.update(overrides)
    if "dispatch_ms" not in overrides and value["http_status"] != 200:
        value["dispatch_ms"] = None
    return value


def replace_json_value(path, replacement):
    path.write_text(json.dumps(replacement) + "\n", encoding="utf-8")
    os.chmod(path, 0o600)


def claim_worker(evidence, start, results):
    start.wait()
    try:
        admission.claim_generation(Path(evidence))
    except admission.AdmissionError:
        results.put("closed")
    else:
        results.put("claimed")


def count_claim_worker(evidence, start, results):
    start.wait()
    try:
        admission.claim_count(Path(evidence))
    except admission.AdmissionError:
        results.put("closed")
    else:
        results.put("claimed")


def outcome_worker(evidence, observation, start, results):
    start.wait()
    try:
        summary = admission.record_outcome(Path(evidence), Path(observation))
    except admission.AdmissionError:
        results.put("closed")
    else:
        results.put(summary["state"])


class AdmissionFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.capacity_file = self.root / "capacity.json"
        self.profile_file = self.root / "profile"
        self.evidence = self.root / "evidence"
        self.write_json(self.capacity_file, capacity())
        self.profile_file.write_text(PROFILE + "\n", encoding="utf-8")
        os.chmod(self.profile_file, 0o600)
        # End-to-end state-machine tests use a deliberately cheap internal card. Production
        # initialize/load paths still call the exact official effective-rate helper, which has
        # separate unmocked contract tests below.
        self.rate_patcher = mock.patch.object(
            admission,
            "_expected_official_rate_at",
            return_value=test_rate(),
        )
        self.clock_patcher = mock.patch.object(
            admission.time,
            "time",
            return_value=PROMO_TEST_EPOCH,
        )
        self.rate_patcher.start()
        self.clock_patcher.start()

    def tearDown(self):
        self.clock_patcher.stop()
        self.rate_patcher.stop()
        self.temporary.cleanup()

    @staticmethod
    def write_json(path, value):
        path.write_text(json.dumps(value) + "\n", encoding="utf-8")
        os.chmod(path, 0o600)

    def initialize(
        self,
        *,
        stream=True,
        budget=admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
        max_output_tokens=admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
    ):
        return admission.initialize(
            self.evidence,
            self.capacity_file,
            self.profile_file,
            PLAN,
            SHA,
            SHA,
            budget,
            max_output_tokens,
            stream,
        )

    def count_and_arm(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        admission.arm_generation(self.evidence)
        return admission.claim_generation(self.evidence)


class AdmissionContractTests(AdmissionFixture):
    def test_initialize_is_offline_exact_profile_and_count_tokens_first(self):
        with mock.patch("urllib.request.urlopen") as urlopen, mock.patch(
            "subprocess.run"
        ) as subprocess_run:
            summary = self.initialize()
        urlopen.assert_not_called()
        subprocess_run.assert_not_called()
        self.assertEqual(summary["state"], "awaiting_count_tokens")
        self.assertFalse(summary["count_tokens_claimed"])
        self.assertFalse(summary["generation_dispatched"])
        self.assertEqual(summary["not_after"], admission.PROMO_END_EPOCH)
        self.assertNotIn(PROFILE, json.dumps(summary))
        self.assertNotIn(PLAN, json.dumps(summary))
        self.assertFalse((self.evidence / admission.GENERATION_REQUEST).exists())
        request = json.loads((self.evidence / admission.COUNT_REQUEST).read_text())
        self.assertEqual(request["kind"], "count_tokens")
        self.assertEqual(
            request["path"],
            "/v1beta/models/gemini-3.7-flash:countTokens",
        )
        self.assertEqual(request["target_profile"], PROFILE)
        self.assertEqual(request["request_id"].count("-"), 4)
        self.assertEqual(request["not_after"], admission.PROMO_END_EPOCH)
        self.assertEqual(
            request["body"]["contents"][0]["parts"][0]["text"],
            admission.PROMPT,
        )
        self.assertNotIn("calibration_request_id", request)

    def test_nonstream_contract_is_rejected_before_evidence(self):
        with self.assertRaises(admission.AdmissionError):
            self.initialize(stream=False)
        self.assertFalse(self.evidence.exists())

    def test_count_tokens_accepts_canonical_decimal_string(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence, total_tokens="2"))
        admission.claim_count(self.evidence)
        summary = admission.record_count(self.evidence, count)
        self.assertEqual(summary["state"], "counted")
        self.assertEqual(summary["count_dispatch_ms"], DISPATCH_MS)

    def test_count_tokens_rejects_float_negative_and_noncanonical_values(self):
        invalid_values = (2.0, -2, "-2", "02", "+2", " 2", "2 ", "2.0", True)
        for index, total_tokens in enumerate(invalid_values):
            with self.subTest(total_tokens=total_tokens):
                evidence = self.root / f"invalid-count-{index}"
                admission.initialize(
                    evidence,
                    self.capacity_file,
                    self.profile_file,
                    PLAN,
                    SHA,
                    SHA,
                )
                count = self.root / f"invalid-count-{index}.json"
                self.write_json(
                    count,
                    count_observation(evidence, total_tokens=total_tokens),
                )
                admission.claim_count(evidence)
                with self.assertRaises(admission.AdmissionError):
                    admission.record_count(evidence, count)
                self.assertEqual(
                    admission.inspect(evidence)["state"],
                    "withdrawn_count_tokens",
                )

    def test_count_tokens_requires_pre_cutoff_dispatch_attestation(self):
        invalid_values = (
            None,
            True,
            0,
            "1",
            admission.PROMO_END_EPOCH * 1000,
            admission.PROMO_END_EPOCH * 1000 + 1,
        )
        for index, dispatch_ms in enumerate(invalid_values):
            with self.subTest(dispatch_ms=dispatch_ms):
                evidence = self.root / f"invalid-count-dispatch-{index}"
                admission.initialize(
                    evidence,
                    self.capacity_file,
                    self.profile_file,
                    PLAN,
                    SHA,
                    SHA,
                )
                observation = self.root / f"invalid-count-dispatch-{index}.json"
                self.write_json(
                    observation,
                    count_observation(evidence, dispatch_ms=dispatch_ms),
                )
                admission.claim_count(evidence)
                with self.assertRaises(admission.AdmissionError):
                    admission.record_count(evidence, observation)
                self.assertEqual(
                    admission.inspect(evidence)["state"],
                    "withdrawn_count_tokens",
                )

    def test_init_rejects_non_exact_shas_plan_budget_and_replay(self):
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                PLAN,
                SHA.upper(),
                SHA.upper(),
            )
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                PLAN,
                SHA,
                "f" * 40,
            )
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                "free",
                SHA,
                SHA,
            )
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                PLAN,
                SHA,
                SHA,
                admission.AUTHORIZED_STANDARD_CEILING_NANOUSD + 1,
            )
        self.initialize()
        with self.assertRaises(admission.AdmissionError):
            self.initialize()

    def test_explicit_profile_must_match_exact_authoritative_plan(self):
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                "google_ai_ultra",
                SHA,
                SHA,
            )

    def test_generation_request_is_digest_bound_before_irreversible_arm(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        summary = admission.record_count(self.evidence, count)
        self.assertEqual(summary["state"], "counted")
        self.assertFalse(summary["generation_dispatched"])
        request_path = self.evidence / admission.GENERATION_REQUEST
        request = json.loads(request_path.read_text())
        self.assertEqual(request["target_profile"], PROFILE)
        self.assertEqual(request["calibration_request_id"].count("-"), 4)
        self.assertEqual(request["not_after"], admission.PROMO_END_EPOCH)
        request["body"]["contents"][0]["parts"][0]["text"] = "tampered"
        self.write_json(request_path, request)
        with self.assertRaises(admission.AdmissionError):
            admission.arm_generation(self.evidence)
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_recomputed_request_and_journal_digests_cannot_rewrite_contract(self):
        self.initialize()
        count_path = self.evidence / admission.COUNT_REQUEST
        count_request = json.loads(count_path.read_text())
        count_request["body"]["contents"][0]["parts"][0]["text"] = "attacker prompt"
        self.write_json(count_path, count_request)
        journal_path = self.evidence / admission.JOURNAL
        journal = json.loads(journal_path.read_text())
        journal["count_request_sha256"] = admission._request_digest(count_path)
        journal["contract_sha256"] = admission._contract_digest(journal)
        self.write_json(journal_path, journal)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

        other = self.root / "generation-tamper"
        admission.initialize(
            other,
            self.capacity_file,
            self.profile_file,
            PLAN,
            SHA,
            SHA,
            admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
            admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
        )
        count = self.root / "generation-count.json"
        self.write_json(count, count_observation(other))
        admission.claim_count(other)
        admission.record_count(other, count)
        request_path = other / admission.GENERATION_REQUEST
        request = json.loads(request_path.read_text())
        request["body"]["tools"] = [{"googleSearch": {}}]
        self.write_json(request_path, request)
        journal_path = other / admission.JOURNAL
        journal = json.loads(journal_path.read_text())
        journal["generation_request_sha256"] = admission._request_digest(request_path)
        journal["contract_sha256"] = admission._contract_digest(journal)
        self.write_json(journal_path, journal)
        with self.assertRaises(admission.AdmissionError):
            admission.arm_generation(other)

    def test_count_observation_is_bound_to_exact_request_digest_and_identity(self):
        for field, value in (
            ("request_id", "00000000-0000-4000-8000-000000000000"),
            ("request_sha256", "0" * 64),
            ("target_profile", "other-opaque-profile"),
            ("model", "gemini-3.6-flash"),
        ):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                evidence = root / "evidence"
                profile = root / "profile"
                capacity_file = root / "capacity.json"
                profile.write_text(PROFILE + "\n", encoding="utf-8")
                os.chmod(profile, 0o600)
                self.write_json(capacity_file, capacity())
                admission.initialize(evidence, capacity_file, profile, PLAN, SHA, SHA)
                count = root / "count.json"
                self.write_json(count, count_observation(evidence, **{field: value}))
                admission.claim_count(evidence)
                with self.assertRaises(admission.AdmissionError):
                    admission.record_count(evidence, count)
                self.assertEqual(admission.inspect(evidence)["state"], "withdrawn_count_tokens")

    def test_count_claim_is_atomic_and_single_use_across_processes(self):
        self.initialize()
        context = multiprocessing.get_context("fork")
        start = context.Event()
        results = context.Queue()
        workers = [
            context.Process(
                target=count_claim_worker,
                args=(self.evidence, start, results),
            )
            for _ in range(2)
        ]
        for worker in workers:
            worker.start()
        start.set()
        for worker in workers:
            worker.join(10)
            self.assertEqual(worker.exitcode, 0)
        self.assertCountEqual(
            [results.get(timeout=2) for _ in workers],
            ["claimed", "closed"],
        )
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "count_tokens_claimed")
        self.assertTrue(summary["count_tokens_claimed"])
        self.assertFalse(summary["count_tokens_recorded"])
        with self.assertRaises(admission.AdmissionError):
            admission.claim_count(self.evidence)

    def test_count_claim_crash_before_journal_replace_is_permanent_ambiguity(self):
        self.initialize()
        with mock.patch.object(admission, "_write_journal", side_effect=OSError("crash")):
            with self.assertRaises(OSError):
                admission.claim_count(self.evidence)
        self.assertTrue((self.evidence / admission.COUNT_DISPATCH_CLAIM).exists())
        with self.assertRaises(admission.AdmissionError):
            admission.claim_count(self.evidence)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

    def test_count_failure_is_terminal_and_cannot_be_recorded_or_claimed_again(self):
        self.initialize()
        admission.claim_count(self.evidence)
        failed = self.root / "count-failed.json"
        self.write_json(
            failed,
            count_observation(
                self.evidence,
                http_status=401,
                execution_state="unknown",
                response=None,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_count(self.evidence, failed)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_count_tokens")
        self.assertTrue(summary["count_tokens_claimed"])
        self.assertFalse(summary["count_tokens_recorded"])
        self.assertEqual(summary["http_status"], 401)
        self.assertEqual(summary["failure_class"], "count_tokens_failed_no_retry")
        self.assertTrue((self.evidence / admission.COUNT_OUTCOME_RECEIPT).exists())
        with self.assertRaises(admission.AdmissionError):
            admission.record_count(self.evidence, failed)
        with self.assertRaises(admission.AdmissionError):
            admission.claim_count(self.evidence)

    def test_count_success_requires_completed_execution_evidence(self):
        for execution_state in (None, "not_started", "started", "unknown"):
            with (
                self.subTest(execution_state=execution_state),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                evidence = root / "evidence"
                capacity_file = root / "capacity.json"
                profile_file = root / "profile.txt"
                self.write_json(capacity_file, capacity())
                profile_file.write_text(PROFILE + "\n")
                profile_file.chmod(0o600)
                admission.initialize(evidence, capacity_file, profile_file, PLAN, SHA, SHA)
                observation = root / "count.json"
                self.write_json(
                    observation,
                    count_observation(evidence, execution_state=execution_state),
                )
                admission.claim_count(evidence)
                with self.assertRaises(admission.AdmissionError):
                    admission.record_count(evidence, observation)
                summary = admission.inspect(evidence)
                self.assertEqual(summary["state"], "withdrawn_count_tokens")
                self.assertEqual(summary["failure_class"], "count_tokens_invalid")

    def test_count_failure_rejects_contradictory_completed_execution_evidence(self):
        self.initialize()
        failed = self.root / "count-failed-completed.json"
        self.write_json(
            failed,
            count_observation(
                self.evidence,
                http_status=503,
                execution_state="completed",
                response=None,
            ),
        )
        admission.claim_count(self.evidence)
        with self.assertRaises(admission.AdmissionError):
            admission.record_count(self.evidence, failed)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_count_tokens")
        self.assertEqual(summary["failure_class"], "count_tokens_invalid")

    def test_single_use_claim_is_atomic_across_processes(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        admission.arm_generation(self.evidence)
        context = multiprocessing.get_context("fork")
        start = context.Event()
        results = context.Queue()
        workers = [
            context.Process(target=claim_worker, args=(self.evidence, start, results))
            for _ in range(2)
        ]
        for worker in workers:
            worker.start()
        start.set()
        for worker in workers:
            worker.join(10)
            self.assertEqual(worker.exitcode, 0)
        self.assertCountEqual([results.get(timeout=2) for _ in workers], ["claimed", "closed"])
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "generation_claimed")
        self.assertTrue(summary["generation_claimed"])
        with self.assertRaises(admission.AdmissionError):
            admission.claim_generation(self.evidence)

    def test_claim_crash_before_journal_replace_is_permanent_ambiguity(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        admission.arm_generation(self.evidence)
        with mock.patch.object(admission, "_write_journal", side_effect=OSError("crash")):
            with self.assertRaises(OSError):
                admission.claim_generation(self.evidence)
        self.assertTrue((self.evidence / admission.DISPATCH_CLAIM).exists())
        with self.assertRaises(admission.AdmissionError):
            admission.claim_generation(self.evidence)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

    def test_concurrent_record_count_has_one_transition_winner(self):
        self.initialize()
        valid = self.root / "valid-count.json"
        invalid = self.root / "invalid-count.json"
        self.write_json(valid, count_observation(self.evidence))
        self.write_json(
            invalid,
            count_observation(self.evidence, request_sha256="0" * 64),
        )
        admission.claim_count(self.evidence)
        barrier = threading.Barrier(2)
        results = []

        def run(path):
            barrier.wait()
            try:
                results.append(admission.record_count(self.evidence, path)["state"])
            except admission.AdmissionError:
                results.append("closed")

        threads = [threading.Thread(target=run, args=(path,)) for path in (valid, invalid)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(10)
            self.assertFalse(thread.is_alive())
        final_state = admission.inspect(self.evidence)["state"]
        self.assertIn(final_state, {"counted", "withdrawn_count_tokens"})
        if final_state == "counted":
            self.assertCountEqual(results, ["counted", "closed"])
        else:
            self.assertEqual(results, ["closed", "closed"])

    def test_fence_without_completed_journal_arm_is_fail_closed(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        fence = {
            "schema": "gemini-3.7-admission-dispatch-fence/v1",
            "model": admission.MODEL,
            "implementation_sha": SHA,
            "release_sha": SHA,
            "contract_sha256": journal["contract_sha256"],
            "generation_request_sha256": journal["generation_request_sha256"],
            "request_id": journal["request_id"],
            "not_after": journal["not_after"],
        }
        self.write_json(self.evidence / admission.DISPATCH_FENCE, fence)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)
        with self.assertRaises(admission.AdmissionError):
            admission.arm_generation(self.evidence)

    def test_inspector_rejects_journal_tampering_and_unexpected_artifacts(self):
        self.initialize()
        journal_path = self.evidence / admission.JOURNAL
        journal = json.loads(journal_path.read_text())
        journal["budget_nanousd"] = str(
            admission.AUTHORIZED_STANDARD_CEILING_NANOUSD + 1
        )
        journal["contract_sha256"] = admission._contract_digest(journal)
        self.write_json(journal_path, journal)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

        other_evidence = self.root / "other-evidence"
        admission.initialize(
            other_evidence,
            self.capacity_file,
            self.profile_file,
            PLAN,
            SHA,
            SHA,
        )
        (other_evidence / "unexpected").write_text("x", encoding="utf-8")
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(other_evidence)

    def test_rate_card_types_are_revalidated_before_arithmetic(self):
        self.initialize()
        journal_path = self.evidence / admission.JOURNAL
        journal = json.loads(journal_path.read_text())
        journal["rate"]["input"] = "1"
        # Keep the contract digest internally consistent so the test reaches strict rate parsing.
        journal["contract_sha256"] = admission._contract_digest(journal)
        self.write_json(journal_path, journal)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

    def test_promotional_cutoff_is_immutable_journal_contract_state(self):
        self.initialize()
        journal_path = self.evidence / admission.JOURNAL
        journal = json.loads(journal_path.read_text())
        self.assertEqual(journal["not_after"], admission.PROMO_END_EPOCH)
        journal["not_after"] -= 1
        journal["contract_sha256"] = admission._contract_digest(journal)
        self.write_json(journal_path, journal)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

    def test_arm_binds_exact_cutoff_into_permanent_dispatch_fence(self):
        summary = self.count_and_arm()
        fence = json.loads((self.evidence / admission.DISPATCH_FENCE).read_text())
        request = json.loads((self.evidence / admission.GENERATION_REQUEST).read_text())
        self.assertEqual(summary["not_after"], admission.PROMO_END_EPOCH)
        self.assertEqual(request["not_after"], admission.PROMO_END_EPOCH)
        self.assertEqual(fence["not_after"], admission.PROMO_END_EPOCH)

    def test_arm_rejects_generation_request_with_tampered_cutoff(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        request_path = self.evidence / admission.GENERATION_REQUEST
        request = json.loads(request_path.read_text())
        request["not_after"] -= 1
        self.write_json(request_path, request)
        with self.assertRaises(admission.AdmissionError):
            admission.arm_generation(self.evidence)
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_inspector_rejects_dispatch_fence_with_tampered_cutoff(self):
        self.count_and_arm()
        fence_path = self.evidence / admission.DISPATCH_FENCE
        fence = json.loads(fence_path.read_text())
        fence["not_after"] -= 1
        self.write_json(fence_path, fence)
        with self.assertRaises(admission.AdmissionError):
            admission.inspect(self.evidence)

    def test_symlinked_inputs_are_rejected(self):
        symlink = self.root / "profile-link"
        symlink.symlink_to(self.profile_file)
        with self.assertRaises(admission.AdmissionError):
            admission.initialize(
                self.evidence,
                self.capacity_file,
                symlink,
                PLAN,
                SHA,
                SHA,
            )

    def test_failed_generation_is_terminal_even_when_provider_proves_not_started(self):
        self.count_and_arm()
        observation = self.root / "observation.json"
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                http_status=503,
                execution_state="not_started",
                response=None,
                immutable_capacity=None,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_generation_not_started")
        self.assertTrue(summary["generation_dispatched"])
        with self.assertRaises(admission.AdmissionError):
            admission.arm_generation(self.evidence)
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)

    def test_ambiguous_transport_failure_is_terminal_without_evidence_retry(self):
        self.count_and_arm()
        observation = self.root / "observation.json"
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                http_status=0,
                execution_state="unknown",
                response=None,
                immutable_capacity=None,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_generation_ambiguous")
        self.assertEqual(summary["http_status"], 0)
        self.assertEqual(summary["execution_state"], "unknown")
        self.assertTrue(summary["generation_dispatched"])
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)

    def test_generation_success_requires_completed_execution_evidence(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        observation = self.root / "observation.json"
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                execution_state="unknown",
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_evidence")
        self.assertEqual(summary["failure_class"], "generation_evidence_rejected")

    def test_stream_success_requires_visible_output_model_usage_and_cost_parity(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        observation = self.root / "observation.json"
        self.write_json(
            observation,
            outcome_observation(self.evidence, immutable_event(journal["request_id"])),
        )
        summary = admission.record_outcome(self.evidence, observation)
        self.assertEqual(summary["state"], "success")
        self.assertEqual(summary["actual_nanousd"], "3")
        self.assertTrue(summary["response_evidence"]["terminal_finish"])
        self.assertTrue(summary["response_evidence"]["terminal_usage"])
        self.assertTrue(summary["response_evidence"]["usage_matches_immutable_event"])
        self.assertTrue(summary["response_evidence"]["exact_fixed_output"])
        self.assertEqual(summary["response_evidence"]["model_version"], admission.MODEL)
        self.assertTrue(summary["response_evidence"]["raw_upstream_model_version"])
        self.assertEqual(summary["generation_dispatch_ms"], DISPATCH_MS)
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        self.assertEqual(journal["evidence"]["priced_ts"], str(PROMO_TEST_EPOCH))
        self.assertEqual(journal["evidence"]["calibration_dispatch_ms"], str(DISPATCH_MS))
        admission.inspect(self.evidence, require_success=True)
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)

    def test_generation_requires_pre_cutoff_dispatch_attestation(self):
        invalid_values = (
            None,
            False,
            0,
            "1",
            admission.PROMO_END_EPOCH * 1000,
            admission.PROMO_END_EPOCH * 1000 + 1,
        )
        for index, dispatch_ms in enumerate(invalid_values):
            with self.subTest(dispatch_ms=dispatch_ms):
                evidence = self.root / f"invalid-generation-dispatch-{index}"
                admission.initialize(
                    evidence,
                    self.capacity_file,
                    self.profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                )
                count = self.root / f"invalid-generation-count-{index}.json"
                self.write_json(count, count_observation(evidence))
                admission.claim_count(evidence)
                admission.record_count(evidence, count)
                admission.arm_generation(evidence)
                admission.claim_generation(evidence)
                journal = json.loads((evidence / admission.JOURNAL).read_text())
                observation = self.root / f"invalid-generation-dispatch-{index}.json"
                self.write_json(
                    observation,
                    outcome_observation(
                        evidence,
                        immutable_event(journal["request_id"]),
                        dispatch_ms=dispatch_ms,
                    ),
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)
                self.assertEqual(
                    admission.inspect(evidence)["state"],
                    "withdrawn_evidence",
                )

    def test_stream_success_requires_exact_fixed_output(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        frames = stream_response()
        frames[1]["candidates"][0]["content"]["parts"][0]["text"] += " 65"
        observation = self.root / "wrong-output.json"
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                observed_response=frames,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        self.assertEqual(admission.inspect(self.evidence)["state"], "withdrawn_evidence")

    def test_stream_success_rejects_extra_output_whitespace(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        frames = stream_response()
        frames[1]["candidates"][0]["content"]["parts"][0]["text"] += "\n"
        observation = self.root / "extra-whitespace.json"
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                observed_response=frames,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        self.assertEqual(admission.inspect(self.evidence)["state"], "withdrawn_evidence")

    def test_malformed_stream_is_terminally_withdrawn(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        observation = self.root / "malformed-stream.json"
        frames = stream_response()
        frames[0]["candidates"] = [1]
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"]),
                observed_response=frames,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_evidence")
        self.assertEqual(summary["failure_class"], "generation_evidence_rejected")

    def test_exact_event_cost_must_reproduce_from_pinned_rate_card(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        event = immutable_event(journal["request_id"])
        event["api_input_nanousd"] = "1"
        event["api_total_nanousd"] = "2"
        observation = self.root / "observation.json"
        self.write_json(
            observation,
            outcome_observation(self.evidence, event),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_evidence")
        self.assertTrue(summary["generation_dispatched"])

    def test_terminal_success_requires_contemporaneous_exact_profile_plan(self):
        for name, terminal_capacity, event_plan in (
            (
                "missing-profile",
                lambda event: immutable_capacity(event, include_profile=False),
                PLAN,
            ),
            (
                "mismatched-profile-plan",
                lambda event: immutable_capacity(event, plan="google_ai_ultra"),
                PLAN,
            ),
            (
                "mismatched-event-plan",
                lambda event: immutable_capacity(event),
                "google_ai_ultra",
            ),
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                capacity_file = root / "capacity.json"
                profile_file = root / "profile"
                evidence = root / "evidence"
                self.write_json(capacity_file, capacity())
                profile_file.write_text(PROFILE + "\n", encoding="utf-8")
                os.chmod(profile_file, 0o600)
                admission.initialize(
                    evidence,
                    capacity_file,
                    profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                )
                count = root / "count.json"
                self.write_json(count, count_observation(evidence))
                admission.claim_count(evidence)
                admission.record_count(evidence, count)
                admission.arm_generation(evidence)
                admission.claim_generation(evidence)
                journal = json.loads((evidence / admission.JOURNAL).read_text())
                event = immutable_event(journal["request_id"])
                observation = root / "observation.json"
                self.write_json(
                    observation,
                    outcome_observation(
                        evidence,
                        event,
                        capacity_value=terminal_capacity(event),
                        event_plan=event_plan,
                    ),
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)
                self.assertEqual(admission.inspect(evidence)["state"], "withdrawn_evidence")

    def test_concurrent_success_and_failure_have_one_irreversible_terminal_winner(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        event = immutable_event(journal["request_id"])
        success = self.root / "success.json"
        failure = self.root / "failure.json"
        self.write_json(success, outcome_observation(self.evidence, event))
        self.write_json(
            failure,
            outcome_observation(
                self.evidence,
                event,
                http_status=503,
                execution_state="not_started",
                response=None,
                immutable_capacity=None,
            ),
        )
        context = multiprocessing.get_context("fork")
        start = context.Event()
        results = context.Queue()
        workers = [
            context.Process(
                target=outcome_worker,
                args=(self.evidence, path, start, results),
            )
            for path in (success, failure)
        ]
        for worker in workers:
            worker.start()
        start.set()
        for worker in workers:
            worker.join(10)
            self.assertEqual(worker.exitcode, 0)
        outcomes = [results.get(timeout=2) for _ in workers]
        self.assertEqual(outcomes.count("closed"), 1)
        self.assertIn(admission.inspect(self.evidence)["state"], {"success", "withdrawn_generation_not_started"})

    def test_outcome_rejects_missing_noncanonical_or_out_of_epoch_priced_ts(self):
        invalid_values = (
            ("missing", None),
            ("zero", "0"),
            ("leading-zero", f"0{PROMO_TEST_EPOCH}"),
            ("before-rate-epoch", PROMO_TEST_EPOCH - 1),
            ("promo-cutoff", admission.PROMO_END_EPOCH),
        )
        for name, priced_ts in invalid_values:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                capacity_file = root / "capacity.json"
                profile_file = root / "profile"
                evidence = root / "evidence"
                self.write_json(capacity_file, capacity())
                profile_file.write_text(PROFILE + "\n", encoding="utf-8")
                os.chmod(profile_file, 0o600)
                admission.initialize(
                    evidence,
                    capacity_file,
                    profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                )
                count = root / "count.json"
                self.write_json(count, count_observation(evidence))
                admission.claim_count(evidence)
                admission.record_count(evidence, count)
                admission.arm_generation(evidence)
                admission.claim_generation(evidence)
                journal = json.loads((evidence / admission.JOURNAL).read_text())
                event = immutable_event(journal["request_id"])
                if priced_ts is None:
                    event.pop("priced_ts")
                else:
                    event["priced_ts"] = priced_ts
                observation = root / "observation.json"
                self.write_json(
                    observation,
                    outcome_observation(evidence, event),
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)
                summary = admission.inspect(evidence)
                self.assertEqual(summary["state"], "withdrawn_evidence")

    def test_outcome_rejects_completion_before_pricing_snapshot(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        event = immutable_event(
            journal["request_id"],
            completed_at=PROMO_TEST_EPOCH - 1,
        )
        observation = self.root / "observation.json"
        self.write_json(
            observation,
            outcome_observation(self.evidence, event),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        self.assertEqual(admission.inspect(self.evidence)["state"], "withdrawn_evidence")

    def test_outcome_reselects_exact_official_rate_at_priced_timestamp(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        priced_ts = PROMO_TEST_EPOCH + 1
        event = immutable_event(
            journal["request_id"],
            priced_ts=priced_ts,
            completed_at=priced_ts,
        )
        observation = self.root / "observation.json"
        self.write_json(
            observation,
            outcome_observation(self.evidence, event),
        )
        with mock.patch.object(
            admission,
            "_expected_official_rate_at",
            side_effect=lambda epoch: (
                test_rate()
                if epoch == PROMO_TEST_EPOCH
                else admission.run_live.ModelRates(
                    **{**test_rate().__dict__, "input": 2}
                )
            ),
        ):
            with self.assertRaises(admission.AdmissionError):
                admission.record_outcome(self.evidence, observation)
        self.assertEqual(admission.inspect(self.evidence)["state"], "withdrawn_evidence")

    def test_stream_success_requires_two_visible_text_frames(self):
        self.initialize()
        count = self.root / "count.json"
        self.write_json(count, count_observation(self.evidence))
        admission.claim_count(self.evidence)
        admission.record_count(self.evidence, count)
        admission.arm_generation(self.evidence)
        admission.claim_generation(self.evidence)
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        observation = self.root / "observation.json"
        frames = stream_response(output_tokens=2)
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"], output_tokens=2, total=4),
                observed_response=frames,
            ),
        )
        summary = admission.record_outcome(self.evidence, observation)
        self.assertTrue(summary["response_evidence"]["incremental_sse"])
        self.assertEqual(summary["response_evidence"]["candidate_frames"], 2)
        self.assertEqual(summary["response_evidence"]["visible_text_frames"], 2)
        self.assertTrue(summary["response_evidence"]["exact_fixed_output"])

    def test_two_candidate_frames_without_preterminal_visible_text_are_rejected(self):
        self.count_and_arm()
        journal = json.loads((self.evidence / admission.JOURNAL).read_text())
        frames = stream_response(output_tokens=2)
        frames[0]["candidates"][0]["content"]["parts"] = [{"text": ""}]
        frames[1]["candidates"][0]["content"]["parts"] = [
            {"text": admission.EXPECTED_OUTPUT}
        ]
        observation = self.root / "buffered-candidate-frames.json"
        self.write_json(
            observation,
            outcome_observation(
                self.evidence,
                immutable_event(journal["request_id"], output_tokens=2, total=4),
                observed_response=frames,
            ),
        )
        with self.assertRaises(admission.AdmissionError):
            admission.record_outcome(self.evidence, observation)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_evidence")
        self.assertTrue(summary["generation_claimed"])

    def test_admission_withdraws_non_stop_or_nonterminal_stop_without_replay(self):
        cases = (
            ("safety", "SAFETY", False),
            ("max-tokens", "MAX_TOKENS", False),
            ("malformed-call", "MALFORMED_FUNCTION_CALL", False),
            ("missing", None, False),
            ("stop-before-final", "STOP", True),
        )
        for name, reason, stop_before_final in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                capacity_file = root / "capacity.json"
                profile_file = root / "profile"
                evidence = root / "evidence"
                self.write_json(capacity_file, capacity())
                profile_file.write_text(PROFILE + "\n", encoding="utf-8")
                os.chmod(profile_file, 0o600)
                admission.initialize(
                    evidence,
                    capacity_file,
                    profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                    True,
                )
                count = root / "count.json"
                self.write_json(count, count_observation(evidence))
                admission.claim_count(evidence)
                admission.record_count(evidence, count)
                admission.arm_generation(evidence)
                admission.claim_generation(evidence)
                journal = json.loads((evidence / admission.JOURNAL).read_text())
                first_candidate = {"content": {"parts": [{"text": "O"}]}}
                terminal_candidate = {"content": {"parts": [{"text": "K"}]}}
                if stop_before_final:
                    first_candidate["finishReason"] = "STOP"
                elif reason is not None:
                    terminal_candidate["finishReason"] = reason
                frames = [
                    {
                        "modelVersion": admission.MODEL,
                        "candidates": [first_candidate],
                    },
                    {
                        "modelVersion": admission.MODEL,
                        "candidates": [terminal_candidate],
                        "usageMetadata": {
                            "promptTokenCount": "2",
                            "candidatesTokenCount": "2",
                        },
                    },
                ]
                observation = root / "observation.json"
                self.write_json(
                    observation,
                    outcome_observation(
                        evidence,
                        immutable_event(
                            journal["request_id"], output_tokens=2, total=4
                        ),
                        observed_response=frames,
                    ),
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)
                self.assertEqual(
                    admission.inspect(evidence)["state"], "withdrawn_evidence"
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)

    def test_buffered_stream_or_wrong_public_model_withdraws_without_replay(self):
        for name, stream, observed_response in (
            ("buffered", True, [response()]),
            (
                "private-model",
                True,
                stream_response(model="private-gemini-3.7-flash"),
            ),
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                capacity_file = root / "capacity.json"
                profile_file = root / "profile"
                evidence = root / "evidence"
                self.write_json(capacity_file, capacity())
                profile_file.write_text(PROFILE + "\n", encoding="utf-8")
                os.chmod(profile_file, 0o600)
                admission.initialize(
                    evidence,
                    capacity_file,
                    profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                    stream,
                )
                count = root / "count.json"
                self.write_json(count, count_observation(evidence))
                admission.claim_count(evidence)
                admission.record_count(evidence, count)
                admission.arm_generation(evidence)
                admission.claim_generation(evidence)
                journal = json.loads((evidence / admission.JOURNAL).read_text())
                observation = root / "observation.json"
                self.write_json(
                    observation,
                    outcome_observation(
                        evidence,
                        immutable_event(journal["request_id"]),
                        observed_response=observed_response,
                    ),
                )
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)
                summary = admission.inspect(evidence)
                self.assertEqual(summary["state"], "withdrawn_evidence")
                self.assertTrue(summary["generation_dispatched"])
                with self.assertRaises(admission.AdmissionError):
                    admission.record_outcome(evidence, observation)


class OfficialRateContractTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.capacity_file = self.root / "capacity.json"
        self.profile_file = self.root / "profile"
        self.evidence = self.root / "evidence"
        self.profile_file.write_text(PROFILE + "\n", encoding="utf-8")
        os.chmod(self.profile_file, 0o600)

    def tearDown(self):
        self.temporary.cleanup()

    @staticmethod
    def write_json(path, value):
        path.write_text(json.dumps(value) + "\n", encoding="utf-8")
        os.chmod(path, 0o600)

    def initialize(
        self,
        budget=admission.DEFAULT_BUDGET_NANOUSD,
        max_output_tokens=admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
    ):
        with mock.patch.object(admission.time, "time", return_value=PROMO_TEST_EPOCH):
            return admission.initialize(
                self.evidence,
                self.capacity_file,
                self.profile_file,
                PLAN,
                SHA,
                SHA,
                budget,
                max_output_tokens,
            )

    def record_count(self):
        response_file = self.root / "count.json"
        self.write_json(response_file, count_observation(self.evidence))
        with mock.patch.object(admission.time, "time", return_value=PROMO_TEST_EPOCH):
            admission.claim_count(self.evidence)
            return admission.record_count(self.evidence, response_file)

    def test_official_rate_changes_exactly_at_2027_cutoff(self):
        promo = admission._expected_official_rate_at(admission.PROMO_END_EPOCH - 1)
        standard = admission._expected_official_rate_at(admission.PROMO_END_EPOCH)
        self.assertEqual(
            promo.tariff_schedule_id,
            admission.OFFICIAL_TARIFF_SCHEDULE_ID,
        )
        self.assertEqual(promo.input_token_limit, 1_048_576)
        self.assertEqual(promo.max_output_tokens, 65_536)
        self.assertEqual(
            (promo.input, promo.audio_input, promo.cached_input, promo.cached_audio_input),
            (750, 750, 75, 75),
        )
        self.assertEqual(promo.output, 3_750)
        self.assertEqual(promo.long_threshold, 2**64 - 1)
        self.assertEqual(
            (
                promo.long_input,
                promo.long_audio_input,
                promo.long_cached_input,
                promo.long_cached_audio_input,
                promo.long_output,
            ),
            (750, 750, 75, 75, 3_750),
        )
        self.assertEqual((promo.search_unit, promo.search), ("query", 14_000_000))
        self.assertEqual(
            (
                standard.input,
                standard.audio_input,
                standard.cached_input,
                standard.cached_audio_input,
                standard.output,
            ),
            (1_500, 1_500, 150, 150, 7_500),
        )
        self.assertEqual(
            (
                standard.long_input,
                standard.long_audio_input,
                standard.long_cached_input,
                standard.long_cached_audio_input,
                standard.long_output,
            ),
            (1_500, 1_500, 150, 150, 7_500),
        )
        self.assertEqual(
            admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
            1_048_576 * 1_500 + 256 * 7_500,
        )
        self.assertEqual(
            promo.upper_bound(2, admission.AUTHORIZED_MAX_OUTPUT_TOKENS, "fresh"),
            787_392_000,
        )
        self.assertEqual(
            admission._authorized_pre_dispatch_upper_bound(
                2,
                admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
            ),
            1_574_784_000,
        )

    def test_tampered_low_rate_rejected_before_any_evidence_or_request(self):
        tampered = official_rate_row(PROMO_TEST_EPOCH)
        tampered["rates"]["input_nanousd_per_token"] = "1"
        self.write_json(self.capacity_file, capacity(tampered))
        with self.assertRaises(admission.AdmissionError):
            self.initialize()
        self.assertFalse(self.evidence.exists())
        self.assertFalse((self.evidence / admission.COUNT_REQUEST).exists())
        self.assertFalse((self.evidence / admission.GENERATION_REQUEST).exists())

    def test_default_budget_withdraws_under_exact_official_full_context_bound(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        self.initialize()
        with self.assertRaises(admission.AdmissionError):
            self.record_count()
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_budget")
        self.assertEqual(
            int(summary["upper_bound_nanousd"]),
            admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
        )
        self.assertFalse((self.evidence / admission.GENERATION_REQUEST).exists())
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_exact_authorized_ceiling_permits_request_after_free_count(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        summary = self.initialize(admission.AUTHORIZED_STANDARD_CEILING_NANOUSD)
        self.assertEqual(summary["state"], "awaiting_count_tokens")
        summary = self.record_count()
        self.assertEqual(summary["state"], "counted")
        self.assertEqual(
            int(summary["upper_bound_nanousd"]),
            admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
        )
        self.assertTrue((self.evidence / admission.GENERATION_REQUEST).exists())
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_budget_or_output_different_from_authorized_contract_are_rejected(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        with self.assertRaises(admission.AdmissionError):
            self.initialize(admission.AUTHORIZED_STANDARD_CEILING_NANOUSD + 1)
        self.assertFalse(self.evidence.exists())

        with self.assertRaises(admission.AdmissionError):
            self.initialize(
                admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                admission.AUTHORIZED_MAX_OUTPUT_TOKENS + 1,
            )
        self.assertFalse(self.evidence.exists())

        with self.assertRaises(admission.AdmissionError):
            self.initialize(
                admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                admission.AUTHORIZED_MAX_OUTPUT_TOKENS - 1,
            )
        self.assertFalse(self.evidence.exists())

    def test_post_2027_contract_is_fail_closed_even_with_exact_standard_rate(self):
        epoch = admission.PROMO_END_EPOCH
        self.write_json(self.capacity_file, capacity(official_rate_row(epoch)))
        with mock.patch.object(admission.time, "time", return_value=epoch):
            with self.assertRaises(admission.AdmissionError):
                admission.initialize(
                    self.evidence,
                    self.capacity_file,
                    self.profile_file,
                    PLAN,
                    SHA,
                    SHA,
                    admission.AUTHORIZED_STANDARD_CEILING_NANOUSD,
                    admission.AUTHORIZED_MAX_OUTPUT_TOKENS,
                )
        self.assertFalse(self.evidence.exists())

    def test_promo_contract_expiring_after_init_withdraws_before_dispatch(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        self.initialize(admission.AUTHORIZED_STANDARD_CEILING_NANOUSD)
        response_file = self.root / "count.json"
        self.write_json(response_file, count_observation(self.evidence))
        with mock.patch.object(
            admission.time,
            "time",
            return_value=admission.PROMO_END_EPOCH,
        ):
            with self.assertRaises(admission.AdmissionError):
                admission.claim_count(self.evidence)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_contract_expired")
        self.assertFalse(summary["count_tokens_claimed"])
        self.assertFalse((self.evidence / admission.GENERATION_REQUEST).exists())
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_promo_contract_expiring_after_count_withdraws_before_arm(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        self.initialize(admission.AUTHORIZED_STANDARD_CEILING_NANOUSD)
        self.record_count()
        with mock.patch.object(
            admission.time,
            "time",
            return_value=admission.PROMO_END_EPOCH,
        ):
            with self.assertRaises(admission.AdmissionError):
                admission.arm_generation(self.evidence)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_contract_expired")
        self.assertTrue((self.evidence / admission.GENERATION_REQUEST).exists())
        self.assertFalse((self.evidence / admission.DISPATCH_FENCE).exists())

    def test_promo_contract_expiring_after_arm_withdraws_before_claim(self):
        self.write_json(self.capacity_file, capacity(official_rate_row(PROMO_TEST_EPOCH)))
        self.initialize(admission.AUTHORIZED_STANDARD_CEILING_NANOUSD)
        self.record_count()
        admission.arm_generation(self.evidence)
        with mock.patch.object(
            admission.time,
            "time",
            return_value=admission.PROMO_END_EPOCH,
        ):
            with self.assertRaises(admission.AdmissionError):
                admission.claim_generation(self.evidence)
        summary = admission.inspect(self.evidence)
        self.assertEqual(summary["state"], "withdrawn_contract_expired")
        self.assertTrue(summary["generation_armed"])
        self.assertFalse(summary["generation_claimed"])
        self.assertFalse((self.evidence / admission.OUTCOME_RECEIPT).exists())


if __name__ == "__main__":
    unittest.main()
