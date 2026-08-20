import contextlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.gemini_batch import run_live


class DryRunTests(unittest.TestCase):
    def test_default_is_network_free_dry_run(self):
        stdout = io.StringIO()
        with mock.patch.object(subprocess, "run") as subprocess_run, contextlib.redirect_stdout(stdout):
            self.assertEqual(run_live.main([]), 0)
        subprocess_run.assert_not_called()
        plan = json.loads(stdout.getvalue())
        self.assertEqual(plan["schema"], run_live.PLAN_SCHEMA)
        self.assertEqual(plan["mode"], "dry-run")
        self.assertEqual(plan["network_requests"], 0)
        self.assertEqual(plan["authorized_budget_nanousd"], "10000000000")
        self.assertEqual(
            [scenario["name"] for scenario in plan["scenarios"]],
            [scenario.name for scenario in run_live.SCENARIOS],
        )
        self.assertFalse(plan["scenarios"][3]["paid_create"])

    def test_execute_requires_exact_sha_and_previous_checkpoint(self):
        for argv in (
            ["--execute"],
            ["--execute", "--implementation-sha", "a" * 40],
            ["--execute", "--previous-spend-nanousd", "0"],
            ["--execute", "--implementation-sha", "A" * 40, "--previous-spend-nanousd", "0"],
        ):
            with self.subTest(argv=argv), self.assertRaises(SystemExit):
                run_live.parse_args(argv)


class BudgetTests(unittest.TestCase):
    def test_exact_decimal_and_integer_budget_math(self):
        self.assertEqual(run_live.usd_to_nano("10"), 10_000_000_000)
        self.assertEqual(run_live.usd_to_nano("0.000000001"), 1)
        self.assertEqual(run_live.remaining_budget(1), 9_999_999_999)
        self.assertEqual(run_live.reserve_budget(9_000_000_000, [400_000_000, 600_000_000]), 0)
        for invalid in ("1e1", ".5", "-1", "0.0000000001", "x"):
            with self.subTest(invalid=invalid), self.assertRaises(run_live.RunnerError):
                run_live.usd_to_nano(invalid)

    def test_original_budget_and_holds_fail_closed(self):
        with self.assertRaises(run_live.RunnerError):
            run_live.remaining_budget(10_000_000_001)
        with self.assertRaises(run_live.RunnerError):
            run_live.remaining_budget(9_000_000_000, 1_000_000_001)
        with self.assertRaises(run_live.RunnerError):
            run_live.reserve_budget(9_500_000_000, [250_000_001, 250_000_000])


class CommandSecrecyTests(unittest.TestCase):
    def test_local_command_contains_names_not_secret_values(self):
        marker_api = "SECRET_API_MARKER"
        marker_panel = "SECRET_PANEL_MARKER"
        with mock.patch.dict(
            "os.environ",
            {
                "GEMINI_BATCH_STAGE5_API_KEY": marker_api,
                "CLAUDE_API_PANEL_KEY": marker_panel,
            },
            clear=False,
        ):
            argv = run_live.ssh_argv("apitokensale", 8794)
        rendered = " ".join(argv)
        self.assertNotIn(marker_api, rendered)
        self.assertNotIn(marker_panel, rendered)
        self.assertIn("/srv/claude-api/data/server.env", rendered)
        self.assertNotIn("Authorization:", rendered)
        self.assertNotIn("x-goog-api-key: sk-", rendered)

    def test_remote_helper_never_prints_keys_or_payloads(self):
        self.assertNotIn("print(BATCH_KEY", run_live.REMOTE_HELPER)
        self.assertNotIn("print(PANEL_KEY", run_live.REMOTE_HELPER)
        self.assertNotIn("'response': payload", run_live.REMOTE_HELPER)
        self.assertIn("diagnostic_projection", run_live.REMOTE_HELPER)
        self.assertIn("hold_nano", run_live.REMOTE_HELPER)


class AmbiguityTests(unittest.TestCase):
    def test_paid_create_timeout_is_ambiguous_and_not_retried(self):
        remote = run_live.Remote("apitokensale", 8794)
        with mock.patch.object(
            subprocess,
            "run",
            side_effect=subprocess.TimeoutExpired(remote.argv, 90),
        ) as subprocess_run:
            with self.assertRaises(run_live.AmbiguousPaidCreate):
                remote.call({"op": "create", "model": "gemini-2.5-flash", "items": 1}, paid_create=True)
        self.assertEqual(subprocess_run.call_count, 1)

    def test_paid_create_ssh_255_is_ambiguous_and_not_retried(self):
        remote = run_live.Remote("apitokensale", 8794)
        completed = subprocess.CompletedProcess(remote.argv, 255, stdout=b"", stderr=b"lost")
        with mock.patch.object(subprocess, "run", return_value=completed) as subprocess_run:
            with self.assertRaises(run_live.AmbiguousPaidCreate):
                remote.call({"op": "create", "model": "gemini-2.5-flash", "items": 1}, paid_create=True)
        self.assertEqual(subprocess_run.call_count, 1)

    def test_existing_checkpoint_cannot_resume(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "checkpoint.json"
            path.write_text("{}", encoding="utf-8")
            with self.assertRaises(run_live.RunnerError):
                run_live.checkpoint_path(str(path))


if __name__ == "__main__":
    unittest.main()
