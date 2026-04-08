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

import aggregate_benchmark_results  # noqa: E402
import init_benchmark_case  # noqa: E402
import init_benchmark_suite  # noqa: E402
import write_benchmark_result  # noqa: E402


class BenchmarkWorkflowTests(unittest.TestCase):
    def test_init_benchmark_suite_creates_governance_and_summary_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "suite"
            argv = [
                "init_benchmark_suite.py",
                "--benchmark-root",
                str(benchmark_root),
                "--benchmark-name",
                "generic-suite",
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_benchmark_suite.main())

            self.assertTrue((benchmark_root / "README.md").exists())
            self.assertTrue((benchmark_root / "governance" / "curator-notes.md").exists())
            self.assertTrue((benchmark_root / "governance" / "proposals.jsonl").exists())
            self.assertTrue((benchmark_root / "summaries" / "latest-summary.json").exists())
            self.assertTrue((benchmark_root / "summaries" / "latest-summary.md").exists())

            summary = json.loads((benchmark_root / "summaries" / "latest-summary.json").read_text(encoding="utf-8"))
            self.assertEqual("generic-suite", summary["benchmark_name"])

    def test_write_benchmark_result_updates_case_evaluation_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "suite"
            with mock.patch.object(
                sys,
                "argv",
                ["init_benchmark_case.py", "--benchmark-root", str(benchmark_root), "--case-id", "case-001"],
            ):
                self.assertEqual(0, init_benchmark_case.main())

            argv = [
                "write_benchmark_result.py",
                "--benchmark-root",
                str(benchmark_root),
                "--case-id",
                "case-001",
                "--run-label",
                "run-01",
                "--skill-revision",
                "rev-a",
                "--status",
                "completed",
                "--verdict",
                "blocked",
                "--summary",
                "A real blocker was detected.",
                "--blocker-classification",
                "public-surface-gap",
                "--metric",
                "question_count=3",
                "--metric",
                "truthful_blocker=true",
                "--evidence-path",
                "logs/run-01.md",
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, write_benchmark_result.main())

            result = json.loads(
                (benchmark_root / "cases" / "dev" / "case-001" / "evaluation" / "result.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual("run-01", result["run_label"])
            self.assertEqual("rev-a", result["skill_revision"])
            self.assertEqual("blocked", result["verdict"])
            self.assertEqual(3, result["metrics"]["question_count"])
            self.assertTrue(result["metrics"]["truthful_blocker"])

    def test_aggregate_benchmark_results_generates_split_and_blocker_summary(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "suite"

            with mock.patch.object(
                sys,
                "argv",
                ["init_benchmark_case.py", "--benchmark-root", str(benchmark_root), "--case-id", "case-001"],
            ):
                self.assertEqual(0, init_benchmark_case.main())
            with mock.patch.object(
                sys,
                "argv",
                [
                    "init_benchmark_case.py",
                    "--benchmark-root",
                    str(benchmark_root),
                    "--case-id",
                    "case-002",
                    "--split",
                    "holdout",
                ],
            ):
                self.assertEqual(0, init_benchmark_case.main())

            with mock.patch.object(
                sys,
                "argv",
                [
                    "write_benchmark_result.py",
                    "--benchmark-root",
                    str(benchmark_root),
                    "--case-id",
                    "case-001",
                    "--verdict",
                    "pass",
                    "--summary",
                    "passed",
                ],
            ):
                self.assertEqual(0, write_benchmark_result.main())

            with mock.patch.object(
                sys,
                "argv",
                [
                    "write_benchmark_result.py",
                    "--benchmark-root",
                    str(benchmark_root),
                    "--case-id",
                    "case-002",
                    "--verdict",
                    "blocked",
                    "--summary",
                    "blocked",
                    "--blocker-classification",
                    "toolchain-gap",
                ],
            ):
                self.assertEqual(0, write_benchmark_result.main())

            with mock.patch.object(
                sys,
                "argv",
                ["aggregate_benchmark_results.py", "--benchmark-root", str(benchmark_root)],
            ):
                self.assertEqual(0, aggregate_benchmark_results.main())

            summary = json.loads((benchmark_root / "summaries" / "latest-summary.json").read_text(encoding="utf-8"))
            self.assertEqual(2, summary["totals"]["cases"])
            self.assertEqual(1, summary["totals"]["pass"])
            self.assertEqual(1, summary["totals"]["blocked"])
            self.assertIn("dev", summary["by_split"])
            self.assertIn("holdout", summary["by_split"])
            self.assertEqual("toolchain-gap", summary["top_blockers"][0]["blocker_classification"])


if __name__ == "__main__":
    unittest.main()
