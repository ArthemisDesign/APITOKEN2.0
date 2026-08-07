import contextlib
import io
import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest
import urllib.error
from unittest import mock

from tools.kimi_calibration import run_live


def turn_event(
    request_id: str = "req-1",
    profile: str = "profile-a",
    requested_model: str = "kimi-for-coding",
    served_model: str = "kimi-for-coding",
    completed_at: str = "100",
    tariff: str = run_live.KIMI_TARIFF_SCHEDULE_ID,
):
    value = {
        "request_id": request_id,
        "profile_id": profile,
        "plan": "Moderato",
        "requested_model": requested_model,
        "served_model": served_model,
        "context_mode": "256k",
        "reasoning_effort": "high",
        "tariff_schedule_id": tariff,
        "priced_ts": "100",
        "completed_at": completed_at,
    }
    value.update({field: "0" for field in run_live.EVENT_TOKEN_FIELDS})
    value.update({field: "0" for field in run_live.EVENT_MONEY_FIELDS})
    # Exact kimi-k2.7-code tariff: 10 x 950 input, 1 x 4000 output. The runner recomputes every
    # money leg from the token counts, so an invented vector no longer parses as a served turn.
    value["input_tokens"] = "10"
    value["output_tokens"] = "1"
    value["api_input_nanousd"] = "9500"
    value["api_output_nanousd"] = "4000"
    value["api_total_nanousd"] = "13500"
    return value


def window(
    duration: int = 18000,
    used: str = "10",
    limit: str = "1000",
    fraction: int = 1_000_000,
    resolution: int = 100_000,
    resets_at=200,
    observed_at=100,
):
    return {
        "duration_secs": duration,
        "used_units": used,
        "limit_units": limit,
        "used_fraction_units": fraction,
        "measurement_resolution_fraction_units": resolution,
        "resets_at": resets_at,
        "observed_at": observed_at,
    }


def profile(
    profile_id: str = "profile-a",
    plan: str = "Moderato",
    live: bool = True,
    cooling=None,
    quota=None,
    quota_observed_at=100,
):
    """Mirror the exact `/kimi-subs` profile shape emitted by `kimi_profile_value`.

    There is no `authenticated` boolean on the wire and the window list is named `quota`;
    inventing either here is what let the runner ship against a payload the engine never sent.
    """
    return {
        "id": profile_id,
        "plan": plan,
        "live": live,
        "inflight": 0,
        "cooling": cooling
        if cooling is not None
        else {"auth_until": None, "transport_until": None, "quota_until": None},
        "quota_observed_at": quota_observed_at,
        "quota": quota if quota is not None else [window()],
        "calibration": {
            "samples": 0,
            "confidence_bp": None,
            "capacity_nano": {"current": None, "low": None, "high": None},
            "remaining": {"native_units": None, "api_nano": None},
            "observed_spend_nano": "0",
            "unattributed_fraction_units": 0,
            "last_measured_at": None,
            "estimator_version": 1,
        },
    }


def subs(events=None, profiles=None):
    return {
        "now": 100,
        "enabled": True,
        "calibration_authority_available": True,
        "delivery": {"pending_events": 0, "dropped_events": 0, "persistence_ok": True},
        "calibration_recent_turn_limit": 512,
        "calibration_recent_turns": events or [],
        "profiles": profiles if profiles is not None else [profile()],
        "conversion_models": [],
    }


class FakeApi:
    def __init__(self, fail: bool = False):
        self.request_id = None
        self.calls = []
        self.fail = fail

    def request(
        self,
        path,
        method="GET",
        body=None,
        target_profile=None,
        calibration_request_id=None,
    ):
        self.calls.append({
            "path": path,
            "method": method,
            "target_profile": target_profile,
            "calibration_request_id": calibration_request_id,
        })
        if self.fail:
            raise run_live.CalibrationError("ambiguous paid transport failure")
        self.request_id = calibration_request_id
        return {"id": "msg_1", "model": "kimi-k2.7-code"}


class FakeSubs:
    def __init__(self, api, fresh_quota: bool = True, tariff: str = run_live.KIMI_TARIFF_SCHEDULE_ID):
        self.api = api
        self.fresh_quota = fresh_quota
        self.tariff = tariff

    def read(self):
        events = []
        observed = 100
        used = "10"
        fraction = 1_000_000
        if self.api.request_id:
            events = [turn_event(self.api.request_id, completed_at="150", tariff=self.tariff)]
            used = "20"
            fraction = 2_000_000
            observed = 160 if self.fresh_quota else 100
        payload = subs(events)
        payload["profiles"] = [
            profile(quota=[window(used=used, fraction=fraction, observed_at=observed)])
        ]
        return payload


def for_coding_leg(effort: str = "high") -> run_live.Leg:
    legs = {
        leg.name: leg for leg in run_live.build_coverage_legs(list(run_live.DEFAULT_MODELS))
    }
    return legs[f"kimi-for-coding:256k:{effort}"]


