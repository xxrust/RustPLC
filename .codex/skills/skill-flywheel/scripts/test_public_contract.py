from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_ROOT = SCRIPT_DIR.parent
CONFIG_DIR = SKILL_ROOT / ".skill_flywheel"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import init_public_surface  # noqa: E402


class PublicContractTests(unittest.TestCase):
    def test_autonomous_self_improve_artifacts_are_exposed(self) -> None:
        config = json.loads((CONFIG_DIR / "public_surface.json").read_text(encoding="utf-8"))

        self.assertIn("autonomous-self-improve-command.txt", config["artifact_paths"])
        self.assertIn("autonomous-self-improve-checklist.md", config["artifact_paths"])
        self.assertIn("autonomous-self-improve-observe-command.txt", config["artifact_paths"])

        for artifact in config["artifact_paths"]:
            self.assertTrue(
                (CONFIG_DIR / "public" / artifact).exists(),
                f"missing public artifact: {artifact}",
            )

    def test_profile_matches_five_round_shell_goal(self) -> None:
        profile = (CONFIG_DIR / "profile.md").read_text(encoding="utf-8")

        self.assertIn("5 轮外层迭代", profile)
        self.assertIn("runner_state.json", profile)
        self.assertIn("progress.txt", profile)
        self.assertNotIn("最小 cycle", profile)

    def test_init_public_surface_exports_autonomous_checklist(self) -> None:
        repo_root = SKILL_ROOT.parents[3]
        with tempfile.TemporaryDirectory() as temp_dir:
            cycle_dir = Path(temp_dir) / "cycle-test"
            argv = [
                "init_public_surface.py",
                "--repo-root",
                str(repo_root),
                "--target-skill-path",
                str(SKILL_ROOT),
                "--task-file",
                "autonomous-self-improve.md",
                "--cycle-dir",
                str(cycle_dir),
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_public_surface.main())

            self.assertTrue((cycle_dir / "public" / "autonomous-self-improve-command.txt").exists())
            self.assertTrue((cycle_dir / "public" / "autonomous-self-improve-checklist.md").exists())

    def test_autonomous_checklist_points_to_closeout_path_for_active_cycle(self) -> None:
        checklist = (CONFIG_DIR / "public" / "autonomous-self-improve-checklist.md").read_text(
            encoding="utf-8"
        )

        self.assertIn("single-agent-closeout-checklist.md", checklist)
        self.assertIn("do not open a new cycle", checklist)
        self.assertIn("autonomous-self-improve-observe-command.txt", checklist)

    def test_single_agent_closeout_checklist_requires_structured_decision_sync(self) -> None:
        checklist = (CONFIG_DIR / "public" / "single-agent-closeout-checklist.md").read_text(
            encoding="utf-8"
        )

        self.assertIn("decision.json", checklist)
        self.assertIn("shell runner", checklist)
        self.assertIn("hypothesis_status", checklist)
        self.assertIn("continue_next_cycle", checklist)
        self.assertIn("sync_cycle_artifacts.py", checklist)
        self.assertIn("research_question", checklist)
        self.assertIn("decision_summary", checklist)


if __name__ == "__main__":
    unittest.main()
