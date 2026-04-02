from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_FLYWHEEL_ROOT = SCRIPT_DIR.parent
REPO_ROOT = SKILL_FLYWHEEL_ROOT.parents[2]
PLC_SYSTEM_ROOT = REPO_ROOT / ".codex" / "skills" / "plc-system"
PLC_SYSTEM_CONFIG = PLC_SYSTEM_ROOT / ".skill_flywheel"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import init_public_surface  # noqa: E402


class PlcSystemTargetConfigTests(unittest.TestCase):
    def test_plc_system_public_surface_uses_artifact_paths_schema(self) -> None:
        config = json.loads((PLC_SYSTEM_CONFIG / "public_surface.json").read_text(encoding="utf-8"))

        self.assertIn("artifact_paths", config)
        self.assertNotIn("include_paths", config)
        self.assertIn("system-day1-draft-workflow.md", config["artifact_paths"])

    def test_plc_system_init_public_surface_exports_day1_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            cycle_dir = Path(temp_dir) / "cycle-test"
            argv = [
                "init_public_surface.py",
                "--repo-root",
                str(REPO_ROOT),
                "--target-skill-path",
                str(PLC_SYSTEM_ROOT),
                "--task-file",
                "system-day1.md",
                "--cycle-dir",
                str(cycle_dir),
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_public_surface.main())

            self.assertTrue((cycle_dir / "context" / "program.md").exists())
            self.assertTrue((cycle_dir / "public" / "system-day1-draft-workflow.md").exists())
            self.assertTrue((cycle_dir / "public" / "system-day1-required-sections.md").exists())
            self.assertTrue((cycle_dir / "public" / "system-day1-concurrency-guardrails.md").exists())
            self.assertTrue((cycle_dir / "public" / "system-day1-handoff-gate.md").exists())
            self.assertTrue((cycle_dir / "public" / "system-day1-checklist.md").exists())


if __name__ == "__main__":
    unittest.main()