class DryRunAndBudgetTests(unittest.TestCase):
    def test_dry_run_is_the_default_and_sends_nothing(self):
        stdout = io.StringIO()
        with mock.patch.object(run_live, "CapacityReader") as capacity_reader, mock.patch.object(
            run_live, "JsonHttpClient"
        ) as api_client, contextlib.redirect_stdout(stdout):
            self.assertEqual(run_live.main([]), 0)
        capacity_reader.assert_not_called()
        api_client.assert_not_called()
        plan = json.loads(stdout.getvalue())
        self.assertEqual(plan["schema"], "kimi-live-calibration-plan/v1")
        self.assertEqual(plan["mode"], "dry-run")
        self.assertEqual(plan["paid_requests"], 0)
        self.assertEqual(plan["budget_nanousd_total"], "100000")
        self.assertEqual(plan["budget_hard_cap_nanousd"], str(run_live.MAX_BUDGET_NANO))
        self.assertTrue(plan["legs"])
        self.assertTrue(all("upper_bound_nanousd" in leg for leg in plan["legs"]))

    def test_budget_hard_cap_is_the_authorized_amount_and_nothing_above_it(self):
        # The default stays the smallest useful run: raising the ceiling must not raise the amount
        # an unattended invocation spends.
        self.assertEqual(run_live.parse_args([]).budget_usd, "0.0001")
        self.assertEqual(run_live.parse_args(["--budget-usd", "10.00"]).budget_usd, "10.00")
        self.assertEqual(run_live.parse_args(["--budget-usd", "0.0001"]).budget_usd, "0.0001")
        for rejected in ("10.01", "11", "100", "0"):
            with self.assertRaises(SystemExit):
                run_live.parse_args(["--budget-usd", rejected])

    def test_budget_parser_is_a_strict_exact_decimal(self):
        self.assertEqual(run_live.usd_to_nano("0.0001"), 100_000)
        self.assertEqual(run_live.usd_to_nano("0.000000001"), 1)
        self.assertEqual(run_live.usd_to_nano("1"), 1_000_000_000)
        for invalid in ("1e-4", "0.1e-3", "1.5e-1", "abc", "", ".5", "0.0000000001", "-1", "0,0001"):
            with self.subTest(invalid=invalid):
                with self.assertRaises(run_live.CalibrationError):
                    run_live.usd_to_nano(invalid)
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--budget-usd", "1e-4"])

    def test_execute_requires_a_valid_exact_profile(self):
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--execute"])
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--profile", "bad.id"])
        self.assertEqual(run_live.parse_args(["--profile", "ok_id-1"]).profile, "ok_id-1")

    def test_aggregate_budget_guard_across_sequential_legs(self):
        budget = run_live.Budget(100_000)
        budget.require(60_000)
        budget.charge("profile-a", 60_000, 60_000)
        budget.require(40_000)
        with self.assertRaises(run_live.CalibrationError):
            budget.require(40_001)
        budget.charge("profile-a", 40_000, 40_000)
        self.assertEqual(budget.total_nano, 100_000)
        self.assertEqual(budget.by_profile["profile-a"], 100_000)
        with self.assertRaises(run_live.CalibrationError):
            budget.charge("profile-a", 1, 1)

    def test_upper_bound_math_per_served_model_including_thinking_off_reroute(self):
        legs = {
            leg.name: leg for leg in run_live.build_coverage_legs(list(run_live.DEFAULT_MODELS))
        }
        bound = run_live.request_upper_bound_nano
        self.assertEqual(
            bound(legs["kimi-for-coding:256k:high"], run_live.RATE_CARD),
            262_144 * 950 + 256 * 4_000,
        )
        self.assertEqual(
            bound(legs["kimi-for-coding-highspeed:256k:high"], run_live.RATE_CARD),
            262_144 * 1_900 + 256 * 8_000,
        )
        self.assertEqual(
            bound(legs["k3-256k:256k:max"], run_live.RATE_CARD),
            262_144 * 3_000 + 256 * 15_000,
        )
        self.assertEqual(
            bound(legs["k3:1m:high"], run_live.RATE_CARD),
            1_048_576 * 3_000 + 256 * 15_000,
        )
        # Thinking off re-routes to kimi-k2.6: the bound must price the worst candidate card.
        off_for_coding = bound(legs["kimi-for-coding:256k:off"], run_live.RATE_CARD)
        self.assertEqual(off_for_coding, 262_144 * 950 + 256 * 4_000)
        off_k3 = bound(legs["k3-256k:256k:off"], run_live.RATE_CARD)
        k26_only = 262_144 * 950 + 256 * 4_000
        self.assertEqual(off_k3, 262_144 * 3_000 + 256 * 15_000)
        self.assertGreater(off_k3, k26_only)

    def test_charge_rejects_nonpositive_or_above_bound_actuals(self):
        budget = run_live.Budget(1_000_000)
        with self.assertRaises(run_live.CalibrationError):
            budget.charge("profile-a", 0, 1)
        with self.assertRaises(run_live.CalibrationError):
            budget.charge("profile-a", 2, 1)
        with self.assertRaises(run_live.CalibrationError):
            budget.require(0)


