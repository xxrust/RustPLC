#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

SKILL_ROOT = Path(__file__).resolve().parent.parent
AGENTS_DIR = SKILL_ROOT / "agents"

DEFAULT_BLOCKED_SEGMENTS = {
    ".git",
    "src",
    "crates",
    "target",
    "vendor",
    "web-ui",
}


@dataclass(frozen=True)
class CyclePaths:
    root: Path
    public: Path
    logs: Path
    prompts: Path
    context: Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="为 skill-flywheel 的一次研究回合创建辅助工件包、上下文和提示词包。"
    )
    parser.add_argument("--repo-root", required=True, help="受保护仓库的根目录。")
    parser.add_argument(
        "--cycle-dir",
        help="本轮 cycle 的输出目录。默认使用 <target-skill-path>/.skill_flywheel/cycles/<timestamp>。",
    )
    parser.add_argument("--target-skill-path", required=True, help="要进化或验证的目标 skill 目录。")
    parser.add_argument("--task", help="给禁止读源码执行者的真实任务。")
    parser.add_argument(
        "--task-file",
        help="任务模板文件。可传绝对路径，或传 <target-skill-path>/.skill_flywheel/tasks/ 下的相对文件名。",
    )
    parser.add_argument(
        "--project-name",
        help="便于阅读的项目名。默认使用仓库目录名。",
    )
    parser.add_argument(
        "--config-dir",
        help="目标 skill 的 .skill_flywheel 配置目录。默认尝试 <target-skill-path>/.skill_flywheel。",
    )
    parser.add_argument(
        "--include",
        action="append",
        default=[],
        help="额外导出的辅助工件相对路径。相对于 <config-dir>/public/。可重复传入。",
    )
    parser.add_argument(
        "--exclude-segment",
        action="append",
        default=[],
        help="复制 `.skill_flywheel/public/` 下目录时额外屏蔽的路径片段。可重复传入。",
    )
    args = parser.parse_args()
    if bool(args.task) == bool(args.task_file):
        parser.error("必须且只能提供一个：--task 或 --task-file。")
    return args


def ensure_dirs(cycle_dir: Path) -> CyclePaths:
    public_dir = cycle_dir / "public"
    logs_dir = cycle_dir / "logs"
    prompts_dir = cycle_dir / "prompts"
    context_dir = cycle_dir / "context"
    runs_dir = logs_dir / "runs"
    public_dir.mkdir(parents=True, exist_ok=True)
    logs_dir.mkdir(parents=True, exist_ok=True)
    prompts_dir.mkdir(parents=True, exist_ok=True)
    context_dir.mkdir(parents=True, exist_ok=True)
    runs_dir.mkdir(parents=True, exist_ok=True)
    return CyclePaths(root=cycle_dir, public=public_dir, logs=logs_dir, prompts=prompts_dir, context=context_dir)


def is_blocked(path: Path, blocked_segments: set[str]) -> bool:
    return any(part in blocked_segments for part in path.parts)


def copy_public_artifact(artifact_root: Path, public_root: Path, relative_path: str, blocked_segments: set[str]) -> dict:
    source = (artifact_root / relative_path).resolve()
    if not source.exists():
        return {"path": relative_path, "status": "missing"}
    try:
        source.relative_to(artifact_root)
    except ValueError as exc:
        raise ValueError(f"路径越出了 .skill_flywheel/public 根目录：{relative_path}") from exc
    if is_blocked(source.relative_to(artifact_root), blocked_segments):
        return {"path": relative_path, "status": "blocked"}

    destination = public_root / source.relative_to(artifact_root)
    if source.is_dir():
        copied_files = 0
        for item in source.rglob("*"):
            if not item.is_file():
                continue
            rel = item.relative_to(artifact_root)
            if is_blocked(rel, blocked_segments):
                continue
            out_file = public_root / rel
            out_file.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(item, out_file)
            copied_files += 1
        return {"path": relative_path, "status": "copied-dir", "files": copied_files}

    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return {"path": relative_path, "status": "copied-file", "files": 1}


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def normalize_path_string(path: Path) -> str:
    return str(path.resolve())


