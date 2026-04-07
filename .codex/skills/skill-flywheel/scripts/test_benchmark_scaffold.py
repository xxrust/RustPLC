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

import init_benchmark_case  # noqa: E402


class BenchmarkScaffoldTests(unittest.TestCase):
    def test_init_benchmark_case_creates_root_manifest_and_case_layout(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "benchmarks" / "generic-suite"
            argv = [
                "init_benchmark_case.py",
                "--benchmark-root",
                str(benchmark_root),
                "--benchmark-name",
                "generic-suite",
                "--case-id",
                "case-001",
                "--split",
                "dev",
                "--status",
                "draft",
                "--question",
                "Verify a generic benchmark prompt.",
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_benchmark_case.main())

            manifest = json.loads((benchmark_root / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual("generic-suite", manifest["benchmark_name"])
            self.assertEqual(1, manifest["schema_version"])
            self.assertEqual("freeze, retire, and split cases outside the active optimization round", manifest["governance"]["curator_role"])
            self.assertEqual(1, len(manifest["cases"]))

            case_dir = benchmark_root / "cases" / "dev" / "case-001"
            self.assertTrue((case_dir / "case.json").exists())
            self.assertTrue((case_dir / "public" / "prompt.md").exists())
            self.assertTrue((case_dir / "public" / "inputs" / "README.md").exists())
            self.assertTrue((case_dir / "hidden" / "rubric.json").exists())
            self.assertTrue((case_dir / "hidden" / "oracle.json").exists())
            self.assertTrue((case_dir / "evaluation" / "result.json").exists())

            case_payload = json.loads((case_dir / "case.json").read_text(encoding="utf-8"))
            self.assertEqual("case-001", case_payload["case_id"])
            self.assertEqual("dev", case_payload["split"])

            oracle = json.loads((case_dir / "hidden" / "oracle.json").read_text(encoding="utf-8"))
            self.assertTrue(oracle["hidden_to_flywheel"])

    def test_init_benchmark_case_appends_second_case_without_reinitializing_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "generic-benchmark"
            first = [
                "init_benchmark_case.py",
                "--benchmark-root",
                str(benchmark_root),
                "--case-id",
                "case-001",
            ]
            second = [
                "init_benchmark_case.py",
                "--benchmark-root",
                str(benchmark_root),
                "--case-id",
                "case-002",
                "--split",
                "holdout",
                "--status",
                "frozen",
            ]

            with mock.patch.object(sys, "argv", first):
                self.assertEqual(0, init_benchmark_case.main())
            with mock.patch.object(sys, "argv", second):
                self.assertEqual(0, init_benchmark_case.main())

            manifest = json.loads((benchmark_root / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(2, len(manifest["cases"]))
            self.assertEqual({"dev", "holdout"}, {item["split"] for item in manifest["cases"]})

    def test_init_benchmark_case_rejects_duplicate_case_id(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            benchmark_root = Path(temp_dir) / "generic-benchmark"
            argv = [
                "init_benchmark_case.py",
                "--benchmark-root",
                str(benchmark_root),
                "--case-id",
                "case-001",
            ]

            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(0, init_benchmark_case.main())

            with mock.patch.object(sys, "argv", argv):
                with self.assertRaisesRegex(ValueError, "case 已存在"):
                    init_benchmark_case.main()


if __name__ == "__main__":
    unittest.main()