class EvidenceTests(unittest.TestCase):
    def test_fixed_contract_event_parses_and_attributes_exactly(self):
        contract_event = {
            "request_id": "uuid-v4",
            "profile_id": "opaque-id",
            "plan": "Moderato",
            "requested_model": "kimi-for-coding",
            "served_model": "kimi-for-coding",
            "context_mode": "256k",
            "reasoning_effort": "high",
            "tariff_schedule_id": "moonshot/kimi-open-platform/2026-08-03",
            "priced_ts": 1756000000,
            "completed_at": 1756000001,
            "input_tokens": 100,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "output_tokens": 50,
            "reasoning_output_tokens": 10,
            "api_input_nanousd": "95000",
            "api_cache_read_nanousd": "0",
            "api_cache_write_nanousd": "0",
            "api_output_nanousd": "200000",
            "api_total_nanousd": "295000",
        }
        payload = subs([contract_event])
        parsed = run_live.recent_turn_events(payload)
        self.assertEqual(parsed["uuid-v4"]["api_total_nanousd"], 295_000)
        self.assertEqual(parsed["uuid-v4"]["reasoning_output_tokens"], 10)
        event = run_live.exact_new_turn(set(), payload, "uuid-v4", "opaque-id", for_coding_leg("high"))
        self.assertEqual(event["request_id"], "uuid-v4")
        with self.assertRaises(run_live.CalibrationError):
            run_live.exact_new_turn({"uuid-v4"}, payload, "uuid-v4", "opaque-id", for_coding_leg("high"))

    def test_exact_attribution_ignores_concurrent_unrelated_events(self):
        payload = subs([
            turn_event("req-unrelated-1", profile="profile-b", served_model="kimi-k3"),
            turn_event("req-1"),
            turn_event("req-unrelated-2", profile="profile-a", served_model="kimi-k2.6"),
        ])
        event = run_live.exact_new_turn(set(), payload, "req-1", "profile-a", for_coding_leg("high"))
        self.assertEqual(event["request_id"], "req-1")
        self.assertIsNone(
            run_live.exact_new_turn(set(), subs([]), "req-1", "profile-a", for_coding_leg("high"))
        )

    def test_duplicate_same_id_events_fail_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([turn_event("req-1"), turn_event("req-1")]))

    def test_rebind_of_profile_or_served_model_fails_closed(self):
        rebound_profile = subs([turn_event("req-1", profile="profile-b")])
        with self.assertRaises(run_live.CalibrationError):
            run_live.exact_new_turn(set(), rebound_profile, "req-1", "profile-a", for_coding_leg("high"))
        rebound_model = subs([turn_event("req-1", served_model="kimi-k2.6")])
        with self.assertRaises(run_live.CalibrationError):
            run_live.exact_new_turn(set(), rebound_model, "req-1", "profile-a", for_coding_leg("high"))

    def test_incomplete_identity_fails_closed(self):
        incomplete = turn_event()
        incomplete.pop("served_model")
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([incomplete]))

    def test_cost_vector_integrity_fails_closed(self):
        broken = turn_event()
        broken["api_total_nanousd"] = "101"
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([broken]))
        missing = turn_event()
        missing.pop("api_output_nanousd")
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([missing]))
        fractional = turn_event()
        fractional["api_input_nanousd"] = "19.5"
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([fractional]))
        zero = turn_event()
        zero["api_input_nanousd"] = "0"
        zero["api_total_nanousd"] = "0"
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([zero]))
        impossible = turn_event()
        impossible["reasoning_output_tokens"] = "2"
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(subs([impossible]))

    def test_recent_turn_limit_below_512_stops_the_run(self):
        payload = subs()
        payload["calibration_recent_turn_limit"] = 256
        with self.assertRaises(run_live.CalibrationError):
            run_live.recent_turn_events(payload)

    def test_baseline_violations_stop_before_spending(self):
        run_live.require_healthy_delivery(subs())
        pending = subs()
        pending["delivery"]["pending_events"] = 1
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(pending)
        run_live.require_healthy_delivery(pending, require_empty=False)
        dropped = subs()
        dropped["delivery"]["dropped_events"] = 1
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(dropped, require_empty=False)
        degraded = subs()
        degraded["delivery"]["persistence_ok"] = False
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(degraded)
        no_authority = subs()
        no_authority["calibration_authority_available"] = False
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(no_authority)
        disabled = subs()
        disabled["enabled"] = False
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_healthy_delivery(disabled)


class ProfileGateTests(unittest.TestCase):
    def test_cooling_in_any_lane_refuses_the_target(self):
        for lane in ("auth_until", "transport_until", "quota_until"):
            with self.subTest(lane=lane):
                cooling = {"auth_until": None, "transport_until": None, "quota_until": None}
                cooling[lane] = 200
                state = run_live.profile_state(subs(profiles=[profile(cooling=cooling)]))
                with self.assertRaises(run_live.CalibrationError):
                    run_live.require_routable_profile(state.get("profile-a"), "profile-a", 100)
                run_live.require_routable_profile(state.get("profile-a"), "profile-a", 300)

    def test_dead_unauthenticated_or_absent_profile_is_refused(self):
        dead = run_live.profile_state(subs(profiles=[profile(live=False)]))
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_routable_profile(dead.get("profile-a"), "profile-a", 100)
        quarantined = run_live.profile_state(
            subs(
                profiles=[
                    profile(
                        cooling={
                            "auth_until": 500,
                            "transport_until": None,
                            "quota_until": None,
                        }
                    )
                ]
            )
        )
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_routable_profile(
                quarantined.get("profile-a"), "profile-a", 100
            )
        with self.assertRaises(run_live.CalibrationError):
            run_live.require_routable_profile(None, "profile-a", 100)

    def test_profile_id_format_matches_the_roster_contract(self):
        self.assertEqual(run_live.validate_profile_id("p-1_OK"), "p-1_OK")
        self.assertEqual(run_live.validate_profile_id("a" * 128), "a" * 128)
        for invalid in ("", "a" * 129, "bad.id", "bad id", "bad/id", "профиль", None, 7):
            with self.subTest(invalid=invalid):
                with self.assertRaises(run_live.CalibrationError):
                    run_live.validate_profile_id(invalid)


