from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import sync_cycle_artifacts  # noqa: E402


class SyncCycleArtifactsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.cycle_dir = Path(self.temp_dir.name) / "cycle-20260326-000000"
        self.logs_dir = self.cycle_dir / "logs"
        self.logs_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def write_payload(self, name: str, payload: dict) -> None:
        (self.logs_dir / name).write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    def test_sync_rebuilds_markdown_from_json(self) -> None:
        self.write_payload(
            "pain-points.json",
            {
                "task": "task",
                "hypothesis_signal": "partial",
                "result_summary": "summary",
                "pain_points": [
                    {
                        "step": "step-a",
                        "blocker": "blocker-a",
                        "missing_item": "item-a",
                        "impact": "impact-a",
                    }
                ],
            },
        )
        self.write_payload(
            "root-cause.json",
            {
                "task": "task",
                "hypothesis_status": "supported",
                "findings": [
                    {
                        "pain_point": "pp-a",
                        "classification": "public-surface-gap",
                        "cause": "cause-a",
                        "minimal_fix": "fix-a",
                    }
                ],
            },
        )
        self.write_payload(
            "decision.json",
            {
                "research_question": "question-root",
                "hypothesis_status": "supported",
                "key_evidence": ["evidence-a"],
                "minimal_actions": ["action-a"],
                "continue_next_cycle": True,
                "classification": "code-gap",
                "decision_summary": "summary-a",
                "next_question": "question-a",
            },
        )

        argv = [
            "sync_cycle_artifacts.py",
            "--cycle-dir",
            str(self.cycle_dir),
            "--require-non-placeholder-decision",
            "--sync-experiments",
        ]
        with mock.patch.object(sys, "argv", argv):
            self.assertEqual(0, sync_cycle_artifacts.main())

        decision_md = (self.logs_dir / "decision.md").read_text(encoding="utf-8")
        experiments = (self.cycle_dir.parent.parent / "experiments.jsonl").read_text(encoding="utf-8")
        self.assertIn("question-root", decision_md)
        self.assertIn("supported", decision_md)
        self.assertIn("action-a", decision_md)
        self.assertIn("question-a", decision_md)
        self.assertIn('"cycle": "cycle-20260326-000000"', experiments)
        self.assertIn('"classification": "code-gap"', experiments)
        self.assertIn('"reason": "summary-a"', experiments)

    def test_sync_rejects_placeholder_decision_when_required(self) -> None:
        self.write_payload(
            "pain-points.json",
            {"task": "task", "hypothesis_signal": "unknown", "result_summary": "", "pain_points": []},
        )
        self.write_payload(
            "root-cause.json",
            {"task": "task", "hypothesis_status": "unknown", "findings": []},
        )
        self.write_payload(
            "decision.json",
            {
                "hypothesis_status": "unknown",
                "key_evidence": [],
                "minimal_actions": [],
                "continue_next_cycle": False,
                "next_question": "",
            },
        )

        argv = ["sync_cycle_artifacts.py", "--cycle-dir", str(self.cycle_dir), "--require-non-placeholder-decision"]
        with mock.patch.object(sys, "argv", argv):
            self.assertEqual(1, sync_cycle_artifacts.main())

    def test_sync_updates_existing_experiment_record_for_same_cycle(self) -> None:
        experiments_path = self.cycle_dir.parent.parent / "experiments.jsonl"
        experiments_path.parent.mkdir(parents=True, exist_ok=True)
        experiments_path.write_text(
            json.dumps(
                {
                    "cycle": "cycle-20260326-000000",
                    "question": "old-q",
                    "decision": "stop",
                    "reason": "old-r",
                    "classification": "old-c",
                },
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )
        self.write_payload(
            "pain-points.json",
            {"task": "task", "hypothesis_signal": "partial", "result_summary": "summary", "pain_points": []},
        )
        self.write_payload(
            "root-cause.json",
            {"task": "task", "hypothesis_status": "supported", "findings": []},
        )
        self.write_payload(
            "decision.json",
            {
                "research_question": "new-q",
                "hypothesis_status": "supported",
                "key_evidence": ["e1"],
                "minimal_actions": ["a1"],
                "continue_next_cycle": False,
                "classification": "validated",
                "decision_summary": "new-r",
                "next_question": "",
            },
        )

        self.assertEqual(0, sync_cycle_artifacts.sync_cycle_artifacts(self.cycle_dir, sync_experiments=True))

        content = experiments_path.read_text(encoding="utf-8").splitlines()
        self.assertEqual(1, len([line for line in content if line.strip()]))
        self.assertIn('"question": "new-q"', content[0])
        self.assertIn('"classification": "validated"', content[0])


if __name__ == "__main__":
    unittest.main()
