from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import flywheel_runner as runner  # noqa: E402


class FlywheelRunnerStateScopeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.config_dir = self.root / ".skill_flywheel"
        self.config_dir.mkdir(parents=True, exist_ok=True)
        self.paths = runner.RunnerPaths(
            config_dir=self.config_dir,
            state_file=self.config_dir / "runner_state.json",
            progress_file=self.config_dir / "progress.txt",
            log_dir=self.config_dir / "runner_logs",
        )
        self.paths.log_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def write_cycle(self, name: str, decision_payload: dict) -> Path:
        cycle_dir = self.config_dir / "cycles" / name / "logs"
        cycle_dir.mkdir(parents=True, exist_ok=True)
        runner.write_json(cycle_dir / "decision.json", decision_payload)
        return cycle_dir.parent

    def make_state(self, baseline_cycle_name: str | None) -> dict:
        return runner.build_initial_state(
            repo_root=self.root,
            target_skill_path=self.root / "target-skill",
            task_text="test task",
            task_source_path=None,
            task_label="test-task",
            tool="codex",
            baseline_cycle_name=baseline_cycle_name,
        )

    def test_sync_state_ignores_baseline_cycle_decision(self) -> None:
        self.write_cycle(
            "cycle-20260326-010752",
            {
                "hypothesis_status": "supported",
                "key_evidence": ["old evidence"],
                "minimal_actions": ["stop"],
                "continue_next_cycle": False,
                "next_question": "historical stop",
            },
        )
        state = self.make_state("cycle-20260326-010752")

        synced_state, substantive = runner.sync_state_from_latest_cycle(self.paths, state)

        self.assertFalse(substantive)
        self.assertEqual("active", synced_state["status"])
        self.assertTrue(synced_state["continue_next_iteration"])
        self.assertIsNone(synced_state["last_cycle"])

    def test_reconcile_resets_false_completion_without_new_cycle(self) -> None:
        state = self.make_state("cycle-20260326-010752")
        state["status"] = "complete"
        state["continue_next_iteration"] = False
        state["last_cycle"] = str(self.config_dir / "cycles" / "cycle-20260326-010752")
        state["last_decision"] = "stop"
        state["last_summary"] = "historical stop"

        reconciled = runner.reconcile_state_with_session_scope(self.paths, state)

        self.assertEqual("active", reconciled["status"])
        self.assertTrue(reconciled["continue_next_iteration"])
        self.assertIsNone(reconciled["last_cycle"])
        self.assertEqual("", reconciled["last_decision"])
        self.assertIn("historical decisions are context only", reconciled["last_summary"])

    def test_sync_state_accepts_post_baseline_cycle_decision(self) -> None:
        self.write_cycle(
            "cycle-20260326-010900",
            {
                "hypothesis_status": "supported",
                "key_evidence": ["new evidence"],
                "minimal_actions": ["stop"],
                "continue_next_cycle": False,
                "next_question": "new stop",
            },
        )
        state = self.make_state("cycle-20260326-010752")

        synced_state, substantive = runner.sync_state_from_latest_cycle(self.paths, state)

        self.assertTrue(substantive)
        self.assertEqual("complete", synced_state["status"])
        self.assertFalse(synced_state["continue_next_iteration"])
        self.assertTrue(str(synced_state["last_cycle"]).endswith("cycle-20260326-010900"))
        self.assertEqual("supported", synced_state["last_decision"])
        self.assertEqual("new stop", synced_state["last_summary"])

    def test_latest_substantive_cycle_name_ignores_placeholder_latest_cycle(self) -> None:
        self.write_cycle(
            "cycle-20260326-010752",
            {
                "hypothesis_status": "supported",
                "key_evidence": ["real stop"],
                "minimal_actions": ["stop"],
                "continue_next_cycle": False,
                "next_question": "done",
            },
        )
        self.write_cycle(
            "cycle-20260326-010900",
            {
                "hypothesis_status": "unknown",
                "key_evidence": [],
                "minimal_actions": [],
                "continue_next_cycle": False,
                "next_question": "",
            },
        )

        self.assertEqual("cycle-20260326-010752", runner.latest_substantive_cycle_name(self.config_dir))

    def test_sync_state_resumes_placeholder_post_baseline_cycle(self) -> None:
        self.write_cycle(
            "cycle-20260326-010752",
            {
                "hypothesis_status": "supported",
                "key_evidence": ["real stop"],
                "minimal_actions": ["stop"],
                "continue_next_cycle": False,
                "next_question": "done",
            },
        )
        self.write_cycle(
            "cycle-20260326-010900",
            {
                "hypothesis_status": "unknown",
                "key_evidence": [],
                "minimal_actions": [],
                "continue_next_cycle": False,
                "next_question": "",
            },
        )

        state = self.make_state(runner.latest_substantive_cycle_name(self.config_dir))
        synced_state, substantive = runner.sync_state_from_latest_cycle(self.paths, state)

        self.assertFalse(substantive)
        self.assertTrue(str(synced_state["last_cycle"]).endswith("cycle-20260326-010900"))
        self.assertEqual("active", synced_state["status"])
        self.assertTrue(synced_state["continue_next_iteration"])
        self.assertEqual(1, synced_state["idle_iteration_count"])

    def test_bootstrap_state_restores_active_placeholder_cycle(self) -> None:
        self.write_cycle(
            "cycle-20260326-010752",
            {
                "hypothesis_status": "supported",
                "key_evidence": ["real stop"],
                "minimal_actions": ["stop"],
                "continue_next_cycle": False,
                "next_question": "done",
            },
        )
        self.write_cycle(
            "cycle-20260326-010900",
            {
                "hypothesis_status": "unknown",
                "key_evidence": [],
                "minimal_actions": [],
                "continue_next_cycle": False,
                "next_question": "",
            },
        )
        state = self.make_state(runner.latest_substantive_cycle_name(self.config_dir))

        bootstrapped, substantive = runner.bootstrap_state_for_session(self.paths, state)

        self.assertFalse(substantive)
        self.assertTrue(str(bootstrapped["last_cycle"]).endswith("cycle-20260326-010900"))
        self.assertEqual("active", bootstrapped["status"])

    def test_codex_command_disables_chrome_devtools_mcp(self) -> None:
        with mock.patch.object(runner, "resolve_executable", return_value="codex"):
            command = runner.build_tool_command("codex")

        self.assertEqual("codex", command[0])
        self.assertIn("mcp_servers.chrome-devtools.enabled=false", command)
        self.assertIn('model_reasoning_effort="medium"', command)
        self.assertIn("--dangerously-bypass-approvals-and-sandbox", command)
        self.assertEqual("-", command[-1])

    def test_dry_run_accumulates_iteration_count_across_invocations(self) -> None:
        repo_root = self.root / "repo"
        repo_root.mkdir(parents=True, exist_ok=True)
        target_skill = self.root / "target-skill"
        task_dir = target_skill / ".skill_flywheel" / "tasks"
        task_dir.mkdir(parents=True, exist_ok=True)
        (task_dir / "autonomous-self-improve.md").write_text("test task\n", encoding="utf-8")

        base_argv = [
            "flywheel_runner.py",
            "--repo-root",
            str(repo_root),
            "--target-skill-path",
            str(target_skill),
            "--task-file",
            "autonomous-self-improve.md",
            "--dry-run",
            "--state-file",
            str(self.paths.state_file),
            "--progress-file",
            str(self.paths.progress_file),
            "--log-dir",
            str(self.paths.log_dir),
        ]

        with mock.patch.object(sys, "argv", base_argv):
            self.assertEqual(0, runner.main())
        state_after_first_run = runner.read_json(self.paths.state_file)
        self.assertIsNotNone(state_after_first_run)
        self.assertEqual(1, state_after_first_run["iteration_count"])
        self.assertTrue((self.paths.log_dir / "iter_001_prompt.md").exists())

        with mock.patch.object(sys, "argv", base_argv):
            self.assertEqual(0, runner.main())
        state_after_second_run = runner.read_json(self.paths.state_file)
        self.assertIsNotNone(state_after_second_run)
        self.assertEqual(2, state_after_second_run["iteration_count"])
        self.assertTrue((self.paths.log_dir / "iter_002_prompt.md").exists())


if __name__ == "__main__":
    unittest.main()