class QuotaAttributionTests(unittest.TestCase):
    def test_resolved_window_reports_fraction_and_native_deltas(self):
        parsed = run_live.profile_state(subs())
        before = parsed["profile-a"]["windows"][18000]
        after = dict(before, used_units=20, used_fraction_units=2_000_000, observed_at=160)
        delta = run_live.window_observation_delta(before, after, 150)
        self.assertEqual(delta["status"], "resolved")
        self.assertEqual(delta["fraction_delta"], 1_000_000)
        self.assertEqual(delta["native_delta"], 10)

    def test_unresolved_snapshot_is_not_zero_and_is_excluded(self):
        parsed = run_live.profile_state(subs())
        before = parsed["profile-a"]["windows"][18000]
        stale = dict(before, used_units=20, used_fraction_units=2_000_000, observed_at=140)
        delta = run_live.window_observation_delta(before, stale, 150)
        self.assertEqual(delta["status"], "unresolved")
        self.assertIsNone(delta["fraction_delta"])
        self.assertIsNone(delta["native_delta"])
        unobserved = dict(before, observed_at=None)
        self.assertEqual(
            run_live.window_observation_delta(before, unobserved, 150)["status"], "unresolved"
        )
        self.assertEqual(
            run_live.window_observation_delta(before, None, 150)["status"], "unresolved"
        )

    def test_reset_identity_change_reports_reset_crossed(self):
        parsed = run_live.profile_state(subs())
        before = parsed["profile-a"]["windows"][18000]
        crossed = dict(before, used_units=1, used_fraction_units=100_000, resets_at=400, observed_at=160)
        delta = run_live.window_observation_delta(before, crossed, 150)
        self.assertEqual(delta["status"], "reset-crossed")
        self.assertIsNone(delta["fraction_delta"])
        self.assertIsNone(delta["native_delta"])

    def test_negative_movement_without_reset_is_not_evidence(self):
        parsed = run_live.profile_state(subs())
        before = parsed["profile-a"]["windows"][18000]
        backwards = dict(before, used_units=5, used_fraction_units=500_000, observed_at=160)
        delta = run_live.window_observation_delta(before, backwards, 150)
        self.assertEqual(delta["status"], "resolved")
        self.assertIsNone(delta["fraction_delta"])
        self.assertIsNone(delta["native_delta"])

    def test_profitability_uses_only_positive_observed_deltas(self):
        def record(model, actual, delta, eligible=True, resolved=True, status="resolved"):
            return {
                "plan": "Moderato",
                "served_model": model,
                "context_mode": "256k",
                "reasoning_effort": "high",
                "actual_nanousd": str(actual),
                "profitability_eligible": eligible,
                "quota_snapshot_resolved": resolved,
                "windows": [
                    {"duration_secs": 18000, "status": status, "fraction_delta": delta}
                ],
            }

        rows = run_live.model_profitability([
            record("kimi-a", 100, 10),
            record("kimi-b", 300, 10),
            record("kimi-concurrent", 999999, 1, eligible=False),
            record("kimi-unresolved", 999999, 10, resolved=False),
            record("kimi-zero", 999999, 0),
            record("kimi-none", 999999, None),
            record("kimi-crossed", 999999, 10, status="reset-crossed"),
        ])
        self.assertEqual([row["served_model"] for row in rows], ["kimi-b", "kimi-a"])
        self.assertTrue(all(row["window_duration_secs"] == 18000 for row in rows))
        self.assertEqual(rows[0]["api_nanousd_per_1pct_window"], "30000000")


class MatrixTests(unittest.TestCase):
    def test_default_matrix_covers_models_contexts_efforts_and_dedupes_reroute(self):
        legs = run_live.build_coverage_legs(list(run_live.DEFAULT_MODELS))
        names = {leg.name for leg in legs}
        self.assertEqual(names, {
            "k3-256k:256k:low",
            "k3-256k:256k:high",
            "k3-256k:256k:max",
            "k3-256k:256k:off",
            "k3:1m:low",
            "k3:1m:high",
            "k3:1m:max",
            "k3:1m:off",
            "kimi-for-coding:256k:high",
            "kimi-for-coding:256k:off",
            "kimi-for-coding-highspeed:256k:high",
            "kimi-for-coding-highspeed:256k:off",
        })
        for leg in legs:
            if leg.reasoning_effort == "off":
                self.assertEqual(leg.served_model, "kimi-k2.6")
            else:
                self.assertEqual(
                    leg.served_model, run_live.ALIAS_SPECS[leg.requested_model].official_model
                )

    def test_unknown_served_model_fails_closed(self):
        with self.assertRaises(run_live.CalibrationError):
            run_live.build_coverage_legs(["kimi-k2.5"])

    def _main_with_stubbed_legs(self, argv, baseline):
        fake_capacity = mock.Mock()
        fake_capacity.read.return_value = baseline
        calls = []

        def execute(runner, leg, target_profile):
            calls.append((target_profile, leg.name, leg.context_mode))
            runner.records.append({
                "leg": leg.name,
                "profitability_eligible": False,
                "quota_snapshot_resolved": False,
                "windows": [],
            })
            return runner.records[-1]

        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with mock.patch.dict(os.environ, {"APITOKEN_API_KEY": "test-key"}), mock.patch.object(
                run_live, "CapacityReader", return_value=fake_capacity
            ), mock.patch.object(
                run_live, "JsonHttpClient", return_value=mock.Mock()
            ), mock.patch.object(
                run_live.Runner, "execute_leg", new=execute
            ):
                rc = run_live.main(argv + ["--capacity-command", "unused", "--report", report])
            with open(report, encoding="utf-8") as report_file:
                payload = json.load(report_file)
        return rc, payload, calls

    def test_one_m_records_unavailable_until_the_plan_is_reviewed(self):
        rc, payload, calls = self._main_with_stubbed_legs(
            ["--execute", "--profile", "profile-a", "--models", "kimi-k3"], subs()
        )
        self.assertEqual(rc, 0)
        self.assertTrue(payload["complete"])
        gated = payload["unavailable_capabilities"]
        self.assertTrue(all(item["skipped_before_dispatch"] for item in gated))
        self.assertTrue(all(not item["blocking"] for item in gated))
        one_m = [item for item in gated if "1m context" in item["reason"]]
        self.assertEqual(len(one_m), 4)
        # Tools and media are reported as untested rather than silently dropped: their
        # per-request unit cost is unproven, so no authorization means no spend and no claim.
        probes = [item for item in gated if "unit cost is unproven" in item["reason"]]
        self.assertEqual({item["capability"].rsplit(":", 1)[-1] for item in probes}, {"tools", "media"})
        self.assertEqual(len(calls), 4)
        self.assertTrue(all(context == "256k" for _, _, context in calls))

    def test_one_m_reviewed_plan_runs_the_full_k3_matrix(self):
        rc, payload, calls = self._main_with_stubbed_legs(
            [
                "--execute",
                "--profile",
                "profile-a",
                "--models",
                "kimi-k3",
                "--one-m-plans",
                "Moderato",
            ],
            subs(),
        )
        self.assertEqual(rc, 0)
        self.assertTrue(payload["complete"])
        # The 1m gate is lifted, but the capability probes still carry no authorization.
        gated = payload["unavailable_capabilities"]
        self.assertTrue(all("unit cost is unproven" in item["reason"] for item in gated))
        self.assertEqual(len(calls), 8)
        self.assertEqual({context for _, _, context in calls}, {"256k", "1m"})


