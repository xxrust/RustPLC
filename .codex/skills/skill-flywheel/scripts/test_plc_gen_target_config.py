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
        self.assertIn("complex-project-public-brief.md", config["artifact_paths"])
        self.assertIn("source-shape-selection.md", config["artifact_paths"])
        self.assertIn("delivery-asset-placeholder-replacement.md", config["artifact_paths"])
        self.assertIn("delivery-asset-write-map.md", config["artifact_paths"])
        self.assertIn("controller-io-modeling-guardrails.md", config["artifact_paths"])
        self.assertIn("legacy-io-model-removal.md", config["artifact_paths"])
        self.assertIn("operator-command-modeling.md", config["artifact_paths"])
        self.assertIn("confirmed-system-lowering.md", config["artifact_paths"])
        self.assertIn("intent-alignment-boundary.md", config["artifact_paths"])
        self.assertIn("delivery-status-contract.md", config["artifact_paths"])
        self.assertIn("optimization-surface.md", config["artifact_paths"])
        self.assertIn("scenario-toolchain-limitations.md", config["artifact_paths"])
        self.assertIn("scenario-friendly-guard-patterns.md", config["artifact_paths"])
        self.assertGreaterEqual(config["parallel_runs"], 2)

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
            self.assertTrue((cycle_dir / "public" / "complex-project-public-brief.md").exists())
            self.assertTrue((cycle_dir / "public" / "source-shape-selection.md").exists())
            self.assertTrue((cycle_dir / "public" / "delivery-asset-placeholder-replacement.md").exists())
            self.assertTrue((cycle_dir / "public" / "delivery-asset-write-map.md").exists())
            self.assertTrue((cycle_dir / "public" / "controller-io-modeling-guardrails.md").exists())
            self.assertTrue((cycle_dir / "public" / "legacy-io-model-removal.md").exists())
            self.assertTrue((cycle_dir / "public" / "operator-command-modeling.md").exists())
            self.assertTrue((cycle_dir / "public" / "confirmed-system-lowering.md").exists())
            self.assertTrue((cycle_dir / "public" / "intent-alignment-boundary.md").exists())
            self.assertTrue((cycle_dir / "public" / "delivery-status-contract.md").exists())
            self.assertTrue((cycle_dir / "public" / "optimization-surface.md").exists())
            self.assertTrue((cycle_dir / "public" / "scenario-toolchain-limitations.md").exists())
            self.assertTrue((cycle_dir / "public" / "scenario-friendly-guard-patterns.md").exists())

    def test_plc_gen_public_artifacts_capture_key_contracts(self) -> None:
        public_dir = PLC_GEN_CONFIG / "public"

        launchers = (public_dir / "scaffold-day1-launchers.md").read_text(encoding="utf-8")
        validation_order = (public_dir / "scaffold-day1-validation-order.md").read_text(encoding="utf-8")
        checklist = (public_dir / "scaffold-day1-checklist.md").read_text(encoding="utf-8")
        source_shape = (public_dir / "source-shape-selection.md").read_text(encoding="utf-8")
        placeholder_replacement = (public_dir / "delivery-asset-placeholder-replacement.md").read_text(encoding="utf-8")
        write_map = (public_dir / "delivery-asset-write-map.md").read_text(encoding="utf-8")
        controller_guardrails = (public_dir / "controller-io-modeling-guardrails.md").read_text(encoding="utf-8")
        legacy_io = (public_dir / "legacy-io-model-removal.md").read_text(encoding="utf-8")
        operator_commands = (public_dir / "operator-command-modeling.md").read_text(encoding="utf-8")
        intent_boundary = (public_dir / "intent-alignment-boundary.md").read_text(encoding="utf-8")
        delivery_status = (public_dir / "delivery-status-contract.md").read_text(encoding="utf-8")
        optimization = (public_dir / "optimization-surface.md").read_text(encoding="utf-8")

        self.assertIn("project-check", launchers)
        self.assertIn("project-check", validation_order)
        self.assertIn("blocked by toolchain limitation", validation_order)
        self.assertIn("project-check", checklist)
        self.assertIn(".bundle.toml", source_shape)
        self.assertIn("three_station_assembly", source_shape)
        self.assertIn("line", source_shape)
        self.assertIn("单机运行", source_shape)
        self.assertIn("Default Starter Flow", placeholder_replacement)
        self.assertIn("replace_me_after_authoring", placeholder_replacement)
        self.assertIn("UTF-8", placeholder_replacement)
        self.assertIn("delivery asset", placeholder_replacement)
        self.assertIn("docs/*.system.md", write_map)
        self.assertIn("main.bundle.toml", write_map)
        self.assertIn("intent_alignment.contract.json", write_map)
        self.assertIn("model_ref", controller_guardrails)
        self.assertIn("SCN-MAP-010", controller_guardrails)
        self.assertIn("SEM-108", legacy_io)
        self.assertIn("reserved for real hardware equipment", legacy_io)
        self.assertIn("selector_switch", operator_commands)
        self.assertIn("push_button", operator_commands)
        self.assertIn("authored sidecar", intent_boundary)
        self.assertIn("toolchain artifacts", intent_boundary)
        self.assertIn("required by default", intent_boundary)
        self.assertIn("lowercase SHA-256", intent_boundary)
        self.assertIn("validated with warnings", delivery_status)
        self.assertIn("toolchain artifacts", delivery_status)
        self.assertIn("replace_me_after_authoring", delivery_status)
        self.assertIn("lowercase SHA-256", delivery_status)
        self.assertIn("library", optimization)
        self.assertIn("CLI", optimization)
        self.assertIn("subcommand", optimization)

    def test_plc_gen_valid_fixtures_keep_operator_inputs_semantic(self) -> None:
        fixtures_dir = PLC_GEN_ROOT / "fixtures" / "valid"
        raw_digital_io = []
        push_button_fixtures = []
        selector_switch_fixtures = []

        for fixture_path in fixtures_dir.glob("*.plc"):
            source = fixture_path.read_text(encoding="utf-8")
            if ": digital_input {" in source or ": digital_output {" in source:
                raw_digital_io.append(fixture_path.name)
            if 'subtype: "push_button"' in source:
                push_button_fixtures.append(fixture_path.name)
            if 'subtype: "selector_switch"' in source:
                selector_switch_fixtures.append(fixture_path.name)

        self.assertEqual([], raw_digital_io)
        self.assertNotEqual([], push_button_fixtures)
        self.assertNotEqual([], selector_switch_fixtures)


if __name__ == "__main__":
    unittest.main()
