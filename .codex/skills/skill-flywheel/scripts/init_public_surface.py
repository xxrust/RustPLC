#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


DEFAULT_PROFILE = "rust-plc"
PROFILE_INCLUDES = {
    "rust-plc": [
        "README.md",
        "README_EN.md",
        "QUICKSTART.md",
        "AGENTS.md",
        "docs",
        "examples",
        "devices",
        "scenarios",
        ".codex/skills/plc-gen",
        ".codex/skills/plc-system",
    ]
}

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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create a public artifact surface and prompt bundle for a skill-flywheel cycle."
    )
    parser.add_argument("--repo-root", required=True, help="Protected repository root.")
    parser.add_argument("--cycle-dir", required=True, help="Output directory for this cycle.")
    parser.add_argument("--target-skill-path", required=True, help="Skill to evolve or validate.")
    parser.add_argument("--task", required=True, help="Real task for the blind operator.")
    parser.add_argument(
        "--project-name",
        help="Human-friendly project name. Defaults to the repo directory name.",
    )
    parser.add_argument(
        "--profile",
        default=DEFAULT_PROFILE,
        help=f"Public surface profile. Default: {DEFAULT_PROFILE}.",
    )
    parser.add_argument(
        "--include",
        action="append",
        default=[],
        help="Extra repo-relative public path to export. Repeat as needed.",
    )
    parser.add_argument(
        "--exclude-segment",
        action="append",
        default=[],
        help="Extra path segment to block from export. Repeat as needed.",
    )
    return parser.parse_args()


def ensure_dirs(cycle_dir: Path) -> CyclePaths:
    public_dir = cycle_dir / "public"
    logs_dir = cycle_dir / "logs"
    prompts_dir = cycle_dir / "prompts"
    public_dir.mkdir(parents=True, exist_ok=True)
    logs_dir.mkdir(parents=True, exist_ok=True)
    prompts_dir.mkdir(parents=True, exist_ok=True)
    return CyclePaths(root=cycle_dir, public=public_dir, logs=logs_dir, prompts=prompts_dir)


def is_blocked(path: Path, blocked_segments: set[str]) -> bool:
    return any(part in blocked_segments for part in path.parts)


def copy_public_path(repo_root: Path, public_root: Path, relative_path: str, blocked_segments: set[str]) -> dict:
    source = (repo_root / relative_path).resolve()
    if not source.exists():
        return {"path": relative_path, "status": "missing"}
    try:
        source.relative_to(repo_root)
    except ValueError as exc:
        raise ValueError(f"Path escapes repo root: {relative_path}") from exc
    if is_blocked(source.relative_to(repo_root), blocked_segments):
        return {"path": relative_path, "status": "blocked"}

    destination = public_root / source.relative_to(repo_root)
    if source.is_dir():
        copied_files = 0
        for item in source.rglob("*"):
            if not item.is_file():
                continue
            rel = item.relative_to(repo_root)
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


def render_boundary_readme(project_name: str, cycle: CyclePaths, repo_root: Path, manifest_path: Path) -> str:
    return f"""# Public Boundary

Project: {project_name}
Repo root: {normalize_path_string(repo_root)}
Cycle dir: {normalize_path_string(cycle.root)}
Manifest: {normalize_path_string(manifest_path)}

Allowed:
- files under this `public/` directory
- the target skill path referenced in the prompts

Forbidden for the blind operator:
- project source outside this `public/` directory
- protected repo directories such as `src/` and `crates/`

If required information is missing, log it as a pain point instead of crossing the boundary.
"""


def render_pain_points_template(task: str) -> str:
    return f"""# Pain Points

Task:
{task}

## Result

[Summarize what the blind operator managed to do.]

## Pain Points

1. Step:
   Observed blocker:
   Missing artifact or instruction:
   Impact:

2. Step:
   Observed blocker:
   Missing artifact or instruction:
   Impact:
"""


def render_root_cause_template(task: str) -> str:
    return f"""# Root Cause

Task:
{task}

## Findings

1. Pain point:
   Classification:
   Why:
   Minimal fix:

2. Pain point:
   Classification:
   Why:
   Minimal fix:
"""


