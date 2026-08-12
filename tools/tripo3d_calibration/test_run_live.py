#!/usr/bin/env python3
"""Offline unit tests for tools/tripo3d_calibration/run_live.py.

The engine is never started and no network is touched: the plane client and the capacity
reader are fakes/patched. Mock tests prove the guards; only an owned live Tripo3D account
proves the provider contract.

Run: python3 -m unittest tools.tripo3d_calibration.test_run_live
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

from tools.tripo3d_calibration import run_live


BALANCE_TS = 4_000_000_000
API_KEY = "sk-test-admin-key-9f8e7d6c"
CONTROL_KEY = "ctl-test-control-key-1a2b3c4d"
# Paid legs per scripted full run: 1 text version leg + 7 option legs; the refund probe
# follows and must settle zero.
PAID_LEGS_PER_FULL_RUN = 8


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
    walled: bool = False,
    cooling_until: int | None = None,
    inflight: int = 0,
    cohort: str = "api-50",
    spend_nano: int = 0,
    spend_milli: int = 0,
    has_calibration: bool = True,
    balance_observed_at: int | None = BALANCE_TS - 100,
    balance_micro: int | None = 50_000_000,
    frozen_micro: int | None = 0,
    tracked_tasks: int = 0,
    inflight_drains: int = 0,
    inflight_requests: int = 0,
    tariff_anomaly: int = 0,
    missing_credit: int = 0,
    undocumented_final: int = 0,
) -> dict:
    calibration = None
    if has_calibration:
        calibration = {
            "cohort": cohort,
            "samples": 1,
            "confidence_bp": 0,
            "capacity": {"current_nano": None, "low_nano": None, "high_nano": None},
            "remaining": None,
            "observed_spend_nano": str(spend_nano),
            "observed_spend_native_millicredits": spend_milli,
            "last_measured_at": None,
            "estimator_version": 1,
        }
    return {
        "now": BALANCE_TS - 50,
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
            "tracked_tasks": tracked_tasks,
            "rate_limited_profiles": 0,
            "balance_walled_profiles": 0,
            "auth_cooling_profiles": 0,
            "transport_cooling_profiles": 0,
            "missing_consumed_credit": missing_credit,
            "tariff_anomaly": tariff_anomaly,
            "undocumented_final": undocumented_final,
            "artifact_failures": 0,
        },
        "profiles": [
            {
                "id": profile_id,
                "cohort": cohort,
                "live": live,
                "balance_walled": walled,
                "cooling": {
                    "rate_limit_until": cooling_until,
                    "auth_until": None,
                    "transport_until": None,
                },
                "inflight": inflight,
                "balance": {
                    "observed_at": balance_observed_at,
                    "balance_raw": "50.000" if balance_micro else "0.000",
                    "frozen_raw": "0.000",
                    "balance_micro_units": balance_micro,
                    "frozen_micro_units": frozen_micro,
                },
                "calibration": calibration,
            }
        ]
        if profiles
        else [],
    }


class FakePlane:
    """Scripted plane: creates return sequential task ids (or raise), per-task statuses."""

    def __init__(self, default_status: str = "success") -> None:
        self.creates: list[dict] = []
        self.create_errors: list[Exception] = []
        self.default_status = default_status
        self.task_statuses: dict[str, list[str]] = {}
        self.status_calls: dict[str, int] = {}

    def create(self, body: dict) -> dict:
        self.creates.append(body)
        if self.create_errors:
            raise self.create_errors.pop(0)
        return {
            "task_id": f"task-{len(self.creates)}",
            "type": body["type"],
            "status": "created",
        }

    def task_status(self, task_id: str) -> dict:
        calls = self.status_calls.get(task_id, 0)
        self.status_calls[task_id] = calls + 1
        statuses = self.task_statuses.get(task_id, [self.default_status])
        status = statuses[min(calls, len(statuses) - 1)]
        return {
            "task_id": task_id,
            "type": "text_to_model",
            "status": status,
            "progress": 100,
            "artifacts": ["model.glb"] if status == "success" else [],
            "created_at": BALANCE_TS - 10,
            "updated_at": BALANCE_TS,
            "error": None if status == "success" else "task_failed",
        }


class FakeCapacity:
    """Scripted /tripo3d-subs reader: pops one payload per read, repeats the last."""

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
    return run_live.Leg("text_to_model:v2.5-20250123", "text_to_model", "v2.5-20250123")


def refund_leg() -> run_live.Leg:
    return run_live.Leg(
        "refund-probe:image_to_model",
        "image_to_model",
        "v2.5-20250123",
        image_url="https://calibration.invalid/refund-probe.png",
        expect_failure=True,
        required=False,
    )


def full_run_payloads(spend_per_leg_nano: int = 50_000_000) -> list[dict]:
    """Baseline + (before, after) per paid leg with an advancing ledger, then the refund
    probe's zero-delta pair. 5 credits per leg (50_000_000 nano, 5_000 millicredits)."""
    payloads = [subs_payload()]
    spend = 0
    milli = 0
    for k in range(1, PAID_LEGS_PER_FULL_RUN + 1):
        payloads.append(
            subs_payload(tracked_tasks=k - 1, spend_nano=spend, spend_milli=milli)
        )
        spend += spend_per_leg_nano
        milli += 5_000
        payloads.append(
            subs_payload(
                tracked_tasks=k,
                spend_nano=spend,
                spend_milli=milli,
                balance_observed_at=BALANCE_TS,
            )
        )
    # Refund probe: one new tracked task, zero settled spend, balance observed post-turn.
    payloads.append(subs_payload(tracked_tasks=8, spend_nano=spend, spend_milli=milli))
    payloads.append(
        subs_payload(
            tracked_tasks=9,
            spend_nano=spend,
            spend_milli=milli,
            balance_observed_at=BALANCE_TS,
        )
    )
    return payloads


