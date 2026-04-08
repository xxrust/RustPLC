#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from benchmark_common import ensure_benchmark_root, ensure_suite_dirs, write_json, write_text


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="初始化一个通用 benchmark suite 根目录，不绑定任何具体项目 case。"
    )
    parser.add_argument("--benchmark-root", required=True, help="benchmark 根目录。")
    parser.add_argument("--benchmark-name", help="benchmark 名称；默认取目录名。")
    return parser.parse_args()


def render_curator_notes() -> str:
    return """# Curator Notes

记录 benchmark curator 的长期决策。

至少写清：

- 哪些 case 被 freeze / retire
- 变更原因
- 这些变更不应由当前 active optimizer 决定的原因
"""


def render_suite_readme(benchmark_root: Path) -> str:
    return f"""# Benchmark Suite

Root: `{benchmark_root.resolve()}`

目录约定：

- `cases/`：冻结后的 benchmark case
- `candidates/`：proposer 整理的候选材料
- `governance/`：curator 决策、proposal log
- `summaries/`：聚合结果
- `runs/`：可选的 judge 运行级输出

长期规则：

- active flywheel optimizer 不得在同一轮内改写 frozen case
- hidden rubric / oracle 只供 curator / judge 使用
"""


def render_initial_summary_json() -> dict:
    return {
        "schema_version": 1,
        "benchmark_name": "",
        "generated_at_utc": "",
        "totals": {
            "cases": 0,
            "pass": 0,
            "fail": 0,
            "blocked": 0,
            "error": 0,
            "not_run": 0,
        },
        "by_split": {},
        "top_blockers": [],
        "stable_failure_patterns": [],
    }


def render_initial_summary_md() -> str:
    return """# Benchmark Summary

尚未生成聚合结果。
"""


def main() -> int:
    args = parse_args()
    benchmark_root = Path(args.benchmark_root).resolve()
    manifest = ensure_benchmark_root(benchmark_root, args.benchmark_name)
    ensure_suite_dirs(benchmark_root)

    write_text(benchmark_root / "README.md", render_suite_readme(benchmark_root))
    write_text(benchmark_root / "governance" / "curator-notes.md", render_curator_notes())
    write_text(benchmark_root / "governance" / "proposals.jsonl", "")

    summary_json = render_initial_summary_json()
    summary_json["benchmark_name"] = manifest["benchmark_name"]
    write_json(benchmark_root / "summaries" / "latest-summary.json", summary_json)
    write_text(benchmark_root / "summaries" / "latest-summary.md", render_initial_summary_md())

    print(f"[OK] initialized benchmark suite: {benchmark_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