def render_agent_template(template_name: str, values: dict[str, str]) -> str:
    template_path = AGENTS_DIR / template_name
    template = template_path.read_text(encoding="utf-8")
    for key, value in values.items():
        template = template.replace(f"<{key}>", value)
    return template


def default_cycle_dir(target_skill_path: Path, explicit_cycle_dir: str | None) -> Path:
    if explicit_cycle_dir:
        return Path(explicit_cycle_dir).resolve()
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    return (target_skill_path / ".skill_flywheel" / "cycles" / f"cycle-{timestamp}").resolve()


def resolve_config_dir(target_skill_path: Path, explicit_config_dir: str | None) -> Path | None:
    if explicit_config_dir:
        config_dir = Path(explicit_config_dir).resolve()
        return config_dir if config_dir.exists() else None
    candidate = target_skill_path / ".skill_flywheel"
    return candidate if candidate.exists() else None


def load_local_config(config_dir: Path | None) -> dict:
    if config_dir is None:
        return {"artifact_paths": [], "exclude_segments": [], "parallel_runs": 1, "run_id_prefix": "run"}

    config_path = config_dir / "public_surface.json"
    if not config_path.exists():
        return {"artifact_paths": [], "exclude_segments": [], "parallel_runs": 1, "run_id_prefix": "run"}

    data = json.loads(config_path.read_text(encoding="utf-8"))
    if "include_paths" in data:
        raise ValueError(
            "public_surface.json 不再支持 include_paths；请把要导出的辅助工件先放到 .skill_flywheel/public/，再用 artifact_paths 列出。"
        )
    artifact_paths = data.get("artifact_paths", [])
    exclude_segments = data.get("exclude_segments", [])
    parallel_runs = data.get("parallel_runs", 1)
    run_id_prefix = data.get("run_id_prefix", "run")
    if not isinstance(artifact_paths, list) or not all(isinstance(x, str) for x in artifact_paths):
        raise ValueError("public_surface.json 中的 artifact_paths 必须是字符串数组。")
    if not isinstance(exclude_segments, list) or not all(isinstance(x, str) for x in exclude_segments):
        raise ValueError("public_surface.json 中的 exclude_segments 必须是字符串数组。")
    if not isinstance(parallel_runs, int) or parallel_runs < 1:
        raise ValueError("public_surface.json 中的 parallel_runs 必须是大于等于 1 的整数。")
    if not isinstance(run_id_prefix, str) or not run_id_prefix.strip():
        raise ValueError("public_surface.json 中的 run_id_prefix 必须是非空字符串。")
    return {
        "artifact_paths": artifact_paths,
        "exclude_segments": exclude_segments,
        "parallel_runs": parallel_runs,
        "run_id_prefix": run_id_prefix,
    }


def load_optional_text(config_dir: Path | None, filename: str) -> tuple[Path | None, str | None]:
    if config_dir is None:
        return None, None

    file_path = config_dir / filename
    if not file_path.exists():
        return None, None

    return file_path, file_path.read_text(encoding="utf-8")


def resolve_task_source(task: str | None, task_file: str | None, config_dir: Path | None) -> tuple[str, Path | None]:
    if task is not None:
        return task, None

    assert task_file is not None
    requested = Path(task_file)
    candidates: list[Path] = []
    if requested.is_absolute():
        candidates.append(requested)
    else:
        if config_dir is not None:
            candidates.append(config_dir / "tasks" / task_file)
            candidates.append(config_dir / task_file)
        candidates.append(Path.cwd() / task_file)

    for candidate in candidates:
        if candidate.exists():
            resolved = candidate.resolve()
            return resolved.read_text(encoding="utf-8"), resolved

    raise FileNotFoundError(f"找不到任务模板：{task_file}")


def render_boundary_readme(project_name: str, cycle: CyclePaths, repo_root: Path, target_skill_path: Path, manifest_path: Path) -> str:
    return f"""# 公开边界

项目：{project_name}
仓库根目录：{normalize_path_string(repo_root)}
目标 skill：{normalize_path_string(target_skill_path)}
Cycle 目录：{normalize_path_string(cycle.root)}
Manifest：{normalize_path_string(manifest_path)}

允许读取：
- 真实目标 skill：`{normalize_path_string(target_skill_path)}`
- `public/` 目录下显式导出的辅助工件
- `context/` 目录下的研究程序与任务说明

禁止读取：
- 目标 skill 之外的仓库文件
- 仓库里的 `README`、`docs/`、`examples/`、`src/`、`crates/` 等普通文件或受保护目录

如果缺少必要信息，请把它记成痛点，而不是越界读取源码。
"""