class TransportTests(unittest.TestCase):
    def test_production_ssh_keeps_secrets_remote_and_never_retries_paid_messages(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        failed = subprocess.CompletedProcess([], 255, stdout=b"", stderr=b"ambiguous")
        with mock.patch.dict(os.environ, {"CLAUDE_API_KEYS": "local-secret-must-not-leak"}):
            with mock.patch.object(
                run_live.subprocess, "run", return_value=failed
            ) as invoked, mock.patch.object(run_live.time, "sleep") as slept:
                with self.assertRaises(run_live.CalibrationError):
                    client.request(
                        "/v1/messages",
                        "POST",
                        {"model": "kimi-for-coding"},
                        "profile-a",
                        calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                    )
        self.assertEqual(invoked.call_count, 1)
        slept.assert_not_called()
        remote_command = invoked.call_args.args[0][2]
        self.assertNotIn("local-secret-must-not-leak", remote_command)
        self.assertIn("$calibration_key", remote_command)
        self.assertIn("%header{x-apitoken-execution-state}", remote_command)
        self.assertIn("x-apitoken-calibration-profile: profile-a", remote_command)
        self.assertIn(
            "x-apitoken-calibration-request-id: 123e4567-e89b-42d3-a456-426614174000",
            remote_command,
        )

    def test_production_ssh_retries_only_read_only_gets(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        failed = subprocess.CompletedProcess([], 255, stdout=b"", stderr=b"temporary")
        succeeded = subprocess.CompletedProcess(
            [], 0, stdout=b'{"ok": true}\n__CALIBRATION_HTTP__200\n', stderr=b""
        )
        with mock.patch.object(
            run_live.subprocess, "run", side_effect=[failed, succeeded]
        ) as invoked, mock.patch.object(run_live.time, "sleep", return_value=None):
            result = client.request("/v1/models", "GET")
        self.assertEqual(result, {"ok": True})
        self.assertEqual(invoked.call_count, 2)

    def test_production_ssh_preserves_the_not_started_proof(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        refused = subprocess.CompletedProcess(
            [],
            0,
            stdout=b'{"error":"quota"}\n__CALIBRATION_HTTP__503\nnot_started',
            stderr=b"",
        )
        with mock.patch.object(run_live.subprocess, "run", return_value=refused):
            with self.assertRaises(run_live.HttpCalibrationError) as caught:
                client.request(
                    "/v1/messages",
                    "POST",
                    {"model": "kimi-for-coding"},
                    "profile-a",
                    calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                )
        self.assertTrue(caught.exception.execution_not_started)
        self.assertTrue(run_live.is_explicit_transient_stop(caught.exception))
        self.assertFalse(
            run_live.is_explicit_transient_stop(
                run_live.HttpCalibrationError("/v1/messages", 502, "bad gateway", True)
            )
        )

    def test_production_ssh_validates_profile_request_id_path_target_and_port(self):
        client = run_live.ProductionSshJsonHttpClient(timeout=10)
        with self.assertRaises(run_live.CalibrationError):
            client.request("/v1/messages", "POST", {}, "bad.profile")
        with self.assertRaises(run_live.CalibrationError):
            client.request(
                "/v1/messages",
                "POST",
                {},
                "profile-a",
                calibration_request_id="123E4567-e89b-42d3-a456-426614174000",
            )
        with self.assertRaises(run_live.CalibrationError):
            client.request("/kimi-subs", "GET")
        with self.assertRaises(run_live.CalibrationError):
            client.request("/v1/messages;rm", "POST", {})
        with self.assertRaises(run_live.CalibrationError):
            run_live.ProductionSshJsonHttpClient(timeout=10, ssh_target="-oProxyCommand=bad")
        for port in (0, 65_536):
            with self.assertRaises(run_live.CalibrationError):
                run_live.ProductionSshJsonHttpClient(timeout=10, api_port=port)
        with self.assertRaises(SystemExit):
            run_live.parse_args(["--production-api-port", "0"])

    def test_remote_capacity_command_reads_kimi_subs_on_the_loopback(self):
        with mock.patch.dict(os.environ, {"CLAUDE_API_CONTROL_KEY": "local-secret-must-not-leak"}):
            command = run_live.remote_capacity_command()
        self.assertIn("ssh apitokensale", command)
        self.assertIn("127.0.0.1:8803/kimi-subs", command)
        self.assertIn("$CLAUDE_API_CONTROL_KEY", command)
        self.assertNotIn("local-secret-must-not-leak", command)
        canary = run_live.remote_capacity_command("deploy@84.32.48.2", 18895)
        self.assertIn("ssh deploy@84.32.48.2", canary)
        self.assertIn("127.0.0.1:18895/kimi-subs", canary)

    def test_direct_client_sends_exact_calibration_headers_once(self):
        client = run_live.JsonHttpClient("https://api.example", "key-1", 10)
        captured = {}

        class FakeResponse:
            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def read(self):
                return b'{"id": "msg_1"}'

        def fake_urlopen(request, timeout=0):
            captured["request"] = request
            return FakeResponse()

        with mock.patch.object(run_live.urllib.request, "urlopen", side_effect=fake_urlopen):
            result = client.request(
                "/v1/messages",
                "POST",
                {"model": "kimi-for-coding"},
                "profile-a",
                calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
            )
        self.assertEqual(result, {"id": "msg_1"})
        headers = {key.lower(): value for key, value in captured["request"].header_items()}
        self.assertEqual(headers["x-apitoken-calibration-profile"], "profile-a")
        self.assertEqual(
            headers["x-apitoken-calibration-request-id"],
            "123e4567-e89b-42d3-a456-426614174000",
        )
        self.assertEqual(headers["x-api-key"], "key-1")
        with mock.patch.object(run_live.urllib.request, "urlopen") as never:
            with self.assertRaises(run_live.CalibrationError):
                client.request("/v1/messages", "POST", {}, "bad.profile")
        never.assert_not_called()

    def test_direct_client_preserves_the_not_started_proof(self):
        client = run_live.JsonHttpClient("https://api.example", "key-1", 10)
        error = urllib.error.HTTPError(
            "https://api.example/v1/messages",
            503,
            "unavailable",
            {"x-apitoken-execution-state": "not_started"},
            io.BytesIO(b"refused"),
        )
        with mock.patch.object(run_live.urllib.request, "urlopen", side_effect=error):
            with self.assertRaises(run_live.HttpCalibrationError) as caught:
                client.request(
                    "/v1/messages",
                    "POST",
                    {},
                    "profile-a",
                    calibration_request_id="123e4567-e89b-42d3-a456-426614174000",
                )
        self.assertTrue(caught.exception.execution_not_started)
        self.assertTrue(run_live.is_explicit_transient_stop(caught.exception))

    def test_capacity_reader_retries_read_only_failures(self):
        reader = run_live.CapacityReader("emit subs", None, None, 10)
        failed = subprocess.CompletedProcess([], 1, stdout=b"", stderr=b"boom")
        succeeded = subprocess.CompletedProcess(
            [], 0, stdout=json.dumps(subs()).encode(), stderr=b""
        )
        with mock.patch.object(
            run_live.subprocess, "run", side_effect=[failed, succeeded]
        ) as invoked, mock.patch.object(run_live.time, "sleep", return_value=None):
            payload = reader.read()
        self.assertTrue(payload["enabled"])
        self.assertEqual(invoked.call_count, 2)
        with mock.patch.object(
            run_live.subprocess, "run", return_value=failed
        ) as invoked, mock.patch.object(run_live.time, "sleep", return_value=None):
            with self.assertRaises(run_live.CalibrationError):
                reader.read()
        self.assertEqual(invoked.call_count, run_live.SAFE_READ_ATTEMPTS)


class RunnerEndToEndTests(unittest.TestCase):
    def make_runner(self, api, capacity=None, budget_nano=1_000_000_000):
        return run_live.Runner(
            api,
            capacity or FakeSubs(api),
            run_live.RATE_CARD,
            run_live.Budget(budget_nano),
            timeout=1,
            delay=0,
            run_id="run",
        )

    def test_runner_supplies_one_exact_request_id_and_charges_actual_once(self):
        api = FakeApi()
        budget_runner = self.make_runner(api)
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            record = budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertRegex(api.request_id, r"^[0-9a-f-]{36}$")
        self.assertEqual(record["request_id"], api.request_id)
        self.assertEqual(len(api.calls), 1)
        self.assertEqual(api.calls[0]["path"], "/v1/messages")
        self.assertEqual(api.calls[0]["target_profile"], "profile-a")
        self.assertTrue(record["quota_snapshot_resolved"])
        self.assertTrue(record["profitability_eligible"])
        self.assertEqual(record["windows"][0]["status"], "resolved")
        self.assertEqual(record["windows"][0]["fraction_delta"], 1_000_000)
        self.assertEqual(record["windows"][0]["native_delta"], 10)
        self.assertEqual(record["actual_nanousd"], "13500")
        self.assertEqual(
            record["upper_bound_nanousd"], str(262_144 * 950 + 256 * 4_000)
        )
        self.assertEqual(budget_runner.budget.total_nano, 13_500)
        self.assertEqual(budget_runner.budget.by_profile["profile-a"], 13_500)
        self.assertNotIn("CALIBRATION_OK", json.dumps(record))
        self.assertIn("prompt_sha256_12", record)

    def test_paid_request_is_never_retried_after_ambiguous_transport(self):
        api = FakeApi(fail=True)
        budget_runner = self.make_runner(api)
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            with self.assertRaises(run_live.CalibrationError):
                budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertEqual(len(api.calls), 1)
        self.assertEqual(budget_runner.budget.total_nano, 0)

    def test_missing_immutable_event_fails_closed(self):
        api = FakeApi()
        budget_runner = self.make_runner(api, capacity=FakeSubs(api))
        budget_runner.capacity.read = lambda: subs()  # event never becomes durable
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            with self.assertRaises(run_live.CalibrationError) as caught:
                budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertIn("did not appear", str(caught.exception))
        self.assertEqual(budget_runner.budget.total_nano, 0)

    def test_stale_quota_observation_excludes_the_leg_from_profitability(self):
        api = FakeApi()
        budget_runner = self.make_runner(api, capacity=FakeSubs(api, fresh_quota=False))
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            record = budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertFalse(record["quota_snapshot_resolved"])
        self.assertFalse(record["profitability_eligible"])
        self.assertEqual(record["windows"][0]["status"], "unresolved")
        self.assertIsNone(record["windows"][0]["fraction_delta"])
        self.assertEqual(run_live.model_profitability([record]), [])
        self.assertEqual(budget_runner.budget.total_nano, 13_500)

    def test_aggregate_budget_stops_the_next_leg_before_dispatch(self):
        api = FakeApi()
        budget_runner = self.make_runner(api, budget_nano=262_144 * 950 + 256 * 4_000)
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
            with self.assertRaises(run_live.CalibrationError):
                budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertEqual(len(api.calls), 1)
        self.assertEqual(budget_runner.budget.total_nano, 13_500)

    def test_tariff_schedule_drift_fails_closed(self):
        api = FakeApi()
        budget_runner = self.make_runner(
            api, capacity=FakeSubs(api, tariff="moonshot/kimi-open-platform/1999-01-01")
        )
        with mock.patch.object(run_live.time, "sleep", return_value=None):
            with self.assertRaises(run_live.CalibrationError) as caught:
                budget_runner.execute_leg(for_coding_leg("high"), "profile-a")
        self.assertIn("tariff", str(caught.exception))
        self.assertEqual(budget_runner.budget.total_nano, 0)


class MainFlowTests(unittest.TestCase):
    def test_partial_failure_writes_an_explicit_incomplete_report(self):
        fake_capacity = mock.Mock()
        fake_capacity.read.return_value = subs()
        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with mock.patch.dict(os.environ, {"APITOKEN_API_KEY": "test-key"}), mock.patch.object(
                run_live, "CapacityReader", return_value=fake_capacity
            ), mock.patch.object(
                run_live, "JsonHttpClient", return_value=mock.Mock()
            ), mock.patch.object(
                run_live.Runner,
                "execute_leg",
                side_effect=run_live.CalibrationError("simulated paid-stage failure"),
            ):
                with self.assertRaises(run_live.CalibrationError):
                    run_live.main([
                        "--execute",
                        "--profile",
                        "profile-a",
                        "--capacity-command",
                        "unused",
                        "--report",
                        report,
                    ])
            with open(report, encoding="utf-8") as report_file:
                payload = json.load(report_file)
        self.assertEqual(payload["schema"], "kimi-live-calibration/v1")
        self.assertFalse(payload["complete"])
        self.assertIn("simulated paid-stage failure", payload["failure"])
        self.assertEqual(payload["spent_nanousd_total"], "0")
        self.assertEqual(payload["records"], [])
        self.assertEqual(
            len(payload["coverage"]["pending_legs"]),
            len(payload["coverage"]["expected_legs"]),
        )

    def test_secrets_never_enter_the_report(self):
        fake_capacity = mock.Mock()
        fake_capacity.read.return_value = subs()

        def execute(runner, leg, target_profile):
            runner.records.append({
                "leg": leg.name,
                "profitability_eligible": False,
                "quota_snapshot_resolved": False,
                "windows": [],
            })
            return runner.records[-1]

        with tempfile.TemporaryDirectory() as directory:
            report = os.path.join(directory, "report.json")
            with mock.patch.dict(
                os.environ,
                {
                    "APITOKEN_API_KEY": "local-api-secret",
                    "CLAUDE_API_CONTROL_KEY": "local-control-secret",
                },
            ), mock.patch.object(
                run_live, "CapacityReader", return_value=fake_capacity
            ), mock.patch.object(
                run_live, "JsonHttpClient", return_value=mock.Mock()
            ), mock.patch.object(
                run_live.Runner, "execute_leg", new=execute
            ):
                self.assertEqual(
                    run_live.main([
                        "--execute",
                        "--profile",
                        "profile-a",
                        "--models",
                        "kimi-k2.7-code",
                        "--capacity-command",
                        "unused",
                        "--report",
                        report,
                    ]),
                    0,
                )
            with open(report, encoding="utf-8") as report_file:
                text = report_file.read()
        self.assertNotIn("local-api-secret", text)
        self.assertNotIn("local-panel-secret", text)
        self.assertIn("profile-a", text)


if __name__ == "__main__":
    unittest.main()


class CapabilityProbeTests(unittest.TestCase):
    """The tool and media probes are the legs whose price is unknown by definition.

    The plane refuses tools and media precisely because no finite per-request unit ceiling is
    proved, and these legs exist to change that. So they must never run on the coverage budget,
    never run without their own authorization, and never disappear quietly when unauthorized —
    a capability that is silently skipped reads as a capability that was tested and passed.
    """

    def test_probes_are_absent_from_the_plan_without_their_own_authorization(self):
        args = run_live.parse_args(["--models", "kimi-k2.6"])
        plan = run_live.dry_run_plan(args, 100_000)
        self.assertTrue(all(":tools" not in leg["leg"] and ":media" not in leg["leg"] for leg in plan["legs"]))

    def test_probes_enter_the_plan_only_when_authorized(self):
        args = run_live.parse_args(
            ["--models", "kimi-k2.6", "--capability-probe-budget-usd", "0.0001"]
        )
        plan = run_live.dry_run_plan(args, 100_000)
        names = {leg["leg"] for leg in plan["legs"]}
        self.assertTrue(any(name.endswith(":tools") for name in names))
        self.assertTrue(any(name.endswith(":media") for name in names))

    def test_probe_authorization_is_capped_like_the_coverage_budget(self):
        # parse_args turns a rejected value into argparse's own exit, so the CLI cannot be
        # coaxed past the ceiling by passing a larger number.
        for rejected in ("1.00", "0", "0.001"):
            with self.assertRaises(SystemExit):
                run_live.parse_args(["--capability-probe-budget-usd", rejected])

    def test_tool_probe_declares_exactly_one_callable_tool(self):
        leg = next(
            item for item in run_live.build_capability_legs(["kimi-k2.6"]) if item.capability == "tools"
        )
        body = run_live.body_for_leg(leg, "run-1")
        self.assertEqual(len(body["tools"]), 1)
        # A probe that could fan out would price something other than one call.
        self.assertEqual(body["tools"][0]["input_schema"]["properties"], {})
        self.assertEqual(body["max_tokens"], run_live.DEFAULT_MAX_OUTPUT_TOKENS)

    def test_media_probe_sends_one_inline_part_beside_the_prompt(self):
        leg = next(
            item for item in run_live.build_capability_legs(["kimi-k2.6"]) if item.capability == "media"
        )
        body = run_live.body_for_leg(leg, "run-1")
        content = body["messages"][0]["content"]
        self.assertEqual([part["type"] for part in content], ["text", "image"])
        self.assertNotIn("_media_part", body)
        self.assertEqual(content[1]["source"]["media_type"], "image/png")

    def test_ordinary_legs_carry_no_capability_payload(self):
        for leg in run_live.build_coverage_legs(["kimi-k2.6"]):
            body = run_live.body_for_leg(leg, "run-1")
            self.assertIsNone(leg.capability)
            self.assertNotIn("tools", body)
            self.assertNotIn("tool_choice", body)
            self.assertIsInstance(body["messages"][0]["content"], str)


class KimiSubsWireContractTest(unittest.TestCase):
    """Pin the fixtures to the engine's serializer instead of to our own imagination.

    The runner once required an `authenticated` boolean and read the window list from
    `windows`; the engine has always published `cooling.auth_until` and `quota`. Every
    offline test was green because the fixtures invented the same payload the runner
    expected, so the mismatch only surfaced against production. This test removes that
    freedom: the fixture must carry exactly the top-level keys `kimi_profile_value` emits.
    """

    def engine_profile_keys(self) -> set:
        source = (
            pathlib.Path(__file__).resolve().parents[2]
            / "crates"
            / "server"
            / "src"
            / "http.rs"
        ).read_text(encoding="utf-8")
        start = source.index("fn kimi_profile_value")
        body = source[start : source.index("\nfn ", start + 1)]
        # Top-level `json!` members sit at exactly eight spaces; nested ones are deeper.
        return set(re.findall(r'^ {8}"([a-z_]+)":', body, flags=re.MULTILINE))

    def test_fixture_profile_matches_the_engine_serializer(self):
        self.assertEqual(set(profile().keys()), self.engine_profile_keys())

    def test_absent_quota_list_fails_closed_instead_of_reading_as_empty(self):
        drifted = profile()
        del drifted["quota"]
        with self.assertRaises(run_live.CalibrationError):
            run_live.profile_state(subs(profiles=[drifted]))

    def engine_keys(self, function: str) -> set:
        source = (
            pathlib.Path(__file__).resolve().parents[2]
            / "crates"
            / "server"
            / "src"
            / "http.rs"
        ).read_text(encoding="utf-8")
        start = source.index(f"fn {function}")
        body = source[start : source.index("\nfn ", start + 1)]
        return set(re.findall(r'^ {8}"([a-z_0-9]+)":', body, flags=re.MULTILINE))

    def test_fixture_turn_matches_the_engine_serializer(self):
        self.assertEqual(set(turn_event().keys()), self.engine_keys("kimi_turn_value"))

    def test_served_model_is_the_provider_name_not_the_tariff_key(self):
        # The engine reports what it asked the provider for; the tariff key is ours alone.
        self.assertEqual(turn_event()["served_model"], "kimi-for-coding")
        self.assertIn("kimi-k2.7-code", run_live.RATE_CARD)
        self.assertNotIn(turn_event()["served_model"], run_live.RATE_CARD)