def full_run_plane() -> FakePlane:
    plane = FakePlane()
    # The refund probe is the last create of the full matrix.
    plane.task_statuses[f"task-{PAID_LEGS_PER_FULL_RUN + 1}"] = ["failed"]
    return plane


def make_runner(
    plane: FakePlane,
    capacity: FakeCapacity,
    budget_nano: int = run_live.MAX_BUDGET_NANO,
) -> run_live.Runner:
    return run_live.Runner(
        plane,
        capacity,
        run_live.Budget(budget_nano),
        "tripo3d-cal-test",
        task_timeout=60,
        evidence_timeout=60,
        settle_delay=0,
        poll_interval=0,
    )


class ParserAndGuardTests(unittest.TestCase):
    def test_usd_parser_is_integer_only_and_exact(self):
        self.assertEqual(run_live.usd_to_nano("0.05"), 50_000_000)
        self.assertEqual(run_live.usd_to_nano("5.00"), 5_000_000_000)
        self.assertEqual(run_live.usd_to_nano("2"), 2_000_000_000)
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

    def test_profile_and_task_id_validation(self):
        self.assertEqual(run_live.validate_profile_id("owned-1_ok"), "owned-1_ok")
        for bad in ("", "-x", "a b", "a/b", "a" * 129, 7):
            with self.assertRaises(run_live.CalibrationError, msg=str(bad)):
                run_live.validate_profile_id(bad)
        self.assertEqual(run_live.validate_task_id("t3-abc:1"), "t3-abc:1")
        for bad in ("", "a/b", "a?b", "a b", "a" * 129):
            with self.assertRaises(run_live.CalibrationError, msg=bad):
                run_live.validate_task_id(bad)

    def test_url_validation_rejects_credentials_and_whitespace(self):
        self.assertEqual(
            run_live.validate_url("http://127.0.0.1:8787/", "--api-url"),
            "http://127.0.0.1:8787",
        )
        for bad in ("ftp://x", "https://user:pw@host", "http://a b", ""):
            with self.assertRaises(run_live.CalibrationError, msg=bad):
                run_live.validate_url(bad, "--api-url")


