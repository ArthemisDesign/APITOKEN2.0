import dataclasses
import json
import unittest
from unittest.mock import patch

from tools.claude_calibration.run_live import (
    CalibrationError,
    Leg,
    ProfileBudget,
    ProductionSshJsonHttpClient,
    TokenRates,
    attribute_exact_turn,
    body_for_leg,
    build_coverage_legs,
    canonical_rate_id,
    count_body,
    coverage_failure,
    evidence_rows,
    guarded_input_tokens,
    model_profitability,
    rate_catalog,
    rates_for_model,
    request_upper_bound_nano,
    response_service_tier,
    require_healthy_delivery,
    row_deltas,
    usage_from_response,
    usd_to_nano,
)


def aggregate(
    email="alph…",
    model="claude-opus-4-8",
    tier="fast",
    input_tokens=10,
    cache_read_tokens=0,
    cache_write_5m_tokens=0,
    cache_write_1h_tokens=0,
    output_tokens=2,
    search_queries=0,
    total=1_000,
):
    return {
        "email": email,
        "model": model,
        "service_tier": tier,
        "inference_geo": "global",
        "tariff_schedule_id": "tariff-v1",
        "input_tokens": str(input_tokens),
        "cache_read_tokens": str(cache_read_tokens),
        "cache_write_5m_tokens": str(cache_write_5m_tokens),
        "cache_write_1h_tokens": str(cache_write_1h_tokens),
        "output_tokens": str(output_tokens),
        "search_queries": str(search_queries),
        "api_input_nanousd": "100",
        "api_cache_read_nanousd": "0",
        "api_cache_write_5m_nanousd": "0",
        "api_cache_write_1h_nanousd": "0",
        "api_output_nanousd": "900",
        "api_search_nanousd": "0",
        "api_total_nanousd": str(total),
    }


def capacity(rows):
    return {"calibration_evidence": rows}


class EvidenceTests(unittest.TestCase):
    def test_exact_turn_is_attributed_despite_unrelated_concurrent_evidence(self):
        before = evidence_rows(capacity([aggregate(), aggregate(email="beta…", model="x")]))
        after = evidence_rows(
            capacity(
                [
                    aggregate(input_tokens=17, output_tokens=5, total=2_500),
                    aggregate(email="beta…", model="x", input_tokens=99, total=9_000),
                ]
            )
        )
        usage = {
            "input_tokens": 7,
            "cache_read_tokens": 0,
            "cache_write_5m_tokens": 0,
            "cache_write_1h_tokens": 0,
            "output_tokens": 3,
            "search_queries": 0,
        }
        matched = attribute_exact_turn(before, after, usage, "claude-opus-4-8", "fast")
        self.assertIsNotNone(matched)
        profile, delta, _ = matched
        self.assertEqual(profile, "alph…")
        self.assertEqual(delta["api_total_nanousd"], 1_500)

    def test_same_aggregate_concurrency_fails_closed_instead_of_guessing(self):
        before = evidence_rows(capacity([aggregate()]))
        after = evidence_rows(capacity([aggregate(input_tokens=18, output_tokens=5, total=3_000)]))
        usage = {
            "input_tokens": 7,
            "cache_read_tokens": 0,
            "cache_write_5m_tokens": 0,
            "cache_write_1h_tokens": 0,
            "output_tokens": 3,
            "search_queries": 0,
        }
        self.assertIsNone(
            attribute_exact_turn(before, after, usage, "claude-opus-4-8", "fast")
        )

    def test_aggregate_moving_backwards_is_rejected(self):
        before = evidence_rows(capacity([aggregate(total=2_000)]))
        after = evidence_rows(capacity([aggregate(total=1_000)]))
        with self.assertRaisesRegex(CalibrationError, "moved backwards"):
            row_deltas(before, after)

    def test_response_usage_preserves_every_claude_token_class(self):
        parsed = usage_from_response(
            {
                "usage": {
                    "input_tokens": 11,
                    "cache_read_input_tokens": 12,
                    "cache_creation_input_tokens": 30,
                    "cache_creation": {
                        "ephemeral_5m_input_tokens": 13,
                        "ephemeral_1h_input_tokens": 17,
                    },
                    "output_tokens": 14,
                    "server_tool_use": {"web_search_requests": 2},
                }
            }
        )
        self.assertEqual(
            parsed,
            {
                "input_tokens": 11,
                "cache_read_tokens": 12,
                "cache_write_5m_tokens": 13,
                "cache_write_1h_tokens": 17,
                "output_tokens": 14,
                "search_queries": 2,
            },
        )

    def test_response_speed_is_the_authoritative_service_tier(self):
        self.assertEqual(response_service_tier({"usage": {"speed": "fast"}}), "fast")
        self.assertEqual(response_service_tier({"usage": {"speed": "standard"}}), "standard")
        self.assertEqual(response_service_tier({"usage": {}}), "standard")


