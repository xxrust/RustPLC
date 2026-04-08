#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path

from benchmark_common import ensure_benchmark_root, read_json, utc_now, write_json, write_text


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="聚合 benchmark case 的 evaluation/result.json，生成 suite 级 summary。"
    )
    parser.add_argument("--benchmark-root", required=True, help="benchmark 根目录。")
    return parser.parse_args()


def empty_totals() -> dict[str, int]:
    return {
        "cases": 0,
        "pass": 0,
        "fail": 0,
        "blocked": 0,
        "error": 0,
        "not_run": 0,
    }


def render_summary_md(summary: dict) -> str:
    totals = summary["totals"]
    lines = [
        "# Benchmark Summary",
        "",
        f"Generated At: {summary['generated_at_utc']}",
        "",
        "## Totals",
        "",
        f"- Cases: {totals['cases']}",
        f"- Pass: {totals['pass']}",
        f"- Fail: {totals['fail']}",
        f"- Blocked: {totals['blocked']}",
        f"- Error: {totals['error']}",
        f"- Not Run: {totals['not_run']}",
        "",
        "## By Split",
        "",
    ]

    if summary["by_split"]:
        for split, split_totals in summary["by_split"].items():
            lines.append(
                f"- {split}: cases={split_totals['cases']} pass={split_totals['pass']} fail={split_totals['fail']} blocked={split_totals['blocked']} error={split_totals['error']} not_run={split_totals['not_run']}"
            )
    else:
        lines.append("- [no cases]")

    lines.extend(["", "## Top Blockers", ""])
    if summary["top_blockers"]:
        for item in summary["top_blockers"]:
            lines.append(f"- {item['blocker_classification']}: {item['count']}")
    else:
        lines.append("- [none]")

    lines.extend(["", "## Stable Failure Patterns", ""])
    if summary["stable_failure_patterns"]:
        for item in summary["stable_failure_patterns"]:
            lines.append(f"- {item}")
    else:
        lines.append("- [none]")

    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    benchmark_root = Path(args.benchmark_root).resolve()
    manifest = ensure_benchmark_root(benchmark_root, None)

    totals = empty_totals()
    by_split: dict[str, dict[str, int]] = {}
    blocker_counter: Counter[str] = Counter()
    stable_failure_patterns: list[str] = []

    for case_record in manifest.get("cases", []):
        split = str(case_record["split"])
        split_totals = by_split.setdefault(split, empty_totals())
        totals["cases"] += 1
        split_totals["cases"] += 1

        case_dir = benchmark_root / Path(case_record["relative_dir"])
        result_path = case_dir / "evaluation" / "result.json"
        result = read_json(result_path) if result_path.exists() else {"status": "not_run", "verdict": "unknown"}

        status = str(result.get("status", "not_run"))
        verdict = str(result.get("verdict", "unknown"))
        blocker = str(result.get("blocker_classification", "") or "")

        if status == "not_run" or verdict == "unknown":
            totals["not_run"] += 1
            split_totals["not_run"] += 1
        elif verdict == "pass":
            totals["pass"] += 1
            split_totals["pass"] += 1
        elif verdict == "fail":
            totals["fail"] += 1
            split_totals["fail"] += 1
        elif verdict == "blocked":
            totals["blocked"] += 1
            split_totals["blocked"] += 1
        elif verdict == "error" or status == "error":
            totals["error"] += 1
            split_totals["error"] += 1

        if blocker:
            blocker_counter[blocker] += 1
        if verdict in {"fail", "blocked", "error"}:
            stable_failure_patterns.append(f"{case_record['case_id']}::{verdict}")

    summary = {
        "schema_version": 1,
        "benchmark_name": manifest["benchmark_name"],
        "generated_at_utc": utc_now(),
        "totals": totals,
        "by_split": by_split,
        "top_blockers": [
            {"blocker_classification": blocker, "count": count}
            for blocker, count in blocker_counter.most_common(5)
        ],
        "stable_failure_patterns": stable_failure_patterns,
    }

    write_json(benchmark_root / "summaries" / "latest-summary.json", summary)
    write_text(benchmark_root / "summaries" / "latest-summary.md", render_summary_md(summary))
    print(f"[OK] aggregated benchmark results: {benchmark_root / 'summaries' / 'latest-summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
