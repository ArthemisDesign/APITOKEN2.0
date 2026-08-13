#!/usr/bin/env python3
"""Offline unit tests for tools/suno_calibration/run_live.py.

The engine is never started and no network is touched: the plane client and the capacity
reader are fakes/patched. Mock tests prove the guards; only an owned live Suno subscription
proves the provider contract.

Run: python3 -m unittest tools.suno_calibration.test_run_live
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

from tools.suno_calibration import run_live


QUOTA_TS = 4_000_000_000
API_KEY = "sk-test-admin-key-9f8e7d6c"
CONTROL_KEY = "ctl-test-control-key-1a2b3c4d"
WINDOW = 2_592_000  # 30 days, the monthly window
SONG_NANO = 20_000_000  # 5 credits x $0.004
LYRICS_NANO = 200_000_000  # conservative 50-credit reserve x $0.004
# Default full matrix: 5 song legs + 1 lyrics leg.
PAID_LEGS_PER_FULL_RUN = 6


def subs_payload(
    profile_id: str = "owned-1",
    *,
    enabled: bool = True,
    authority: bool = True,
    pending: int = 0,
    dropped: int = 0,
    persistence: bool = True,
    profiles: int = 1,
    live: bool = True,
    routable: bool = True,
    quota_walled: bool = False,
    captcha_until: int | None = None,
    plan: str = "Pro",
    inflight: int = 0,
    quota_observed_at: int | None = QUOTA_TS - 100,
    monthly_limit: int | None = 2500,
    monthly_usage: int | None = 100,
    credits_left: int | None = 2400,
    spend_nano: int = 0,
    spend_milli: int = 0,
    unattributed_fraction: int = 0,
    has_calibration: bool = True,
    tracked_generations: int = 0,
    inflight_drains: int = 0,
    inflight_requests: int = 0,
    unattributed_settlements: int = 0,
    tariff_anomaly: int = 0,
) -> dict:
    calibration = None
    if has_calibration:
        calibration = [
            {
                "window_duration_secs": WINDOW,
                "samples": 1,
                "confidence_bp": 0,
                "capacity": {"current_nano": None, "low_nano": None, "high_nano": None},
                "remaining": None,
                "observed_spend_nano": str(spend_nano),
                "observed_spend_native_millicredits": spend_milli,
                "unattributed_fraction_units": unattributed_fraction,
                "last_measured_at": None,
                "estimator_version": 1,
            }
        ]
    return {
        "now": QUOTA_TS - 50,
        "enabled": enabled,
        "delivery": {
            "pending_events": pending,
            "dropped_events": dropped,
            "persistence_ok": persistence,
        },
        "calibration_authority_available": authority,
        "fleet": {
            "profiles": profiles,
            "live_profiles": 1,
            "available_profiles": 1,
            "inflight_requests": inflight_requests,
            "inflight_drains": inflight_drains,
            "tracked_generations": tracked_generations,
            "rate_limited_profiles": 0,
            "quota_walled_profiles": 0,
            "auth_cooling_profiles": 0,
            "captcha_cooling_profiles": 0,
            "transport_cooling_profiles": 0,
            "unattributed_settlements": unattributed_settlements,
            "tariff_anomaly": tariff_anomaly,
            "artifact_failures": 0,
        },
        "profiles": [
            {
                "id": profile_id,
                "plan": plan,
                "routable": routable,
                "live": live,
                "quota_walled": quota_walled,
                "cooling": {
                    "rate_limit_until": None,
                    "auth_until": None,
                    "captcha_until": captcha_until,
                    "transport_until": None,
                },
                "inflight": inflight,
                "quota": {
                    "observed_at": quota_observed_at,
                    "monthly_limit": monthly_limit,
                    "monthly_usage": monthly_usage,
                    "total_credits_left": credits_left,
                },
                "calibration": calibration,
            }
        ]
        if profiles
        else [],
    }


class FakePlane:
    """Scripted plane: creates return sequential generation ids (or raise), per-id statuses."""

    def __init__(self, default_status: str = "complete") -> None:
        self.creates: list[dict] = []
        self.create_errors: list[Exception] = []
        self.default_status = default_status
        self.statuses: dict[str, list[str]] = {}
        self.status_calls: dict[str, int] = {}

    def create(self, body: dict) -> dict:
        self.creates.append(body)
        if self.create_errors:
            raise self.create_errors.pop(0)
        return {
            "generation_id": f"gen-{len(self.creates)}",
            "operation": body["operation"],
            "status": "created",
        }

    def generation_status(self, generation_id: str) -> dict:
        calls = self.status_calls.get(generation_id, 0)
        self.status_calls[generation_id] = calls + 1
        statuses = self.statuses.get(generation_id, [self.default_status])
        status = statuses[min(calls, len(statuses) - 1)]
        return {
            "generation_id": generation_id,
            "operation": "song",
            "status": status,
            "artifacts": ["audio.mp3"] if status == "complete" else [],
            "created_at": QUOTA_TS - 10,
            "updated_at": QUOTA_TS,
            "error": None if status == "complete" else "generation_failed",
        }


class FakeCapacity:
    """Scripted /suno-subs reader: pops one payload per read, repeats the last."""

    def __init__(self, payloads: list[dict]) -> None:
        if not payloads:
            raise AssertionError("FakeCapacity needs at least one payload")
        self.payloads = list(payloads)
        self.reads = 0
        self.last: dict = self.payloads[0]

    def read(self) -> dict:
        self.reads += 1
        if self.payloads:
            self.last = self.payloads.pop(0)
        return self.last


def first_leg() -> run_live.Leg:
    return run_live.Leg("song:v4", "song", model="v4")


def make_runner(
    plane: FakePlane,
    capacity: FakeCapacity,
    budget_nano: int = run_live.MAX_BUDGET_NANO,
) -> run_live.Runner:
    return run_live.Runner(
        plane,
        capacity,
        run_live.Budget(budget_nano),
        "suno-cal-test",
        task_timeout=60,
        evidence_timeout=60,
        settle_delay=0,
        poll_interval=0,
    )


def full_run_payloads() -> list[dict]:
    """Baseline + (before, after) per paid leg. Song legs settle 5 credits ($0.02); the
    lyrics leg settles at the conservative 50-credit reserve ($0.20)."""
    payloads = [subs_payload()]
    spend = 0
    milli = 0
    usage = 100
    left = 2400
    for k in range(1, PAID_LEGS_PER_FULL_RUN + 1):
        payloads.append(
            subs_payload(
                tracked_generations=k - 1,
                spend_nano=spend,
                spend_milli=milli,
                monthly_usage=usage,
                credits_left=left,
            )
        )
        credits = 5 if k <= 5 else 50
        spend += credits * run_live.NANOUSD_PER_CREDIT
        milli += credits * 1_000
        usage += credits
        left -= credits
        payloads.append(
            subs_payload(
                tracked_generations=k,
                spend_nano=spend,
                spend_milli=milli,
                monthly_usage=usage,
                credits_left=left,
                quota_observed_at=QUOTA_TS,
            )
        )
    return payloads


class ParserAndGuardTests(unittest.TestCase):
    def test_usd_parser_is_integer_only_and_exact(self):
        self.assertEqual(run_live.usd_to_nano("0.02"), 20_000_000)
        self.assertEqual(run_live.usd_to_nano("1.00"), 1_000_000_000)
        for bad in ("", "abc", "1e-3", ".5", "-1", "1.0000000001"):
            with self.assertRaises(run_live.CalibrationError, msg=bad):
                run_live.usd_to_nano(bad)

    def test_budget_guard_blocks_dispatch_and_overcharge(self):
        budget = run_live.Budget(100)
        budget.require_room(60)
        budget.charge(60)
        with self.assertRaises(run_live.CalibrationError):
            budget.require_room(41)
        with self.assertRaises(run_live.CalibrationError):
            budget.charge(41)
        with self.assertRaises(run_live.CalibrationError):
            budget.charge(-1)

    def test_worst_case_hold_is_accounted_against_the_budget(self):
        budget = run_live.Budget(100)
        budget.hold(70)
        with self.assertRaises(run_live.CalibrationError):
            budget.require_room(31)
        budget.charge(30)
        self.assertEqual(budget.committed_nano(), 100)

    def test_profile_and_generation_id_validation(self):
        self.assertEqual(run_live.validate_profile_id("owned-1_ok"), "owned-1_ok")
        for bad in ("", "-x", "a b", "a/b", "a" * 129, 7):
            with self.assertRaises(run_live.CalibrationError, msg=str(bad)):
                run_live.validate_profile_id(bad)
        self.assertEqual(run_live.validate_generation_id("gen-abc:1"), "gen-abc:1")
        for bad in ("", "a/b", "a?b", "a b"):
            with self.assertRaises(run_live.CalibrationError, msg=bad):
                run_live.validate_generation_id(bad)

    def test_upstream_id_validation(self):
        self.assertEqual(run_live.validate_upstream_id("clip-1_AB", "--song-id"), "clip-1_AB")
        for bad in ("", "a/b", "a b", "a" * 129):
            with self.assertRaises(run_live.CalibrationError, msg=bad):
                run_live.validate_upstream_id(bad, "--song-id")


class PricingTests(unittest.TestCase):
    """Mirrors crates/metering/src/suno.rs and OperationKind::reserve_credits."""

    def test_song_is_the_published_flat_five_credits_per_paid_model(self):
        for model in run_live.PAID_MODELS:
            leg = run_live.Leg(f"song:{model}", "song", model=model)
            self.assertEqual(run_live.leg_reserve_credits(leg), 5)
            self.assertEqual(run_live.leg_upper_bound_nano(leg), SONG_NANO)

    def test_conservative_reserve_for_unpriced_operations(self):
        for operation in ("extend", "lyrics", "stems"):
            leg = run_live.Leg(operation, operation, song_id="s-1", continue_clip_id="c-1")
            self.assertEqual(run_live.leg_reserve_credits(leg), 50)
            self.assertEqual(run_live.leg_upper_bound_nano(leg), LYRICS_NANO)

    def test_unknown_model_or_operation_fails_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.leg_reserve_credits(run_live.Leg("song:v6", "song", model="v6"))
        with self.assertRaises(run_live.CalibrationError):
            run_live.leg_reserve_credits(run_live.Leg("covers", "covers"))


class MatrixTests(unittest.TestCase):
    def test_default_matrix_covers_every_paid_model_plus_lyrics(self):
        legs, unavailable = run_live.build_legs(list(run_live.PAID_MODELS), None, None)
        names = [leg.name for leg in legs]
        self.assertEqual(len(names), len(set(names)))
        for model in run_live.PAID_MODELS:
            self.assertIn(f"song:{model}", names)
        self.assertIn("lyrics", names)
        capabilities = {entry["capability"] for entry in unavailable}
        self.assertEqual(capabilities, {"extend", "stems"})
        self.assertTrue(all(entry["skipped_before_dispatch"] for entry in unavailable))

    def test_operator_inputs_enable_extend_and_stems(self):
        legs, unavailable = run_live.build_legs(["v5"], "clip-9", "song-9")
        names = [leg.name for leg in legs]
        self.assertIn("extend", names)
        self.assertIn("stems", names)
        self.assertEqual(unavailable, [])

    def test_unreviewed_model_fails_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.build_legs(["v4.5-all"], None, None)

    def test_bodies_match_the_admitted_wire_shapes(self):
        run_id = "suno-cal-test"
        song = body_song = run_live.body_for_leg(first_leg(), run_id)
        self.assertEqual(
            song,
            {
                "operation": "song",
                "model": "v4",
                "make_instrumental": True,
                "prompt": song["prompt"],
            },
        )
        self.assertEqual(
            run_live.body_for_leg(run_live.Leg("extend", "extend", continue_clip_id="c-1"), run_id),
            {"operation": "extend", "continue_clip_id": "c-1"},
        )
        self.assertEqual(
            run_live.body_for_leg(run_live.Leg("stems", "stems", song_id="s-1"), run_id),
            {"operation": "stems", "song_id": "s-1"},
        )


class HealthGateTests(unittest.TestCase):
    def test_healthy_payload_passes(self):
        payload = subs_payload()
        run_live.require_healthy_plane(payload)
        view = run_live.profile_view(payload, "owned-1", QUOTA_TS - 50)
        self.assertEqual(view["plan"], "Pro")
        self.assertEqual(view["calibration_rows"][WINDOW]["spend_nano"], 0)

    def test_delivery_and_authority_fail_closed(self):
        for kwargs in (
            {"enabled": False},
            {"authority": False},
            {"pending": 1},
            {"dropped": 1},
            {"persistence": False},
        ):
            with self.assertRaises(run_live.CalibrationError, msg=str(kwargs)):
                run_live.require_healthy_plane(subs_payload(**kwargs))

    def test_single_profile_guard(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.profile_view(subs_payload(profiles=2), "owned-1", 0)
        with self.assertRaises(run_live.CalibrationError):
            run_live.profile_view(subs_payload(profile_id="other"), "owned-1", 0)

    def test_dead_walled_cooling_and_plan_fail_closed(self):
        for kwargs in (
            {"live": False},
            {"routable": False},
            {"quota_walled": True},
            {"captcha_until": QUOTA_TS},
            {"plan": "Free"},
        ):
            with self.assertRaises(run_live.CalibrationError, msg=str(kwargs)):
                run_live.profile_view(subs_payload(**kwargs), "owned-1", QUOTA_TS - 50)

    def test_inflight_work_before_dispatch_blocks_the_leg(self):
        runner = make_runner(FakePlane(), FakeCapacity([subs_payload(inflight=1)]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")


class SpendDeltaTests(unittest.TestCase):
    def rows(self, nano: int, milli: int = 0, unattributed: int = 0) -> dict:
        return {
            WINDOW: {
                "spend_nano": nano,
                "spend_millicredits": milli,
                "unattributed_fraction_units": unattributed,
            }
        }

    def test_common_delta_across_rows(self):
        nano, milli, unattributed = run_live.spend_delta(self.rows(5), self.rows(25, 20, 3))
        self.assertEqual((nano, milli, unattributed), (20, 20, 3))

    def test_missing_changed_or_disagreeing_rows_fail_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.spend_delta({}, self.rows(1))
        with self.assertRaises(run_live.CalibrationError):
            run_live.spend_delta(self.rows(1), {})
        changed = {86_400: self.rows(1)[WINDOW]}
        with self.assertRaises(run_live.CalibrationError):
            run_live.spend_delta(self.rows(1), changed)
        before = {**self.rows(1), 86_400: self.rows(1)[WINDOW]}
        after = {WINDOW: self.rows(3)[WINDOW], 86_400: self.rows(9)[WINDOW]}
        with self.assertRaises(run_live.CalibrationError):
            run_live.spend_delta(before, after)
        with self.assertRaises(run_live.CalibrationError):
            run_live.spend_delta(self.rows(10), self.rows(5))


class ExecuteLegTests(unittest.TestCase):
    def test_happy_path_charges_the_exact_attributed_delta(self):
        before = subs_payload(tracked_generations=3, monthly_usage=100, credits_left=2400)
        after = subs_payload(
            tracked_generations=4,
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            monthly_usage=105,
            credits_left=2395,
            quota_observed_at=QUOTA_TS,
        )
        plane = FakePlane()
        runner = make_runner(plane, FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "exact")
        self.assertEqual(record["settlement"], "attributed")
        self.assertEqual(record["settled_nanousd"], str(SONG_NANO))
        self.assertEqual(record["settled_native_millicredits"], "5000")
        self.assertEqual(record["monthly_usage_delta"], 5)
        self.assertEqual(record["credits_left_drawdown"], 5)
        self.assertEqual(runner.budget.spent_nano, SONG_NANO)
        self.assertEqual(len(plane.creates), 1)

    def test_reserve_fallback_settlement_is_recorded(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(
            tracked_generations=4,
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
            unattributed_settlements=1,
            unattributed_fraction=200_000,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["settlement"], "reserve-fallback")
        self.assertEqual(record["unattributed_fraction_delta"], 200_000)

    def test_no_free_quota_preflight_blocks_paid_traffic(self):
        capacity = FakeCapacity([subs_payload(quota_observed_at=None)])
        runner = make_runner(FakePlane(), capacity)
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_foreign_tracked_generation_makes_attribution_ambiguous(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(
            tracked_generations=5,  # a foreign generation raced the leg
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "ambiguous")
        self.assertTrue(record["foreign_traffic"])
        # The money moved and is charged even when attribution is ambiguous.
        self.assertEqual(runner.budget.spent_nano, SONG_NANO)

    def test_foreign_inflight_at_settle_makes_attribution_ambiguous(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(
            tracked_generations=4,
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
            inflight=1,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "ambiguous")

    def test_tariff_anomaly_counter_advance_fails_closed(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(
            tracked_generations=4,
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
            tariff_anomaly=1,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_spend_above_the_preflight_bound_fails_closed(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(
            tracked_generations=4,
            spend_nano=SONG_NANO + 1,  # one nanoUSD above the 5-credit bound
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_complete_without_settled_spend_fails_closed(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(tracked_generations=4, quota_observed_at=QUOTA_TS)
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_error_finalization_fails_closed(self):
        before = subs_payload(tracked_generations=3)
        after = subs_payload(tracked_generations=4, quota_observed_at=QUOTA_TS)
        runner = make_runner(
            FakePlane(default_status="error"), FakeCapacity([before, after])
        )
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_paid_create_is_never_retried_after_transport_ambiguity(self):
        plane = FakePlane()
        plane.create_errors = [run_live.TransportFailureError("connection reset")]
        runner = make_runner(plane, FakeCapacity([subs_payload()]))
        with self.assertRaises(run_live.PaidLegError) as caught:
            runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(
            caught.exception.upper_bound_nano, run_live.leg_upper_bound_nano(first_leg())
        )
        self.assertEqual(len(plane.creates), 1)

    def test_typed_create_error_holds_nothing(self):
        plane = FakePlane()
        plane.create_errors = [
            run_live.HttpCalibrationError(
                "/v1/audio/generations", 400, "suno_operation_unknown"
            )
        ]
        runner = make_runner(plane, FakeCapacity([subs_payload()]))
        with self.assertRaises(run_live.HttpCalibrationError):
            runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(runner.budget.committed_nano(), 0)

    def test_unresolved_settlement_is_not_zero(self):
        stuck = subs_payload(tracked_generations=4, quota_observed_at=1)
        runner = make_runner(
            FakePlane(), FakeCapacity([subs_payload(tracked_generations=3), stuck])
        )
        runner.evidence_timeout = 0  # the settle deadline is already past
        with self.assertRaises(run_live.PostSpendPollError):
            runner.execute_leg(first_leg(), "owned-1")


class MainCase(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.report = str(Path(self.tmp.name) / "report.json")
        self.checkpoint = str(Path(self.tmp.name) / "checkpoint.json")
        patcher = mock.patch.dict(
            os.environ,
            {"APITOKEN_API_KEY": API_KEY, "CLAUDE_API_CONTROL_KEY": CONTROL_KEY},
            clear=False,
        )
        patcher.start()
        self.addCleanup(patcher.stop)
        sleep = mock.patch("time.sleep", lambda *_a, **_k: None)
        sleep.start()
        self.addCleanup(sleep.stop)

    def argv(self, *extra: str) -> list[str]:
        return [
            "--execute",
            "--profile",
            "owned-1",
            "--budget-usd",
            "0.50",
            "--models",
            "v4",
            "--report",
            self.report,
            "--checkpoint",
            self.checkpoint,
            *extra,
        ]

    def run_main(self, argv: list[str], capacity_payloads: list[dict], plane: FakePlane):
        capacity = FakeCapacity(capacity_payloads)
        with (
            mock.patch.object(run_live, "PlaneClient", return_value=plane),
            mock.patch.object(run_live, "CapacityReader", return_value=capacity),
        ):
            code = run_live.main(argv)
        return code, capacity

    def small_run_payloads(self) -> list[dict]:
        """--models v4: one song leg (5 credits) + the lyrics leg (50-credit reserve)."""
        return [
            subs_payload(),
            subs_payload(),
            subs_payload(
                tracked_generations=1,
                spend_nano=SONG_NANO,
                spend_milli=5_000,
                monthly_usage=105,
                credits_left=2395,
                quota_observed_at=QUOTA_TS,
            ),
            subs_payload(
                tracked_generations=1,
                spend_nano=SONG_NANO,
                spend_milli=5_000,
                monthly_usage=105,
                credits_left=2395,
            ),
            subs_payload(
                tracked_generations=2,
                spend_nano=SONG_NANO + LYRICS_NANO,
                spend_milli=55_000,
                monthly_usage=155,
                credits_left=2345,
                quota_observed_at=QUOTA_TS,
            ),
        ]


class DryRunTests(MainCase):
    def test_dry_run_prints_a_plan_and_sends_nothing(self):
        with mock.patch.dict(os.environ, {"CLAUDE_API_CONTROL_KEY": ""}, clear=False):
            with (
                mock.patch.object(run_live, "PlaneClient") as client_cls,
                mock.patch.object(run_live, "CapacityReader") as capacity_cls,
                contextlib.redirect_stdout(io.StringIO()) as out,
            ):
                code = run_live.main(["--profile", "owned-1"])
        self.assertEqual(code, 0)
        plan = json.loads(out.getvalue())
        self.assertEqual(plan["schema"], "suno-live-calibration-plan/v1")
        self.assertEqual(plan["paid_requests"], 0)
        self.assertIsNone(plan["budget_nanousd"])
        expected_legs, _ = run_live.build_legs(list(run_live.PAID_MODELS), None, None)
        self.assertEqual(
            int(plan["total_worst_case_nanousd"]),
            sum(run_live.leg_upper_bound_nano(leg) for leg in expected_legs),
        )
        # 5 songs x 5 credits + lyrics at the 50-credit reserve = 75 credits = $0.30.
        self.assertEqual(int(plan["total_worst_case_nanousd"]), 300_000_000)
        client_cls.assert_not_called()
        capacity_cls.assert_not_called()

    def test_dry_run_with_a_control_key_fetches_only_the_free_baseline(self):
        capacity = FakeCapacity([subs_payload()])
        with (
            mock.patch.object(run_live, "CapacityReader", return_value=capacity),
            contextlib.redirect_stdout(io.StringIO()) as out,
        ):
            code = run_live.main(["--profile", "owned-1"])
        self.assertEqual(code, 0)
        plan = json.loads(out.getvalue())
        self.assertEqual(plan["baseline"]["enabled"], True)
        self.assertEqual(capacity.reads, 1)

    def test_execute_requires_an_explicit_budget(self):
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--execute", "--profile", "owned-1"])

    def test_budget_hard_ceiling_cannot_be_raised_by_cli(self):
        with self.assertRaises(SystemExit):
            run_live.parse_args(
                ["--execute", "--profile", "owned-1", "--budget-usd", "1.01"]
            )
        args = run_live.parse_args(
            ["--execute", "--profile", "owned-1", "--budget-usd", "1.00"]
        )
        self.assertEqual(args.budget_usd, "1.00")

    def test_execute_requires_profile(self):
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--execute", "--budget-usd", "0.50"])


class ExecuteFlowTests(MainCase):
    def test_happy_path_run_is_complete_and_reports_everything(self):
        plane = FakePlane()
        code, _ = self.run_main(self.argv(), self.small_run_payloads(), plane)
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertTrue(report["complete"])
        self.assertEqual(report["spent_nanousd"], str(SONG_NANO + LYRICS_NANO))
        self.assertEqual(report["leg_status"]["song:v4"], "ok")
        self.assertEqual(report["leg_status"]["lyrics"], "ok")
        self.assertEqual(report["records"][1]["reserve_credits"], 50)
        self.assertEqual(report["coverage"]["pending_legs"], [])
        capabilities = {entry["capability"] for entry in report["unavailable_capabilities"]}
        self.assertEqual(capabilities, {"extend", "stems"})
        self.assertEqual(len(plane.creates), 2)

    def test_budget_guard_stops_the_matrix_partway(self):
        # 0.10 USD: the song leg fits (bound $0.02), the lyrics leg's $0.20 reserve does not.
        argv = [
            "--execute", "--profile", "owned-1", "--budget-usd", "0.10",
            "--models", "v4",
            "--report", self.report, "--checkpoint", self.checkpoint,
        ]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(argv, self.small_run_payloads(), FakePlane())
        report = json.loads(Path(self.report).read_text())
        self.assertFalse(report["complete"])
        self.assertEqual(report["spent_nanousd"], str(SONG_NANO))
        self.assertIn("budget guard stopped", report["failure"])
        self.assertEqual(report["coverage"]["pending_legs"], [])
        self.assertEqual(report["leg_status"]["lyrics"], "failed")

    def test_ambiguous_attribution_is_recorded_and_stops_the_matrix(self):
        before = subs_payload(tracked_generations=3)
        raced = subs_payload(
            tracked_generations=5,  # a foreign generation raced the leg
            spend_nano=SONG_NANO,
            spend_milli=5_000,
            quota_observed_at=QUOTA_TS,
        )
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [before, before, raced, raced], FakePlane())
        report = json.loads(Path(self.report).read_text())
        self.assertFalse(report["complete"])
        self.assertEqual(report["records"][0]["attribution"], "ambiguous")
        self.assertIn("foreign traffic", report["failure"])

    def test_held_leg_is_never_resent_even_on_resume(self):
        plane = FakePlane()
        plane.create_errors = [run_live.TransportFailureError("connection reset")]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], plane)
        self.assertEqual(len(plane.creates), 1)
        checkpoint = json.loads(Path(self.checkpoint).read_text())
        self.assertEqual(checkpoint["leg_status"]["song:v4"], "held-ambiguous")
        self.assertEqual(checkpoint["held_nano"], SONG_NANO)
        # Close out every other leg in the checkpoint: the resume then has nothing to send,
        # which is exactly the proof that the held leg is not repeated.
        legs, _ = run_live.build_legs(["v4"], None, None)
        for leg in legs:
            checkpoint["leg_status"].setdefault(leg.name, "ok")
        Path(self.checkpoint).write_text(json.dumps(checkpoint))
        plane2 = FakePlane()
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv("--resume", self.checkpoint), [subs_payload()], plane2)
        self.assertEqual(plane2.creates, [])

    def test_typed_400_on_optional_legs_records_unavailability_and_continues(self):
        plane = FakePlane()
        original_create = plane.create

        def create_with_refusals(body: dict) -> dict:
            if body["operation"] == "lyrics":
                plane.creates.append(body)
                raise run_live.HttpCalibrationError(
                    "/v1/audio/generations", 400, "upstream refused lyrics"
                )
            return original_create(body)

        plane.create = create_with_refusals
        # Reads: baseline, song before/after, one before-read for the refused lyrics leg.
        payloads = self.small_run_payloads()[:3] + [
            subs_payload(
                tracked_generations=1,
                spend_nano=SONG_NANO,
                spend_milli=5_000,
                monthly_usage=105,
                credits_left=2395,
                quota_observed_at=QUOTA_TS,
            )
        ]
        code, _ = self.run_main(self.argv(), payloads, plane)
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["leg_status"]["song:v4"], "ok")
        self.assertEqual(report["leg_status"]["lyrics"], "unavailable")
        lyrics = [
            entry
            for entry in report["unavailable_capabilities"]
            if entry["capability"] == "lyrics"
        ]
        self.assertTrue(lyrics)
        self.assertFalse(lyrics[0]["blocking"])
        self.assertTrue(report["complete"])

    def test_required_leg_400_is_blocking(self):
        plane = FakePlane()
        plane.create_errors = [
            run_live.HttpCalibrationError("/v1/audio/generations", 400, "suno_model_unknown")
        ]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], plane)
        report = json.loads(Path(self.report).read_text())
        blocking = [entry for entry in report["unavailable_capabilities"] if entry["blocking"]]
        self.assertTrue(blocking)
        self.assertFalse(report["complete"])

    def test_session_and_rate_walls_stop_with_profile_stops(self):
        for status in (401, 403, 429):
            plane = FakePlane()
            plane.create_errors = [
                run_live.HttpCalibrationError("/v1/audio/generations", status, "wall")
            ]
            with self.assertRaises(run_live.CalibrationError):
                self.run_main(self.argv(), [subs_payload()], plane)
            report = json.loads(Path(self.report).read_text())
            self.assertEqual(report["stops"][0]["scope"], "profile:owned-1")
            self.assertFalse(report["complete"])
            Path(self.report).unlink()
            Path(self.checkpoint).unlink()

    def test_baseline_health_gate_blocks_before_any_paid_traffic(self):
        plane = FakePlane()
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload(pending=2)], plane)
        self.assertEqual(plane.creates, [])


class SecretContainmentTests(MainCase):
    def test_keys_never_reach_the_report_checkpoint_or_output(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code, _ = self.run_main(self.argv(), self.small_run_payloads(), FakePlane())
        self.assertEqual(code, 0)
        blob = (
            Path(self.report).read_text()
            + Path(self.checkpoint).read_text()
            + out.getvalue()
        )
        self.assertNotIn(API_KEY, blob)
        self.assertNotIn(CONTROL_KEY, blob)

    def test_api_key_is_redacted_from_typed_error_details(self):
        client = run_live.PlaneClient("http://127.0.0.1:8787", API_KEY, 10)
        error = urllib.error.HTTPError(
            "http://x", 400, "bad", None, io.BytesIO(f"key {API_KEY} rejected".encode())
        )
        with mock.patch("urllib.request.urlopen", side_effect=error):
            with self.assertRaises(run_live.HttpCalibrationError) as caught:
                client.create({"operation": "song"})
        self.assertNotIn(API_KEY, caught.exception.detail)
        self.assertIn("***", caught.exception.detail)


class ResumeTests(MainCase):
    def test_resume_skips_completed_legs(self):
        code, _ = self.run_main(self.argv(), self.small_run_payloads(), FakePlane())
        self.assertEqual(code, 0)
        plane2 = FakePlane()
        code, _ = self.run_main(
            self.argv("--resume", self.checkpoint), [subs_payload()], plane2
        )
        self.assertEqual(code, 0)
        self.assertEqual(plane2.creates, [])

    def test_resume_rejects_a_mismatched_run_identity(self):
        code, _ = self.run_main(self.argv(), self.small_run_payloads(), FakePlane())
        self.assertEqual(code, 0)
        mismatched = [
            "--execute", "--profile", "owned-1", "--budget-usd", "0.90",
            "--models", "v4", "--resume", self.checkpoint,
            "--report", self.report, "--checkpoint", self.checkpoint,
        ]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(mismatched, [subs_payload()], FakePlane())

    def test_fresh_run_refuses_to_overwrite_an_existing_checkpoint(self):
        Path(self.checkpoint).write_text("{}")
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], FakePlane())

    def test_incomplete_checkpoint_fails_closed(self):
        partial = self.checkpoint + ".partial"
        Path(partial).write_text(
            json.dumps({"schema": run_live.CHECKPOINT_SCHEMA, "run_id": "x"})
        )
        with self.assertRaises(run_live.CalibrationError):
            run_live.load_resume(
                partial,
                "owned-1",
                "http://127.0.0.1:8787",
                1,
                run_live.matrix_identity(["v4"], None, None),
            )


class ReportCompletenessTests(MainCase):
    def test_incomplete_run_still_writes_a_full_report_shape(self):
        plane = FakePlane()
        plane.create_errors = [run_live.TransportFailureError("connection reset")]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], plane)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["schema"], "suno-live-calibration/v1")
        self.assertFalse(report["complete"])
        self.assertIsNotNone(report["failure"])
        self.assertEqual(report["held_nanousd"], str(SONG_NANO))
        for key in (
            "run_id",
            "budget_nanousd",
            "spent_nanousd",
            "records",
            "coverage",
            "unavailable_capabilities",
            "stops",
            "baseline_observations",
            "final_observations",
        ):
            self.assertIn(key, report)
        self.assertTrue(report["coverage"]["pending_legs"])


if __name__ == "__main__":
    unittest.main()