def render_program_template() -> str:
    return """# 本轮研究程序

## 研究问题

[写明这轮要验证的 skill 能力缺口。]

## 当前假设

[写明这轮最想验证的解释。]

## 成功信号

- [什么观察会支持当前假设]

## 失败信号

- [什么观察会削弱当前假设]

## 决策规则

- 如果属于 `skill-gap`：
- 如果属于 `public-surface-gap`：
- 如果属于 `code-gap`：
- 如果属于 `task-ambiguity`：

## 停止条件

- [什么时候本轮结束]
"""


def render_pain_points_template(task: str) -> str:
    return f"""# 痛点记录

任务：
{task}

## 结果

[总结禁止读源码执行者实际完成了什么。]

## 假设观察

[本轮观察更支持、削弱，还是无法判断当前假设。]

## 痛点

1. 步骤：
   观察到的阻塞：
   缺少的工件或说明：
   影响：

2. 步骤：
   观察到的阻塞：
   缺少的工件或说明：
   影响：
"""


def render_root_cause_template(task: str) -> str:
    return f"""# 根因分析

任务：
{task}

## 假设判断

[支持 / 削弱 / 证据不足]

## 结论

1. 痛点：
   分类：
   原因：
   最小修复：

2. 痛点：
   分类：
   原因：
   最小修复：
"""


def render_decision_template() -> str:
    return """# 本轮决策

## 假设状态

[支持 / 削弱 / 证据不足]

## 关键证据

- 

## 本轮最小动作

- 

## 是否进入下一轮

[是 / 否]

## 下一轮研究问题

[如果继续，写更窄的问题；否则写停止原因。]
"""


def render_synthesis_template() -> str:
    return """# 实例聚合

## 总体信号

[支持 / 削弱 / 证据不足]

## 共性问题

- 

## 实例特有问题

- 

## 冲突与解释

- 
"""