class BudgetTests(unittest.TestCase):
    def test_budget_guard_checks_every_possible_rebind_target_before_dispatch(self):
        budget = ProfileBudget.for_profiles(["a…", "b…"], 40_000_000_000)
        budget.spent_nano["a…"] = 39_000_000_000
        budget.spent_nano["b…"] = 1_000_000_000
        with self.assertRaisesRegex(CalibrationError, "a…"):
            budget.require_room_for_any_routing(1_000_000_001)
        self.assertEqual(budget.spent_nano["a…"], 39_000_000_000)

    def test_actual_charge_cannot_cross_per_profile_limit(self):
        budget = ProfileBudget.for_profiles(["a…"], 100)
        budget.charge("a…", 60)
        with self.assertRaisesRegex(CalibrationError, "exceeded budget"):
            budget.charge("a…", 41)

    def test_exact_profile_budget_does_not_block_other_targets(self):
        budget = ProfileBudget.for_profiles(["a…", "b…"], 100)
        budget.spent_nano["a…"] = 99
        budget.require_room_for_profile("b…", 100)
        with self.assertRaisesRegex(CalibrationError, "a…"):
            budget.require_room_for_profile("a…", 2)

    def test_count_tokens_bound_covers_cache_miss_output_and_search(self):
        rates = TokenRates(10, 1, 12, 20, 30, 1_000)
        self.assertEqual(
            request_upper_bound_nano(100, 5, 1, rates, "1h"),
            100 * 20 + 5 * 30 + 1_000,
        )

    def test_count_body_omits_server_web_search_rejected_by_count_tokens(self):
        body = {
            "model": "claude-fable-5",
            "messages": [{"role": "user", "content": "search"}],
            "tools": [
                {"type": "web_search_20250305", "name": "web_search", "max_uses": 1}
            ],
            "tool_choice": {"type": "auto"},
            "max_tokens": 128,
        }
        counted = count_body(body)
        self.assertNotIn("tools", counted)
        self.assertNotIn("tool_choice", counted)
        self.assertNotIn("max_tokens", counted)

    def test_count_body_preserves_ordinary_client_tools(self):
        ordinary = {
            "name": "get_weather",
            "description": "Get the weather",
            "input_schema": {"type": "object", "properties": {}},
        }
        body = {
            "model": "claude-fable-5",
            "messages": [{"role": "user", "content": "weather"}],
            "tools": [
                ordinary,
                {"type": "web_search_20250305", "name": "web_search", "max_uses": 1},
            ],
            "tool_choice": {"type": "auto"},
        }
        counted = count_body(body)
        self.assertEqual(counted["tools"], [ordinary])
        self.assertEqual(counted["tool_choice"], {"type": "auto"})

    def test_guarded_tokens_add_full_server_tool_json_byte_bound(self):
        server_tools = [
            {"type": "web_search_20250305", "name": "web_search", "max_uses": 1}
        ]
        body = {"tools": server_tools}
        expected_bound = len(
            json.dumps(server_tools, separators=(",", ":")).encode("utf-8")
        )
        self.assertEqual(guarded_input_tokens(body, 17), 17 + expected_bound)

    def test_guarded_tokens_are_unchanged_without_server_tools(self):
        body = {
            "tools": [
                {
                    "name": "get_weather",
                    "input_schema": {"type": "object", "properties": {}},
                }
            ]
        }
        self.assertEqual(guarded_input_tokens(body, 17), 17)

    def test_usd_parser_is_integer_only_and_caps_are_exact(self):
        self.assertEqual(usd_to_nano("40"), 40_000_000_000)
        self.assertEqual(usd_to_nano("0.000000001"), 1)
        with self.assertRaises(CalibrationError):
            usd_to_nano("1e2")

    def test_degraded_delivery_stops_before_another_paid_turn(self):
        require_healthy_delivery(
            {
                "calibration_delivery": {
                    "pending_events": 0,
                    "dropped_events": 0,
                    "persistence_ok": True,
                }
            },
            require_empty=True,
        )
        with self.assertRaisesRegex(CalibrationError, "persistence is degraded"):
            require_healthy_delivery(
                {
                    "calibration_delivery": {
                        "pending_events": 2,
                        "dropped_events": 0,
                        "persistence_ok": False,
                    }
                },
                require_empty=False,
            )
        with self.assertRaisesRegex(CalibrationError, "pending events"):
            require_healthy_delivery(
                {
                    "calibration_delivery": {
                        "pending_events": 1,
                        "dropped_events": 0,
                        "persistence_ok": True,
                    }
                },
                require_empty=True,
            )