class PricingTests(unittest.TestCase):
    """Exact vectors mirrored from crates/metering/src/tripo3d.rs (manifest §5.1)."""

    def price(self, leg: run_live.Leg) -> int:
        return run_live.leg_reserve_credits(leg)

    def test_base_prices_per_tier(self):
        self.assertEqual(self.price(first_leg()), 10)
        self.assertEqual(self.price(run_live.Leg("p1", "text_to_model", "P1-20260311")), 30)
        self.assertEqual(
            self.price(run_live.Leg("p1t", "text_to_model", "P1-20260311", texture=True)), 40
        )
        self.assertEqual(
            self.price(run_live.Leg("v14", "text_to_model", "v1.4-20240625")), 20
        )
        self.assertEqual(
            self.price(
                run_live.Leg(
                    "img", "image_to_model", "v2.5-20250123", image_url="https://x/y.png"
                )
            ),
            20,
        )

    def test_option_surcharges_stack_on_the_standard_base(self):
        self.assertEqual(
            self.price(run_live.Leg("t", "text_to_model", "v2.5-20250123", texture=True)), 20
        )
        self.assertEqual(
            self.price(
                run_live.Leg(
                    "td", "text_to_model", "v2.5-20250123", texture=True,
                    texture_quality="detailed",
                )
            ),
            30,
        )
        self.assertEqual(
            self.price(
                run_live.Leg(
                    "te", "text_to_model", "v2.5-20250123", texture=True,
                    texture_quality="extreme",
                )
            ),
            40,
        )
        self.assertEqual(
            self.price(
                run_live.Leg("slp", "text_to_model", "v2.5-20250123", smart_low_poly=True)
            ),
            20,
        )
        self.assertEqual(
            self.price(run_live.Leg("q", "text_to_model", "v2.5-20250123", quad=True)), 15
        )
        self.assertEqual(
            self.price(
                run_live.Leg("gp", "text_to_model", "v2.5-20250123", generate_parts=True)
            ),
            30,
        )
        self.assertEqual(
            self.price(
                run_live.Leg(
                    "gd", "text_to_model", "v2.5-20250123", geometry_quality="detailed"
                )
            ),
            30,
        )

    def test_unpriceable_combinations_fail_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            self.price(run_live.Leg("p1s", "text_to_model", "P1-20260311", smart_low_poly=True))
        with self.assertRaises(run_live.CalibrationError):
            self.price(run_live.Leg("v14q", "text_to_model", "v1.4-20240625", quad=True))
        with self.assertRaises(run_live.CalibrationError):
            self.price(
                run_live.Leg("qx", "text_to_model", "v2.5-20250123", texture_quality="extreme")
            )
        with self.assertRaises(run_live.CalibrationError):
            self.price(run_live.Leg("unknown", "text_to_model", "v9.9-29990101"))

    def test_texture_model_flat_prices(self):
        for quality, credits in (("standard", 10), ("detailed", 20), ("extreme", 30)):
            leg = run_live.Leg(
                f"tm:{quality}", "texture_model", texture_quality=quality,
                original_model_task_id="upstream-1",
            )
            self.assertEqual(self.price(leg), credits)

    def test_upper_bound_is_the_fixed_official_rate(self):
        self.assertEqual(run_live.leg_upper_bound_nano(first_leg()), 10 * 10_000_000)


class MatrixTests(unittest.TestCase):
    def test_default_matrix_covers_versions_options_and_refund(self):
        legs, unavailable = run_live.build_legs(list(run_live.VERSIONS_TO_MODEL), None, None)
        names = [leg.name for leg in legs]
        self.assertEqual(len(names), len(set(names)))
        for version in run_live.VERSIONS_TO_MODEL:
            self.assertIn(f"text_to_model:{version}", names)
        self.assertIn("refund-probe:image_to_model", names)
        # The texture/quality option legs ride the cheapest reviewed Standard version.
        for leg in legs:
            if leg.name.startswith("option:"):
                self.assertEqual(leg.model_version, run_live.DEFAULT_OPTION_VERSION)
        capabilities = {entry["capability"] for entry in unavailable}
        self.assertIn("image_to_model sweep", capabilities)
        self.assertIn("texture_model", capabilities)
        self.assertTrue(all(entry["skipped_before_dispatch"] for entry in unavailable))

    def test_operator_inputs_enable_the_conditional_legs(self):
        legs, unavailable = run_live.build_legs(
            ["v2.5-20250123"], "https://example.test/x.png", "upstream-9"
        )
        names = [leg.name for leg in legs]
        self.assertIn("image_to_model:v2.5-20250123", names)
        self.assertIn("texture_model:standard", names)
        self.assertEqual(unavailable, [])

    def test_unreviewed_version_fails_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.build_legs(["v9.9-29990101"], None, None)