def render_pain_points_json_template(task: str) -> str:
    return json.dumps(
        {
            "task": task,
            "hypothesis_signal": "unknown",
            "result_summary": "",
            "pain_points": [
                {
                    "step": "",
                    "blocker": "",
                    "missing_item": "",
                    "impact": "",
                }
            ],
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def render_root_cause_json_template(task: str) -> str:
    return json.dumps(
        {
            "task": task,
            "hypothesis_status": "unknown",
            "findings": [
                {
                    "pain_point": "",
                    "classification": "",
                    "cause": "",
                    "minimal_fix": "",
                }
            ],
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def render_decision_json_template() -> str:
    return json.dumps(
        {
            "research_question": "",
            "hypothesis_status": "unknown",
            "key_evidence": [],
            "minimal_actions": [],
            "continue_next_cycle": False,
            "classification": "",
            "decision_summary": "",
            "next_question": "",
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def render_run_index_json_template() -> str:
    return json.dumps(
        {
            "runs": [],
            "synthesis_ready": False,
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def render_synthesis_json_template() -> str:
    return json.dumps(
        {
            "common_findings": [],
            "run_specific_findings": [],
            "conflicts": [],
            "overall_signal": "unknown",
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def render_run_notes_template(run_id: str, task: str) -> str:
    return f"""# Blind Runner {run_id}

任务：
{task}

## 结果

[记录 {run_id} 的独立执行结果。]

## 假设观察

[支持 / 削弱 / 无法判断]

## 痛点

1. 步骤：
   观察到的阻塞：
   缺少的工件或说明：
   影响：
"""


def render_run_json_template(run_id: str, task: str) -> str:
    return json.dumps(
        {
            "run_id": run_id,
            "task": task,
            "hypothesis_signal": "unknown",
            "result_summary": "",
            "pain_points": [],
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n"


def build_run_specs(cycle: CyclePaths, parallel_runs: int, run_id_prefix: str) -> list[dict[str, str]]:
    width = max(2, len(str(parallel_runs)))
    specs: list[dict[str, str]] = []
    for idx in range(1, parallel_runs + 1):
        run_id = f"{run_id_prefix}-{idx:0{width}d}"
        specs.append(
            {
                "run_id": run_id,
                "notes_path": normalize_path_string(cycle.logs / "runs" / f"{run_id}.md"),
                "json_path": normalize_path_string(cycle.logs / "runs" / f"{run_id}.json"),
                "prompt_path": normalize_path_string(cycle.prompts / "runs" / f"{run_id}-agent2.md"),
            }
        )
    return specs


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    target_skill_path = Path(args.target_skill_path).resolve()
    project_name = args.project_name or repo_root.name
    config_dir = resolve_config_dir(target_skill_path, args.config_dir)
    local_config = load_local_config(config_dir)
    cycle_dir = default_cycle_dir(target_skill_path, args.cycle_dir)
    task_text, task_source_path = resolve_task_source(args.task, args.task_file, config_dir)
    program_source_path, program_text = load_optional_text(config_dir, "program.md")
    profile_source_path, profile_text = load_optional_text(config_dir, "profile.md")
    artifact_root = config_dir / "public" if config_dir is not None else None

    cycle = ensure_dirs(cycle_dir)
    blocked_segments = set(DEFAULT_BLOCKED_SEGMENTS)
    blocked_segments.update(local_config["exclude_segments"])
    blocked_segments.update(args.exclude_segment)

    artifact_paths = list(local_config["artifact_paths"])
    artifact_paths.extend(args.include)
    if artifact_paths and artifact_root is None:
        raise ValueError("提供了辅助工件路径，但目标 skill 下不存在 .skill_flywheel/public/。")

    copy_results = []
    if artifact_root is not None:
        copy_results = [
            copy_public_artifact(artifact_root, cycle.public, rel_path, blocked_segments)
            for rel_path in artifact_paths
        ]

    manifest_path = cycle.root / "manifest.json"
    manifest = {
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "project_name": project_name,
        "repo_root": normalize_path_string(repo_root),
        "target_skill_path": normalize_path_string(target_skill_path),
        "config_dir": normalize_path_string(config_dir) if config_dir else None,
        "artifact_source_dir": normalize_path_string(artifact_root) if artifact_root else None,
        "cycle_dir": normalize_path_string(cycle.root),
        "task": task_text,
        "task_source_path": normalize_path_string(task_source_path) if task_source_path else None,
        "program_source_path": normalize_path_string(program_source_path) if program_source_path else None,
        "profile_source_path": normalize_path_string(profile_source_path) if profile_source_path else None,
        "blocked_segments": sorted(blocked_segments),
        "copied_artifacts": copy_results,
        "parallel_runs": local_config["parallel_runs"],
        "run_id_prefix": local_config["run_id_prefix"],
        "structured_logs": {
            "pain_points_json": normalize_path_string(cycle.logs / "pain-points.json"),
            "root_cause_json": normalize_path_string(cycle.logs / "root-cause.json"),
            "decision_json": normalize_path_string(cycle.logs / "decision.json"),
            "run_index_json": normalize_path_string(cycle.logs / "run-index.json"),
            "synthesis_json": normalize_path_string(cycle.logs / "synthesis.json"),
        },
    }
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")

    if program_text is None:
        write_text(cycle.context / "program.md", render_program_template())
    else:
        write_text(cycle.context / "program.md", program_text if program_text.endswith("\n") else f"{program_text}\n")
    write_text(cycle.context / "task.md", task_text if task_text.endswith("\n") else f"{task_text}\n")
    if profile_text is not None:
        write_text(cycle.context / "profile.md", profile_text if profile_text.endswith("\n") else f"{profile_text}\n")
    write_text(
        cycle.public / "README_BOUNDARY.md",
        render_boundary_readme(project_name, cycle, repo_root, target_skill_path, manifest_path),
    )
    write_text(cycle.logs / "pain-points.md", render_pain_points_template(task_text))
    write_text(cycle.logs / "pain-points.json", render_pain_points_json_template(task_text))
    write_text(cycle.logs / "root-cause.md", render_root_cause_template(task_text))
    write_text(cycle.logs / "root-cause.json", render_root_cause_json_template(task_text))
    write_text(cycle.logs / "decision.md", render_decision_template())
    write_text(cycle.logs / "decision.json", render_decision_json_template())
    write_text(cycle.logs / "synthesis.md", render_synthesis_template())
    write_text(cycle.logs / "synthesis.json", render_synthesis_json_template())
    write_text(
        cycle.logs / "agent1-feedback.md",
        "# Agent 1 反馈\n\n[在根因分析完成后，把最小 skill 改动写在这里。]\n",
    )
    run_specs = build_run_specs(cycle, local_config["parallel_runs"], local_config["run_id_prefix"])
    write_text(
        cycle.logs / "run-index.json",
        json.dumps(
            {
                "runs": run_specs,
                "synthesis_ready": False,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
    )
    for run_spec in run_specs:
        write_text(Path(run_spec["notes_path"]), render_run_notes_template(run_spec["run_id"], task_text))
        write_text(Path(run_spec["json_path"]), render_run_json_template(run_spec["run_id"], task_text))
    profile_context_block = ""
    if profile_text is not None:
        profile_context_block = f"\n局部配置路径：`{normalize_path_string(cycle.context / 'profile.md')}`\n"
    task_template_block = ""
    if task_source_path is not None:
        task_template_block = f"\n任务模板路径：`{normalize_path_string(cycle.context / 'task.md')}`\n"
    template_values = {
        "PROJECT_NAME": project_name,
        "REPO_ROOT": normalize_path_string(repo_root),
        "TARGET_SKILL_PATH": normalize_path_string(target_skill_path),
        "PROGRAM_PATH": normalize_path_string(cycle.context / "program.md"),
        "TASK": task_text,
        "PUBLIC_DIR": normalize_path_string(cycle.public),
        "RUN_ID": "run-main",
        "RUN_OUTPUT_PATH": normalize_path_string(cycle.logs / "pain-points.md"),
        "RUN_JSON_PATH": normalize_path_string(cycle.logs / "pain-points.json"),
        "PAIN_POINTS_PATH": normalize_path_string(cycle.logs / "pain-points.md"),
        "ROOT_CAUSE_PATH": normalize_path_string(cycle.logs / "root-cause.md"),
        "DECISION_PATH": normalize_path_string(cycle.logs / "decision.md"),
        "RUN_INDEX_PATH": normalize_path_string(cycle.logs / "run-index.json"),
        "SYNTHESIS_PATH": normalize_path_string(cycle.logs / "synthesis.md"),
        "SYNTHESIS_JSON_PATH": normalize_path_string(cycle.logs / "synthesis.json"),
        "AGENT1_FEEDBACK_PATH": normalize_path_string(cycle.logs / "agent1-feedback.md"),
        "PROFILE_CONTEXT_BLOCK": profile_context_block,
        "TASK_TEMPLATE_BLOCK": task_template_block,
    }
    write_text(
        cycle.prompts / "agent1.md",
        render_agent_template("skill-editor.md", template_values),
    )
    write_text(
        cycle.prompts / "agent2.md",
        render_agent_template("blind-runner.md", template_values),
    )
    for run_spec in run_specs:
        run_values = dict(template_values)
        run_values["RUN_ID"] = run_spec["run_id"]
        run_values["RUN_OUTPUT_PATH"] = run_spec["notes_path"]
        run_values["RUN_JSON_PATH"] = run_spec["json_path"]
        write_text(
            Path(run_spec["prompt_path"]),
            render_agent_template("blind-runner.md", run_values),
        )
    write_text(
        cycle.prompts / "agent3.md",
        render_agent_template("root-cause-analyst.md", template_values),
    )
    write_text(
        cycle.prompts / "synthesizer.md",
        render_agent_template("synthesizer.md", template_values),
    )

    print(f"[OK] 已创建 cycle：{normalize_path_string(cycle.root)}")
    print(f"[OK] 辅助工件包：{normalize_path_string(cycle.public)}")
    print(f"[OK] 提示词目录：{normalize_path_string(cycle.prompts)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