class CatalogueAndPlanTests(unittest.TestCase):
    def setUp(self):
        self.payload = {
            "conversion_models": [
                {
                    "id": "claude-opus-4-8",
                    "web_search_nanousd_per_request": "10000000",
                    "tiers": [
                        {
                            "id": "standard",
                            "input_nanousd_per_token": "5000",
                            "cache_read_nanousd_per_token": "500",
                            "cache_write_5m_nanousd_per_token": "6250",
                            "cache_write_1h_nanousd_per_token": "10000",
                            "output_nanousd_per_token": "25000",
                        },
                        {
                            "id": "fast",
                            "input_nanousd_per_token": "10000",
                            "cache_read_nanousd_per_token": "1000",
                            "cache_write_5m_nanousd_per_token": "12500",
                            "cache_write_1h_nanousd_per_token": "20000",
                            "output_nanousd_per_token": "50000",
                        },
                    ],
                },
                {
                    "id": "claude-opus-4-7",
                    "web_search_nanousd_per_request": "10000000",
                    "tiers": [
                        {
                            "id": "standard",
                            "input_nanousd_per_token": "5000",
                            "cache_read_nanousd_per_token": "500",
                            "cache_write_5m_nanousd_per_token": "6250",
                            "cache_write_1h_nanousd_per_token": "10000",
                            "output_nanousd_per_token": "25000",
                        },
                    ],
                },
            ]
        }

    def test_alias_resolution_is_narrow_and_unknown_models_use_global_ceiling(self):
        catalog, ceiling = rate_catalog(self.payload)
        self.assertEqual(
            canonical_rate_id("claude-opus-5", {key[0] for key in catalog}),
            "claude-opus-4-8",
        )
        self.assertEqual(
            canonical_rate_id("claude-opus-4-5-20251101", {key[0] for key in catalog}),
            "claude-opus-4-8",
        )
        self.assertEqual(
            rates_for_model(catalog, ceiling, "future-model", "standard").output_nano,
            50_000,
        )

    def test_coverage_contains_every_model_tier_and_token_class(self):
        catalog, _ = rate_catalog(self.payload)
        models = [
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-5-20251101",
        ]
        legs = build_coverage_legs(models, 4_096, catalog)
        fresh = {(leg.model, leg.tier) for leg in legs if leg.kind == "fresh"}
        self.assertEqual(
            fresh,
            {
                ("claude-opus-4-8", "standard"),
                ("claude-opus-4-8", "fast"),
                ("claude-opus-4-7", "standard"),
                ("claude-opus-4-5-20251101", "standard"),
            },
        )
        for model, tier in fresh:
            pair = [leg for leg in legs if leg.model == model and leg.tier == tier]
            self.assertEqual(len(pair), 6)
            self.assertEqual(
                {(leg.cache_ttl, leg.cache_phase) for leg in pair if leg.kind == "cache"},
                {("5m", "write"), ("5m", "read"), ("1h", "write"), ("1h", "read")},
            )
            self.assertEqual(sum(leg.kind == "web" for leg in pair), 1)

    def test_cache_payload_is_shared_within_a_run_and_unique_between_runs(self):
        write = Leg(
            name="cache-write",
            model="claude-fable-5",
            tier="standard",
            kind="cache",
            cache_ttl="1h",
            cache_id="coverage-fable-1h",
            cache_phase="write",
        )
        read = dataclasses.replace(write, name="cache-read", cache_phase="read")

        write_body = body_for_leg(write, "run-a")
        read_body = body_for_leg(read, "run-a")
        next_run_body = body_for_leg(write, "run-b")

        self.assertEqual(write_body["system"], read_body["system"])
        self.assertNotEqual(write_body["system"], next_run_body["system"])

    def test_profitability_is_sorted_by_observed_api_dollars_per_quota(self):
        rows = model_profitability(
            [
                {
                    "served_model": "cheap",
                    "tier": "standard",
                    "actual_nano": "100",
                    "fraction_delta_5h": 100,
                    "fraction_delta_7d": 100,
                },
                {
                    "served_model": "good",
                    "tier": "fast",
                    "actual_nano": "500",
                    "fraction_delta_5h": 100,
                    "fraction_delta_7d": 100,
                },
            ]
        )
        self.assertEqual(rows[0]["model"], "good")

    def test_any_token_class_miss_makes_the_final_report_incomplete(self):
        self.assertIsNone(coverage_failure([{"leg": "fresh:a", "coverage_ok": True}]))
        self.assertEqual(
            coverage_failure(
                [
                    {"leg": "fresh:a", "coverage_ok": True},
                    {"leg": "cache-read:a", "coverage_ok": False},
                ]
            ),
            "token-class coverage incomplete for 1 leg: cache-read:a",
        )