class HealthGateTests(unittest.TestCase):
    def test_healthy_payload_passes(self):
        payload = subs_payload()
        run_live.require_healthy_plane(payload)
        view = run_live.profile_view(payload, "owned-1", BALANCE_TS - 50)
        self.assertEqual(view["cohort"], "api-50")
        self.assertEqual(view["observed_spend_nano"], 0)

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

    def test_dead_walled_cooling_inflight_and_cohort_fail_closed(self):
        for kwargs in (
            {"live": False},
            {"walled": True},
            {"cooling_until": BALANCE_TS},
            {"cohort": " "},
        ):
            with self.assertRaises(run_live.CalibrationError, msg=str(kwargs)):
                run_live.profile_view(subs_payload(**kwargs), "owned-1", BALANCE_TS - 50)

    def test_inflight_work_before_dispatch_blocks_the_leg(self):
        runner = make_runner(FakePlane(), FakeCapacity([subs_payload(inflight=1)]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")


class ExecuteLegTests(unittest.TestCase):
    def test_happy_path_charges_the_exact_settled_delta(self):
        before = subs_payload(tracked_tasks=3, spend_nano=100, spend_milli=7)
        after = subs_payload(
            tracked_tasks=4,
            spend_nano=100 + 100_000_000,  # 10 credits = $0.10
            spend_milli=7 + 10_000,
            balance_observed_at=BALANCE_TS,
            balance_micro=49_900_000,
        )
        plane = FakePlane()
        capacity = FakeCapacity([before, after])
        runner = make_runner(plane, capacity)
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "exact")
        self.assertEqual(record["settled_nanousd"], str(100_000_000))
        self.assertEqual(record["settled_native_millicredits"], str(10_000))
        self.assertEqual(record["balance_drawdown_micro_units"], 100_000)
        self.assertEqual(record["task_status"], "success")
        self.assertEqual(runner.budget.spent_nano, 100_000_000)
        self.assertEqual(len(plane.creates), 1)

    def test_no_free_balance_preflight_blocks_paid_traffic(self):
        capacity = FakeCapacity([subs_payload(balance_observed_at=None)])
        runner = make_runner(FakePlane(), capacity)
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_foreign_tracked_task_makes_attribution_ambiguous(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(
            tracked_tasks=5,  # two new tasks: a foreign one raced the leg
            spend_nano=100_000_000,
            spend_milli=10_000,
            balance_observed_at=BALANCE_TS,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "ambiguous")
        self.assertTrue(record["foreign_traffic"])
        # The money moved and is charged even when attribution is ambiguous.
        self.assertEqual(runner.budget.spent_nano, 100_000_000)

    def test_foreign_inflight_at_settle_makes_attribution_ambiguous(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(
            tracked_tasks=4,
            spend_nano=100_000_000,
            spend_milli=10_000,
            balance_observed_at=BALANCE_TS,
            inflight=1,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        record = runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(record["attribution"], "ambiguous")

    def test_tariff_anomaly_counter_advance_fails_closed(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(
            tracked_tasks=4,
            spend_nano=100_000_000,
            spend_milli=10_000,
            balance_observed_at=BALANCE_TS,
            tariff_anomaly=1,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_spend_above_the_preflight_bound_fails_closed(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(
            tracked_tasks=4,
            spend_nano=100_000_001,  # one nanoUSD above the 10-credit bound
            spend_milli=10_000,
            balance_observed_at=BALANCE_TS,
        )
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_success_without_settled_spend_fails_closed(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(tracked_tasks=4, balance_observed_at=BALANCE_TS)
        runner = make_runner(FakePlane(), FakeCapacity([before, after]))
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(first_leg(), "owned-1")

    def test_refund_probe_requires_a_failed_task_with_zero_spend(self):
        before = subs_payload(tracked_tasks=3)
        after = subs_payload(tracked_tasks=4, balance_observed_at=BALANCE_TS)
        plane = FakePlane(default_status="failed")
        runner = make_runner(plane, FakeCapacity([before, after]))
        record = runner.execute_leg(refund_leg(), "owned-1")
        self.assertEqual(record["task_status"], "failed")
        self.assertEqual(record["settled_nanousd"], "0")
        self.assertEqual(runner.budget.spent_nano, 0)

    def test_refund_probe_rejects_a_paid_failure_or_a_success(self):
        before = subs_payload(tracked_tasks=3)
        charged = subs_payload(
            tracked_tasks=4, spend_nano=1, spend_milli=1, balance_observed_at=BALANCE_TS
        )
        runner = make_runner(
            FakePlane(default_status="failed"), FakeCapacity([before, charged])
        )
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(refund_leg(), "owned-1")
        ok = subs_payload(tracked_tasks=4, balance_observed_at=BALANCE_TS)
        runner = make_runner(
            FakePlane(default_status="success"), FakeCapacity([before, ok])
        )
        with self.assertRaises(run_live.CalibrationError):
            runner.execute_leg(refund_leg(), "owned-1")

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
                "/v1/3d/generations", 400, "tripo3d_style_wire_unproven"
            )
        ]
        runner = make_runner(plane, FakeCapacity([subs_payload()]))
        with self.assertRaises(run_live.HttpCalibrationError):
            runner.execute_leg(first_leg(), "owned-1")
        self.assertEqual(runner.budget.committed_nano(), 0)

    def test_unresolved_settlement_is_not_zero(self):
        # The balance observation never advances past task completion: the leg fails as a
        # post-spend poll error instead of recording a zero delta.
        stuck = subs_payload(tracked_tasks=4, balance_observed_at=1)
        runner = make_runner(
            FakePlane(), FakeCapacity([subs_payload(tracked_tasks=3), stuck])
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
            "1.00",
            "--versions",
            "v2.5-20250123",
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
        self.assertEqual(plan["schema"], "tripo3d-live-calibration-plan/v1")
        self.assertEqual(plan["paid_requests"], 0)
        self.assertIsNone(plan["budget_nanousd"])
        expected_legs, _ = run_live.build_legs(list(run_live.VERSIONS_TO_MODEL), None, None)
        self.assertEqual(
            int(plan["total_worst_case_nanousd"]),
            sum(run_live.leg_upper_bound_nano(leg) for leg in expected_legs),
        )
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
                ["--execute", "--profile", "owned-1", "--budget-usd", "5.01"]
            )
        args = run_live.parse_args(
            ["--execute", "--profile", "owned-1", "--budget-usd", "5.00"]
        )
        self.assertEqual(args.budget_usd, "5.00")

    def test_execute_requires_profile(self):
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--execute", "--budget-usd", "1.00"])


class ExecuteFlowTests(MainCase):
    def test_happy_path_full_run_is_complete_and_reports_everything(self):
        plane = full_run_plane()
        code, _ = self.run_main(self.argv(), full_run_payloads(), plane)
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        self.assertTrue(report["complete"])
        self.assertEqual(report["spent_nanousd"], str(8 * 50_000_000))
        statuses = report["leg_status"]
        self.assertEqual(statuses["text_to_model:v2.5-20250123"], "ok")
        self.assertEqual(statuses["option:texture-extreme"], "ok")
        self.assertEqual(statuses["refund-probe:image_to_model"], "ok")
        self.assertEqual(report["coverage"]["pending_legs"], [])
        capabilities = {entry["capability"] for entry in report["unavailable_capabilities"]}
        self.assertIn("image_to_model sweep", capabilities)
        self.assertEqual(len(plane.creates), 9)

    def test_budget_guard_stops_the_matrix_partway(self):
        # 0.11 USD: one 10-credit leg fits, the first option leg's guard stops the run.
        payloads = full_run_payloads()
        argv = [
            "--execute", "--profile", "owned-1", "--budget-usd", "0.11",
            "--versions", "v2.5-20250123",
            "--report", self.report, "--checkpoint", self.checkpoint,
        ]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(argv, payloads, full_run_plane())
        report = json.loads(Path(self.report).read_text())
        self.assertFalse(report["complete"])
        self.assertEqual(report["spent_nanousd"], "50000000")
        self.assertIn("budget guard stopped", report["failure"])
        self.assertTrue(report["coverage"]["pending_legs"])

    def test_ambiguous_attribution_is_recorded_and_stops_the_matrix(self):
        before = subs_payload(tracked_tasks=3)
        raced = subs_payload(
            tracked_tasks=5,  # a foreign task raced the leg
            spend_nano=100_000_000,
            spend_milli=10_000,
            balance_observed_at=BALANCE_TS,
        )
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [before, before, raced, raced], full_run_plane())
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
        self.assertEqual(
            checkpoint["leg_status"]["text_to_model:v2.5-20250123"], "held-ambiguous"
        )
        self.assertEqual(checkpoint["held_nano"], 100_000_000)
        # Close out every other leg in the checkpoint: the resume then has nothing to send,
        # which is exactly the proof that the held leg is not repeated.
        legs, _ = run_live.build_legs(["v2.5-20250123"], None, None)
        for leg in legs:
            checkpoint["leg_status"].setdefault(leg.name, "ok")
        Path(self.checkpoint).write_text(json.dumps(checkpoint))
        plane2 = FakePlane()
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(
                self.argv("--resume", self.checkpoint), [subs_payload()], plane2
            )
        self.assertEqual(plane2.creates, [])

    def test_typed_400_on_optional_legs_records_unavailability_and_continues(self):
        plane = full_run_plane()
        original_create = plane.create

        def create_with_refusals(body: dict) -> dict:
            if body["type"] == "text_to_model" and (
                body.get("texture")
                or body.get("smart_low_poly")
                or body.get("quad")
                or body.get("generate_parts")
                or body.get("geometry_quality")
            ):
                plane.creates.append(body)
                raise run_live.HttpCalibrationError(
                    "/v1/3d/generations", 400, "upstream refused the option"
                )
            return original_create(body)

        plane.create = create_with_refusals
        # Reads: baseline, leg before/after, one before-read per refused option leg (a typed
        # 400 settles nothing, so no settle read), and the refund probe's before/after pair.
        baseline = subs_payload(tracked_tasks=0)
        before = subs_payload(tracked_tasks=0)
        settled = subs_payload(
            tracked_tasks=1,
            spend_nano=50_000_000,
            spend_milli=5_000,
            balance_observed_at=BALANCE_TS,
        )
        refused_reads = [
            subs_payload(
                tracked_tasks=1,
                spend_nano=50_000_000,
                spend_milli=5_000,
                balance_observed_at=BALANCE_TS,
            )
            for _ in range(7)
        ]
        refund_before = refused_reads[-1]
        refund_after = subs_payload(
            tracked_tasks=2,
            spend_nano=50_000_000,
            spend_milli=5_000,
            balance_observed_at=BALANCE_TS,
        )
        payloads = [baseline, before, settled] + refused_reads + [refund_before, refund_after]
        code, _ = self.run_main(self.argv(), payloads, plane)
        # Option 400s are non-blocking: the run still completes.
        self.assertEqual(code, 0)
        report = json.loads(Path(self.report).read_text())
        option_unavailable = [
            entry
            for entry in report["unavailable_capabilities"]
            if str(entry["capability"]).startswith("option:")
        ]
        self.assertEqual(len(option_unavailable), 7)
        self.assertTrue(all(not entry["blocking"] for entry in option_unavailable))
        self.assertEqual(report["leg_status"]["text_to_model:v2.5-20250123"], "ok")

    def test_required_leg_400_is_blocking(self):
        plane = FakePlane()
        plane.create_errors = [
            run_live.HttpCalibrationError("/v1/3d/generations", 400, "admission refused")
        ]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], plane)
        report = json.loads(Path(self.report).read_text())
        blocking = [entry for entry in report["unavailable_capabilities"] if entry["blocking"]]
        self.assertTrue(blocking)
        self.assertFalse(report["complete"])

    def test_balance_wall_and_rate_wall_stop_with_profile_stops(self):
        for status in (403, 429):
            plane = FakePlane()
            plane.create_errors = [
                run_live.HttpCalibrationError("/v1/3d/generations", status, "wall")
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
            code, _ = self.run_main(self.argv(), full_run_payloads(), full_run_plane())
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
                client.create({"type": "text_to_model"})
        self.assertNotIn(API_KEY, caught.exception.detail)
        self.assertIn("***", caught.exception.detail)


class ResumeTests(MainCase):
    def test_resume_skips_completed_legs(self):
        code, _ = self.run_main(self.argv(), full_run_payloads(), full_run_plane())
        self.assertEqual(code, 0)
        plane2 = FakePlane()
        code, _ = self.run_main(
            self.argv("--resume", self.checkpoint), [subs_payload()], plane2
        )
        self.assertEqual(code, 0)
        self.assertEqual(plane2.creates, [])

    def test_resume_rejects_a_mismatched_run_identity(self):
        code, _ = self.run_main(self.argv(), full_run_payloads(), full_run_plane())
        self.assertEqual(code, 0)
        mismatched = [
            "--execute", "--profile", "owned-1", "--budget-usd", "2.00",
            "--versions", "v2.5-20250123", "--resume", self.checkpoint,
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
                run_live.matrix_identity(["v2.5-20250123"], None, None),
            )


class ReportCompletenessTests(MainCase):
    def test_incomplete_run_still_writes_a_full_report_shape(self):
        plane = FakePlane()
        plane.create_errors = [run_live.TransportFailureError("connection reset")]
        with self.assertRaises(run_live.CalibrationError):
            self.run_main(self.argv(), [subs_payload()], plane)
        report = json.loads(Path(self.report).read_text())
        self.assertEqual(report["schema"], "tripo3d-live-calibration/v1")
        self.assertFalse(report["complete"])
        self.assertIsNotNone(report["failure"])
        self.assertEqual(report["held_nanousd"], "100000000")
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
