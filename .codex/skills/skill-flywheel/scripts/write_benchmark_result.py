#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path

from benchmark_common import (
    RESULT_VERDICTS,
    RUN_STATUSES,
    ensure_benchmark_root,
    load_case_json,
    read_json,
    resolve_case_dir,
    utc_now,
    write_json,
)


def parse_metric(raw: str) -> tuple[str, object]:
    if "=" not in raw:
        raise ValueError(f"metric 必须是 key=value 形式：{raw}")
    key, value = raw.split("=", 1)
    key = key.strip()
    if not key:
        raise ValueError(f"metric key 不能为空：{raw}")
    value = value.strip()
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        parsed = value
    return key, parsed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="给 benchmark case 写入或更新标准 evaluation/result.json。"
    )
    parser.add_argument("--benchmark-root", required=True, help="benchmark 根目录。")
    parser.add_argument("--case-id", required=True, help="case id。")
    parser.add_argument("--split", choices=("dev", "holdout", "canary"), help="可选；用于额外校验 case split。")
    parser.add_argument("--run-label", default="", help="本次评测运行标签。")
    parser.add_argument("--skill-revision", default="", help="被测 skill 版本、commit 或 revision。")
    parser.add_argument("--status", default="completed", choices=RUN_STATUSES, help="运行状态。")
    parser.add_argument("--verdict", default="unknown", choices=RESULT_VERDICTS, help="评测结论。")
    parser.add_argument("--summary", default="", help="简短结果摘要。")
    parser.add_argument("--blocker-classification", default="", help="若为 blocked/fail，可记录 blocker 分类。")
    parser.add_argument("--metric", action="append", default=[], help="结构化 metric，格式 key=value，可重复。")
    parser.add_argument("--evidence-path", action="append", default=[], help="相关证据路径，可重复。")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    benchmark_root = Path(args.benchmark_root).resolve()
    manifest = ensure_benchmark_root(benchmark_root, None)
    case_dir = resolve_case_dir(benchmark_root, manifest, args.case_id, args.split)
    case_payload = load_case_json(case_dir)
    result_path = case_dir / "evaluation" / "result.json"
    current = read_json(result_path) if result_path.exists() else {}

    metrics: dict[str, object] = dict(current.get("metrics", {}))
    for raw_metric in args.metric:
        key, value = parse_metric(raw_metric)
        metrics[key] = value

    evidence_paths = [str(Path(item)) for item in args.evidence_path]
    payload = {
        "schema_version": 1,
        "case_id": case_payload["case_id"],
        "split": case_payload["split"],
        "run_label": args.run_label,
        "skill_revision": args.skill_revision,
        "status": args.status,
        "summary": args.summary,
        "verdict": args.verdict,
        "blocker_classification": args.blocker_classification,
        "metrics": metrics,
        "evidence_paths": evidence_paths,
        "evaluated_at_utc": utc_now(),
    }
    write_json(result_path, payload)
    print(f"[OK] wrote benchmark result: {result_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