class ProductionSshClientTests(unittest.TestCase):
    @patch("tools.claude_calibration.run_live.subprocess.run")
    def test_admin_secret_stays_remote_and_target_is_bounded(self, run):
        run.return_value.returncode = 0
        run.return_value.stdout = b'{"input_tokens":17}\n200'
        run.return_value.stderr = b""
        client = ProductionSshJsonHttpClient(30)

        self.assertEqual(
            client.request(
                "/v1/messages/count_tokens",
                method="POST",
                body={"model": "claude-haiku-4-5", "messages": []},
                session_id="123e4567-e89b-42d3-a456-426614174000",
                target_profile="besp…",
            ),
            {"input_tokens": 17},
        )
        command = run.call_args.args[0]
        self.assertEqual(command[:2], ["ssh", "apitokensale"])
        self.assertIn("${CLAUDE_API_KEYS%%,*}", command[2])
        self.assertIn("x-apitoken-calibration-profile: besp", command[2])
        self.assertIn("fast-mode-2026-02-01", command[2])
        self.assertNotIn("APITOKEN_API_KEY", command[2])

        with self.assertRaisesRegex(CalibrationError, "bounded profile hint"):
            client.request("/v1/models", target_profile="too-long…")


if __name__ == "__main__":
    unittest.main()