def render_agent1_prompt(project_name: str, repo_root: Path, public_dir: Path, target_skill_path: Path, task: str, logs_dir: Path) -> str:
    return f"""Use $skill-creator to improve the target skill at {normalize_path_string(target_skill_path)} for project {project_name}.

You may read the repo source at {normalize_path_string(repo_root)} and the target skill.
Real task: {task}
Public bundle used by the no-source operator: {normalize_path_string(public_dir)}
Pain points will be recorded at: {normalize_path_string(logs_dir / "pain-points.md")}
Root-cause findings will be recorded at: {normalize_path_string(logs_dir / "root-cause.md")}

Keep the skill lean. If a blocker is better solved by a public artifact or code change, say so instead of stuffing it into the skill.
"""


def render_agent2_prompt(public_dir: Path, target_skill_path: Path, task: str, logs_dir: Path) -> str:
    return f"""Use the target skill at {normalize_path_string(target_skill_path)} to complete this real task:
{task}

You must stay inside this public workspace:
{normalize_path_string(public_dir)}

Do not read project source or other protected repo paths. This boundary is procedural; honor it strictly.

Write:
1. your result
2. each blocker or inefficiency
3. the exact missing artifact, command, example, or instruction you wanted

Save the blocker list to {normalize_path_string(logs_dir / "pain-points.md")}.
"""


def render_agent3_prompt(task: str, repo_root: Path, target_skill_path: Path, logs_dir: Path) -> str:
    return f"""Analyze the task, the blind operator's output, and the repo source as needed.

Task: {task}
Pain points: {normalize_path_string(logs_dir / "pain-points.md")}
Target skill: {normalize_path_string(target_skill_path)}
Repo root: {normalize_path_string(repo_root)}

Classify each pain point as one of:
- skill-gap
- public-surface-gap
- code-gap
- task-ambiguity

Prefer stable exported artifacts over source-heavy skill additions.

Write findings to {normalize_path_string(logs_dir / "root-cause.md")}. For every skill-gap, specify the minimal delta Agent 1 should add.
"""


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    cycle_dir = Path(args.cycle_dir).resolve()
    target_skill_path = Path(args.target_skill_path).resolve()
    project_name = args.project_name or repo_root.name

    if args.profile not in PROFILE_INCLUDES:
        known = ", ".join(sorted(PROFILE_INCLUDES))
        raise SystemExit(f"Unknown profile '{args.profile}'. Known profiles: {known}")

    cycle = ensure_dirs(cycle_dir)
    blocked_segments = set(DEFAULT_BLOCKED_SEGMENTS)
    blocked_segments.update(args.exclude_segment)

    include_paths = list(PROFILE_INCLUDES[args.profile])
    include_paths.extend(args.include)

    copy_results = [
        copy_public_path(repo_root, cycle.public, rel_path, blocked_segments)
        for rel_path in include_paths
    ]

    manifest_path = cycle.root / "manifest.json"
    manifest = {
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "project_name": project_name,
        "repo_root": normalize_path_string(repo_root),
        "cycle_dir": normalize_path_string(cycle.root),
        "target_skill_path": normalize_path_string(target_skill_path),
        "profile": args.profile,
        "task": args.task,
        "blocked_segments": sorted(blocked_segments),
        "copied_paths": copy_results,
    }
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")

    write_text(cycle.public / "README_BOUNDARY.md", render_boundary_readme(project_name, cycle, repo_root, manifest_path))
    write_text(cycle.logs / "pain-points.md", render_pain_points_template(args.task))
    write_text(cycle.logs / "root-cause.md", render_root_cause_template(args.task))
    write_text(
        cycle.logs / "agent1-feedback.md",
        "# Agent 1 Feedback\n\n[Use this file for minimal skill deltas after root-cause analysis.]\n",
    )
    write_text(
        cycle.prompts / "agent1.md",
        render_agent1_prompt(project_name, repo_root, cycle.public, target_skill_path, args.task, cycle.logs),
    )
    write_text(
        cycle.prompts / "agent2.md",
        render_agent2_prompt(cycle.public, target_skill_path, args.task, cycle.logs),
    )
    write_text(
        cycle.prompts / "agent3.md",
        render_agent3_prompt(args.task, repo_root, target_skill_path, cycle.logs),
    )

    print(f"[OK] Created cycle at {normalize_path_string(cycle.root)}")
    print(f"[OK] Public bundle at {normalize_path_string(cycle.public)}")
    print(f"[OK] Prompts at {normalize_path_string(cycle.prompts)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
