from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path


SPLITS = ("dev", "holdout", "canary")
STATUSES = ("draft", "frozen", "retired")
RESULT_VERDICTS = ("unknown", "pass", "fail", "blocked", "error")
RUN_STATUSES = ("not_run", "completed", "error")


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content if content.endswith("\n") else f"{content}\n", encoding="utf-8")


def write_json(path: Path, payload: dict) -> None:
    write_text(path, json.dumps(payload, indent=2, ensure_ascii=False))


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def default_manifest(benchmark_root: Path, benchmark_name: str) -> dict:
    return {
        "schema_version": 1,
        "benchmark_name": benchmark_name,
        "benchmark_root": str(benchmark_root.resolve()),
        "created_at_utc": utc_now(),
        "governance": {
            "proposer_role": "draft candidate cases only",
            "curator_role": "freeze, retire, and split cases outside the active optimization round",
            "judge_role": "read hidden rubric/oracle and write evaluation results",
            "flywheel_visibility": [
                "public inputs",
                "evaluation summaries",
                "aggregate metrics",
            ],
            "hidden_paths": [
                "cases/*/*/hidden",
            ],
            "frozen_case_rule": "active optimization rounds must not rewrite frozen cases",
        },
        "splits": {
            "dev": {"purpose": "daily iteration and hypothesis narrowing"},
            "holdout": {"purpose": "milestone gate outside the daily optimization loop"},
            "canary": {"purpose": "fresh incoming regressions and drift detection"},
        },
        "artifacts": {
            "curator_notes": "governance/curator-notes.md",
            "proposal_log": "governance/proposals.jsonl",
            "latest_summary_json": "summaries/latest-summary.json",
            "latest_summary_md": "summaries/latest-summary.md",
        },
        "cases": [],
    }


def ensure_benchmark_root(benchmark_root: Path, benchmark_name: str | None) -> dict:
    benchmark_root.mkdir(parents=True, exist_ok=True)
    manifest_path = benchmark_root / "manifest.json"
    if manifest_path.exists():
        manifest = read_json(manifest_path)
        if manifest.get("schema_version") != 1:
            raise ValueError("仅支持 schema_version=1 的 benchmark manifest。")
        return manifest

    actual_name = benchmark_name or benchmark_root.name
    manifest = default_manifest(benchmark_root, actual_name)
    write_json(manifest_path, manifest)
    return manifest


def ensure_split_dirs(benchmark_root: Path) -> None:
    for split in SPLITS:
        (benchmark_root / "cases" / split).mkdir(parents=True, exist_ok=True)


def ensure_suite_dirs(benchmark_root: Path) -> None:
    ensure_split_dirs(benchmark_root)
    for relative_dir in (
        Path("candidates"),
        Path("governance"),
        Path("summaries"),
        Path("runs"),
    ):
        (benchmark_root / relative_dir).mkdir(parents=True, exist_ok=True)


def case_relative_dir(split: str, case_id: str) -> Path:
    return Path("cases") / split / case_id


def find_case_record(manifest: dict, case_id: str) -> dict | None:
    for item in manifest.get("cases", []):
        if item.get("case_id") == case_id:
            return item
    return None


def ensure_case_id_available(manifest: dict, case_id: str) -> None:
    if find_case_record(manifest, case_id) is not None:
        raise ValueError(f"case 已存在：{case_id}")


def resolve_case_dir(benchmark_root: Path, manifest: dict, case_id: str, split: str | None) -> Path:
    record = find_case_record(manifest, case_id)
    if record is None:
        raise ValueError(f"找不到 case：{case_id}")
    if split is not None and record.get("split") != split:
        raise ValueError(f"case split 不匹配：{case_id} 期望 {split} 实际 {record.get('split')}")
    relative_dir = record.get("relative_dir")
    if not relative_dir:
        raise ValueError(f"case 缺少 relative_dir：{case_id}")
    return (benchmark_root / Path(relative_dir)).resolve()


def load_case_json(case_dir: Path) -> dict:
    case_json_path = case_dir / "case.json"
    if not case_json_path.exists():
        raise ValueError(f"缺少 case.json：{case_dir}")
    return read_json(case_json_path)


def default_evaluation(case_id: str, split: str) -> dict:
    return {
        "schema_version": 1,
        "case_id": case_id,
        "split": split,
        "run_label": "",
        "skill_revision": "",
        "status": "not_run",
        "summary": "",
        "verdict": "unknown",
        "blocker_classification": "",
        "metrics": {},
        "evidence_paths": [],
        "evaluated_at_utc": "",
    }
