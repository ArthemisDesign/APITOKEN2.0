#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

MODULE = Path(__file__).parents[1] / "tools" / "codex-native" / "probe-live.py"
SPEC = importlib.util.spec_from_file_location("codex_native_probe", MODULE)
assert SPEC and SPEC.loader
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class CodexNativeProbePrivacyTest(unittest.TestCase):
    def test_usage_projection_contains_shape_but_no_identity(self) -> None:
        payload = {
            "user_id": "secret-user",
            "account_id": "secret-account",
            "email": "secret@example.com",
            "plan_type": "pro",
            "rate_limit": {
                "allowed": True,
                "primary_window": {"used_percent": 1, "reset_at": 2},
                "secondary_window": None,
            },
            "additional_rate_limits": [
                {"limit_name": "private-feature", "identity": "secret-subject"}
            ],
            "credits": {"balance": "0", "private_account_id": "secret-account"},
            "spend_control": {"reached": False, "owner": "secret-user"},
        }
        projection = probe.redacted_usage_projection(payload)
        encoded = json.dumps(projection, sort_keys=True)
        for secret in (
            "secret-user",
            "secret-account",
            "secret@example.com",
            "secret-subject",
            "private-feature",
        ):
            self.assertNotIn(secret, encoded)
        self.assertEqual(projection["plan_type"], "pro")
        self.assertEqual(projection["additional_rate_limit_count"], 1)
        self.assertEqual(projection["primary_window_keys"], ["reset_at", "used_percent"])
        self.assertTrue(projection["spend_control_present"])


if __name__ == "__main__":
    unittest.main()
