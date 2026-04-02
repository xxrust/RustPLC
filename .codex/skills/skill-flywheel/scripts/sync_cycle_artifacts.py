#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="根据结构化 JSON 日志重建 cycle 的 Markdown closeout 工件。"
    )
    parser.add_argument("--cycle-dir", required=True, help="cycle 根目录。")
    parser.add_argument(
        "--require-non-placeholder-decision",
        action="store_true",
        help="若 decision.json 仍是占位内容，则返回非零退出码。",
    )
    parser.add_argument(
        "--sync-experiments",
        action="store_true",
        help="若 cycle 已有非占位 decision，则同步 experiments.jsonl。",
    )
    return parser.parse_args()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def write_text(path: Path, content: str) -> None:
    path.write_text(content if content.endswith("\n") else f"{content}\n", encoding="utf-8")


def is_placeholder_decision(payload: dict) -> bool:
    return (
        payload.get("hypothesis_status", "unknown") == "unknown"
        and not payload.get("key_evidence")
        and not payload.get("minimal_actions")
        and not payload.get("next_question")
    )


def render_pain_points_md(payload: dict) -> str:
    task = payload.get("task", "")
    result_summary = payload.get("result_summary", "") or "[未填写结果总结。]"
    signal = payload.get("hypothesis_signal", "unknown")
    items = payload.get("pain_points") or []
    lines = [
        "# 痛点记录",
        "",
        "任务：",
        task,
        "",
        "## 结果",
        "",
        result_summary,
        "",
        "## 假设观察",
        "",
        signal,
        "",
        "## 痛点",
        "",
    ]
    if not items:
        lines.extend(["1. 步骤：", "   观察到的阻塞：", "   缺少的工件或说明：", "   影响："])
    else:
        for idx, item in enumerate(items, start=1):
            lines.extend(
                [
                    f"{idx}. 步骤：",
                    f"   {item.get('step', '')}",
                    "   观察到的阻塞：",
                    f"   {item.get('blocker', '')}",
                    "   缺少的工件或说明：",
                    f"   {item.get('missing_item', '')}",
                    "   影响：",
                    f"   {item.get('impact', '')}",
                ]
            )
    return "\n".join(lines) + "\n"


def render_root_cause_md(payload: dict) -> str:
    task = payload.get("task", "")
    status = payload.get("hypothesis_status", "unknown")
    findings = payload.get("findings") or []
    lines = ["# 根因分析", "", "任务：", task, "", "## 假设判断", "", status, "", "## 结论", ""]
    if not findings:
        lines.extend(["1. 痛点：", "   分类：", "   原因：", "   最小修复："])
    else:
        for idx, finding in enumerate(findings, start=1):
            lines.extend(
                [
                    f"{idx}. 痛点：",
                    f"   {finding.get('pain_point', '')}",
                    "   分类：",
                    f"   {finding.get('classification', '')}",
                    "   原因：",
                    f"   {finding.get('cause', '')}",
                    "   最小修复：",
                    f"   {finding.get('minimal_fix', '')}",
                ]
            )
    return "\n".join(lines) + "\n"


def render_decision_md(payload: dict) -> str:
    research_question = payload.get("research_question", "")
    status = payload.get("hypothesis_status", "unknown")
    evidence = payload.get("key_evidence") or []
    actions = payload.get("minimal_actions") or []
    continue_next = "是" if payload.get("continue_next_cycle") else "否"
    classification = payload.get("classification", "")
    decision_summary = payload.get("decision_summary", "")
    next_question = payload.get("next_question", "")
    lines = ["# 本轮决策", ""]
    if research_question:
        lines.extend(["## 研究问题", "", research_question, ""])
    lines.extend(["## 假设状态", "", status, "", "## 关键证据", ""])
    if evidence:
        lines.extend([f"- {item}" for item in evidence])
    else:
        lines.append("- [未填写关键证据。]")
    lines.extend(["", "## 本轮最小动作", ""])
    if actions:
        lines.extend([f"- {item}" for item in actions])
    else:
        lines.append("- [未填写最小动作。]")
    if classification:
        lines.extend(["", "## 结论分类", "", classification])
    if decision_summary:
        lines.extend(["", "## 决策摘要", "", decision_summary])
    lines.extend(
        [
            "",
            "## 是否进入下一轮",
            "",
            continue_next,
            "",
            "## 下一轮研究问题",
            "",
            next_question or "[未填写下一轮问题。]",
        ]
    )
    return "\n".join(lines) + "\n"


def sync_experiments_log(cycle_dir: Path, decision: dict) -> None:
    config_dir = cycle_dir.parent.parent
    experiments_path = config_dir / "experiments.jsonl"
    if is_placeholder_decision(decision):
        return

    question = decision.get("research_question", "") or ""
    record = {
        "cycle": cycle_dir.name,
        "question": question,
        "decision": "continue" if decision.get("continue_next_cycle") else "stop",
        "reason": decision.get("decision_summary", "") or decision.get("next_question", "") or "",
        "classification": decision.get("classification", "") or "",
    }

    existing: list[dict] = []
    if experiments_path.exists():
        for line in experiments_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            existing.append(json.loads(line))

    updated = False
    for idx, item in enumerate(existing):
        if item.get("cycle") == cycle_dir.name:
            existing[idx] = record
            updated = True
            break
    if not updated:
        existing.append(record)

    write_text(
        experiments_path,
        "\n".join(json.dumps(item, ensure_ascii=False) for item in existing) + "\n",
    )


def sync_cycle_artifacts(
    cycle_dir: Path,
    require_non_placeholder_decision: bool = False,
    sync_experiments: bool = False,
) -> int:
    cycle_dir = cycle_dir.resolve()
    logs_dir = cycle_dir / "logs"

    pain_points = read_json(logs_dir / "pain-points.json")
    root_cause = read_json(logs_dir / "root-cause.json")
    decision = read_json(logs_dir / "decision.json")

    if require_non_placeholder_decision and is_placeholder_decision(decision):
        print(f"[ERROR] decision.json 仍是 placeholder: {logs_dir / 'decision.json'}")
        return 1

    write_text(logs_dir / "pain-points.md", render_pain_points_md(pain_points))
    write_text(logs_dir / "root-cause.md", render_root_cause_md(root_cause))
    write_text(logs_dir / "decision.md", render_decision_md(decision))
    if sync_experiments:
        sync_experiments_log(cycle_dir, decision)
    print(f"[OK] synced cycle artifacts: {cycle_dir}")
    return 0


def main() -> int:
    args = parse_args()
    return sync_cycle_artifacts(
        Path(args.cycle_dir),
        require_non_placeholder_decision=args.require_non_placeholder_decision,
        sync_experiments=args.sync_experiments,
    )


if __name__ == "__main__":
    raise SystemExit(main())
