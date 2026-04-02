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
PLC_GEN_ROOT = REPO_ROOT / ".codex" / "skills" / "plc-gen"
PLC_GEN_CONFIG = PLC_GEN_ROOT / ".skill_flywheel"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import init_public_surface  # noqa: E402


class PlcGenTargetConfigTests(unittest.TestCase):
    def test_plc_gen_public_surface_uses_artifact_paths_schema(self) -> None:
        config = json.loads((PLC_GEN_CONFIG / "public_surface.json").read_text(encoding="utf-8"))

        self.assertIn("artifact_paths", config)
        self.assertNotIn("include_paths", config)
        self.assertIn("scaffold-day1-launchers.md", config["artifact_paths"])
        self.assertIn("confirmed-system-lowering.md", config["artifact_paths"])
        self.assertIn("scenario-toolchain-limitations.md", config["artifact_paths"])
        self.assertIn("scenario-friendly-guard-patterns.md", config["artifact_paths"])

    def test_plc_gen_init_public_surface_exports_day1_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            cycle_dir = Path(temp_dir) / "cycle-test"
            argv = [
                "init_public_surface.py",
                "--repo-root",
                str(REPO_ROOT),
                "--target-skill-path",
                str(PLC_GEN_ROOT),
                "--task-file",
                "scaffold-day1.md",
                "--cycle-dir",
                str(cycle_dir),
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_public_surface.main())

            self.assertTrue((cycle_dir / "context" / "program.md").exists())
            self.assertTrue((cycle_dir / "public" / "scaffold-day1-launchers.md").exists())
            self.assertTrue((cycle_dir / "public" / "scaffold-day1-validation-order.md").exists())
            self.assertTrue((cycle_dir / "public" / "scaffold-day1-checklist.md").exists())
            self.assertTrue((cycle_dir / "public" / "confirmed-system-lowering.md").exists())
            self.assertTrue((cycle_dir / "public" / "scenario-toolchain-limitations.md").exists())
            self.assertTrue((cycle_dir / "public" / "scenario-friendly-guard-patterns.md").exists())


if __name__ == "__main__":
    unittest.main()
