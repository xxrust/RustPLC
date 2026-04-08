#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from benchmark_common import (
    SPLITS,
    STATUSES,
    default_evaluation,
    ensure_benchmark_root,
    ensure_case_id_available,
    ensure_suite_dirs,
    utc_now,
    write_json,
    write_text,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="为 skill-flywheel 初始化一个与具体项目解耦的 benchmark case 脚手架。"
    )
    parser.add_argument("--benchmark-root", required=True, help="benchmark 根目录。")
    parser.add_argument("--benchmark-name", help="benchmark 名称；首次初始化根目录时可选。")
    parser.add_argument("--case-id", required=True, help="case id，例如 case-001。")
    parser.add_argument("--split", default="dev", choices=SPLITS, help="case 所属 split。默认 dev。")
    parser.add_argument("--status", default="draft", choices=STATUSES, help="case 状态。默认 draft。")
    parser.add_argument("--title", default="", help="case 标题。")
    parser.add_argument("--skill-family", default="", help="目标 skill 家族名，例如 generic / coding / planning。")
    parser.add_argument("--case-type", default="generic", help="case 类型，例如 generic / blind-runner / e2e。")
    parser.add_argument("--question", default="", help="本 case 主要验证的问题。")
    return parser.parse_args()


def case_relative_dir(split: str, case_id: str) -> Path:
    return Path("cases") / split / case_id


def build_case_payload(args: argparse.Namespace, case_dir: Path) -> dict:
    return {
        "schema_version": 1,
        "case_id": args.case_id,
        "title": args.title,
        "split": args.split,
        "status": args.status,
        "skill_family": args.skill_family,
        "case_type": args.case_type,
        "question": args.question,
        "created_at_utc": utc_now(),
        "paths": {
            "public_dir": str((case_dir / "public").resolve()),
            "hidden_dir": str((case_dir / "hidden").resolve()),
            "evaluation_dir": str((case_dir / "evaluation").resolve()),
        },
    }


def render_public_prompt(case_id: str, question: str) -> str:
    return f"""# Benchmark Prompt

Case ID: `{case_id}`

## Task

{question or '[在这里写给被测 skill 的公开任务。]'}

## Allowed Inputs

- 仅允许读取本 case `public/` 下的显式输入
- 不允许读取 `hidden/` 下的 rubric / oracle
"""


def render_public_inputs_readme() -> str:
    return """# Public Inputs

把可公开给被测 skill 的输入放在这里。

例如：

- task prompt
- 明确允许的上下文工件
- 可公开的输入样本
"""


def render_hidden_notes() -> str:
    return """# Hidden Notes

这里记录仅供 curator / judge 使用的内部说明。

不要把这些内容暴露给 active flywheel optimizer 或被测 skill。
"""


def default_rubric() -> dict:
    return {
        "schema_version": 1,
        "rubric_type": "generic",
        "criteria": [
            {
                "name": "task_completion",
                "description": "是否完成公开任务，或在遇到真实 blocker 时诚实给出 blocker。",
                "weight": 1.0,
            }
        ],
        "pass_conditions": [],
        "blocker_conditions": [],
        "notes": "",
    }


def default_oracle() -> dict:
    return {
        "schema_version": 1,
        "oracle_type": "textual",
        "hidden_to_flywheel": True,
        "expected_signals": [],
        "executable_command": "",
        "structured_expectations": {},
        "blocker_truth_policy": "when a real blocker exists, truthful blocker reporting counts as success",
    }


def register_case(manifest: dict, args: argparse.Namespace, case_rel_dir: Path) -> dict:
    existing = manifest.get("cases", [])
    record = {
        "case_id": args.case_id,
        "title": args.title,
        "split": args.split,
        "status": args.status,
        "skill_family": args.skill_family,
        "case_type": args.case_type,
        "question": args.question,
        "relative_dir": str(case_rel_dir).replace("\\", "/"),
    }
    existing.append(record)
    manifest["cases"] = existing
    return manifest


def main() -> int:
    args = parse_args()
    benchmark_root = Path(args.benchmark_root).resolve()

    manifest = ensure_benchmark_root(benchmark_root, args.benchmark_name)
    ensure_suite_dirs(benchmark_root)
    ensure_case_id_available(manifest, args.case_id)

    case_rel_dir = case_relative_dir(args.split, args.case_id)
    case_dir = benchmark_root / case_rel_dir
    if case_dir.exists():
        raise ValueError(f"case 目录已存在：{case_dir}")

    (case_dir / "public" / "inputs").mkdir(parents=True, exist_ok=True)
    (case_dir / "hidden").mkdir(parents=True, exist_ok=True)
    (case_dir / "evaluation").mkdir(parents=True, exist_ok=True)

    write_json(case_dir / "case.json", build_case_payload(args, case_dir))
    write_text(case_dir / "public" / "prompt.md", render_public_prompt(args.case_id, args.question))
    write_text(case_dir / "public" / "inputs" / "README.md", render_public_inputs_readme())
    write_json(case_dir / "hidden" / "rubric.json", default_rubric())
    write_json(case_dir / "hidden" / "oracle.json", default_oracle())
    write_text(case_dir / "hidden" / "notes.md", render_hidden_notes())
    write_json(case_dir / "evaluation" / "result.json", default_evaluation(args.case_id, args.split))

    manifest = register_case(manifest, args, case_rel_dir)
    write_json(benchmark_root / "manifest.json", manifest)

    print(f"[OK] initialized benchmark case: {case_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
