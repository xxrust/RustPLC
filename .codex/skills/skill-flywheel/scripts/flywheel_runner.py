#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from tempfile import NamedTemporaryFile

from init_public_surface import resolve_config_dir, resolve_task_source
from sync_cycle_artifacts import sync_cycle_artifacts as sync_cycle_markdown


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_ROOT = SCRIPT_DIR.parent

DEFAULT_ITERATION_TIMEOUT_SECONDS = 1800
DEFAULT_MAX_ITERATIONS = 10
DEFAULT_IDLE_ITERATIONS = 2


@dataclass(frozen=True)
class RunnerPaths:
    config_dir: Path
    state_file: Path
    progress_file: Path
    log_dir: Path


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="像 Ralph 一样，用 fresh-process 循环驱动 skill-flywheel 持续迭代目标 skill。"
    )
    parser.add_argument("--repo-root", required=True, help="仓库根目录。")
    parser.add_argument("--target-skill-path", required=True, help="目标 skill 目录。")
    parser.add_argument("--task", help="内联真实任务。")
    parser.add_argument(
        "--task-file",
        help="任务模板文件。可传绝对路径，或传 <target-skill-path>/.skill_flywheel/tasks/ 下的相对文件名。",
    )
    parser.add_argument(
        "--tool",
        default="codex",
        choices=["amp", "claude", "codex"],
        help="底层执行器。默认 codex。",
    )
    parser.add_argument(
        "--max-iterations",
        type=int,
        default=DEFAULT_MAX_ITERATIONS,
        help=f"最大迭代轮数。默认 {DEFAULT_MAX_ITERATIONS}。",
    )
    parser.add_argument(
        "--iteration-timeout-seconds",
        type=int,
        default=DEFAULT_ITERATION_TIMEOUT_SECONDS,
        help=f"单轮超时时间。默认 {DEFAULT_ITERATION_TIMEOUT_SECONDS} 秒。",
    )
    parser.add_argument(
        "--max-idle-iterations",
        type=int,
        default=DEFAULT_IDLE_ITERATIONS,
        help=f"连续多少轮没有新 cycle 或非占位 decision 就停止。默认 {DEFAULT_IDLE_ITERATIONS}。",
    )
    parser.add_argument(
        "--state-file",
        help="runner_state.json 路径。默认 <target-skill-path>/.skill_flywheel/runner_state.json。",
    )
    parser.add_argument(
        "--progress-file",
        help="progress.txt 路径。默认 <target-skill-path>/.skill_flywheel/progress.txt。",
    )
    parser.add_argument(
        "--log-dir",
        help="迭代日志目录。默认 <target-skill-path>/.skill_flywheel/runner_logs/。",
    )
    parser.add_argument(
        "--task-label",
        help="便于阅读的任务标签。默认优先取 task-file 文件名，否则取 'inline-task'。",
    )
    parser.add_argument(
        "--reset-state",
        action="store_true",
        help="覆盖旧的 runner_state.json，重新开始。",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="只初始化状态、渲染 prompt 并写入日志目录，不真正执行底层工具。",
    )
    args = parser.parse_args()
    if bool(args.task) == bool(args.task_file):
        parser.error("必须且只能提供一个：--task 或 --task-file。")
    if args.max_iterations < 1:
        parser.error("--max-iterations 必须大于等于 1。")
    if args.iteration_timeout_seconds < 1:
        parser.error("--iteration-timeout-seconds 必须大于等于 1。")
    if args.max_idle_iterations < 1:
        parser.error("--max-idle-iterations 必须大于等于 1。")
    return args


def ensure_runner_paths(target_skill_path: Path, config_dir: Path | None, args: argparse.Namespace) -> RunnerPaths:
    actual_config_dir = config_dir or (target_skill_path / ".skill_flywheel")
    actual_config_dir.mkdir(parents=True, exist_ok=True)
    state_file = Path(args.state_file).resolve() if args.state_file else actual_config_dir / "runner_state.json"
    progress_file = (
        Path(args.progress_file).resolve() if args.progress_file else actual_config_dir / "progress.txt"
    )
    log_dir = Path(args.log_dir).resolve() if args.log_dir else actual_config_dir / "runner_logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    state_file.parent.mkdir(parents=True, exist_ok=True)
    progress_file.parent.mkdir(parents=True, exist_ok=True)
    return RunnerPaths(
        config_dir=actual_config_dir.resolve(),
        state_file=state_file.resolve(),
        progress_file=progress_file.resolve(),
        log_dir=log_dir.resolve(),
    )


def default_task_label(args: argparse.Namespace) -> str:
    if args.task_label:
        return args.task_label
    if args.task_file:
        return Path(args.task_file).stem
    return "inline-task"


def read_json(path: Path) -> dict | None:
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def ensure_progress_file(progress_file: Path, target_skill_path: Path, task_label: str) -> None:
    if progress_file.exists():
        return
    progress_file.write_text(
        "\n".join(
            [
                "# Flywheel Runner Progress",
                f"Started: {utc_now()}",
                f"Target skill: {target_skill_path}",
                f"Task label: {task_label}",
                "---",
                "",
            ]
        ),
        encoding="utf-8",
    )


def build_initial_state(
    repo_root: Path,
    target_skill_path: Path,
    task_text: str,
    task_source_path: Path | None,
    task_label: str,
    tool: str,
    baseline_cycle_name: str | None,
) -> dict:
    return {
        "schema_version": 1,
        "repo_root": str(repo_root),
        "target_skill_path": str(target_skill_path),
        "task_label": task_label,
        "task": task_text,
        "task_source_path": str(task_source_path) if task_source_path else None,
        "tool": tool,
        "baseline_cycle_name": baseline_cycle_name,
        "status": "active",
        "continue_next_iteration": True,
        "iteration_count": 0,
        "idle_iteration_count": 0,
        "last_cycle": None,
        "last_decision": "",
        "last_summary": "",
        "last_iteration_log": None,
        "updated_at_utc": utc_now(),
    }


def load_or_init_state(
    paths: RunnerPaths,
    repo_root: Path,
    target_skill_path: Path,
    task_text: str,
    task_source_path: Path | None,
    task_label: str,
    tool: str,
    reset_state: bool,
) -> dict:
    if paths.state_file.exists() and not reset_state:
        state = read_json(paths.state_file)
        if state is not None:
            return state
    state = build_initial_state(
        repo_root,
        target_skill_path,
        task_text,
        task_source_path,
        task_label,
        tool,
        latest_substantive_cycle_name(paths.config_dir),
    )
    write_json(paths.state_file, state)
    return state


def latest_cycle_dir(config_dir: Path) -> Path | None:
    cycles_dir = config_dir / "cycles"
    if not cycles_dir.exists():
        return None
    cycles = sorted(
        [p for p in cycles_dir.iterdir() if p.is_dir() and p.name.startswith("cycle-")],
        key=lambda item: item.name,
    )
    return cycles[-1] if cycles else None


def latest_cycle_name(config_dir: Path) -> str | None:
    cycle_dir = latest_cycle_dir(config_dir)
    return cycle_dir.name if cycle_dir else None


def latest_substantive_cycle_name(config_dir: Path) -> str | None:
    cycles_dir = config_dir / "cycles"
    if not cycles_dir.exists():
        return None
    cycles = sorted(
        [p for p in cycles_dir.iterdir() if p.is_dir() and p.name.startswith("cycle-")],
        key=lambda item: item.name,
        reverse=True,
    )
    for cycle_dir in cycles:
        decision = read_json(cycle_dir / "logs" / "decision.json")
        if not is_placeholder_decision(decision):
            return cycle_dir.name
    return None


def latest_cycle_dir_after_baseline(config_dir: Path, baseline_cycle_name: str | None) -> Path | None:
    cycles_dir = config_dir / "cycles"
    if not cycles_dir.exists():
        return None
    cycles = sorted(
        [p for p in cycles_dir.iterdir() if p.is_dir() and p.name.startswith("cycle-")],
        key=lambda item: item.name,
    )
    if baseline_cycle_name:
        cycles = [cycle for cycle in cycles if cycle.name > baseline_cycle_name]
    return cycles[-1] if cycles else None


def cycle_name_from_state(state: dict) -> str | None:
    last_cycle = state.get("last_cycle")
    if not last_cycle:
        return None
    return Path(str(last_cycle)).name


def is_placeholder_decision(decision: dict | None) -> bool:
    if not decision:
        return True
    return (
        decision.get("hypothesis_status", "unknown") == "unknown"
        and not decision.get("key_evidence")
        and not decision.get("minimal_actions")
        and not decision.get("next_question")
    )


def sync_state_from_latest_cycle(paths: RunnerPaths, state: dict) -> tuple[dict, bool]:
    baseline_cycle_name = state.get("baseline_cycle_name")
    cycle_dir = latest_cycle_dir_after_baseline(paths.config_dir, baseline_cycle_name)
    if cycle_dir is None:
        return state, False

    decision_json_path = cycle_dir / "logs" / "decision.json"
    decision = read_json(decision_json_path)
    substantive = not is_placeholder_decision(decision)

    state["last_cycle"] = str(cycle_dir)
    if substantive and decision is not None:
        continue_next = bool(decision.get("continue_next_cycle"))
        state["continue_next_iteration"] = continue_next
        state["status"] = "continue" if continue_next else "complete"
        state["last_decision"] = decision.get("hypothesis_status", "") or ""
        summary = decision.get("next_question", "") or ""
        if not summary and decision.get("key_evidence"):
            summary = "; ".join(decision["key_evidence"][:2])
        state["last_summary"] = summary
        state["idle_iteration_count"] = 0
    else:
        state["idle_iteration_count"] = int(state.get("idle_iteration_count", 0)) + 1

    state["updated_at_utc"] = utc_now()
    return state, substantive


def reconcile_state_with_session_scope(paths: RunnerPaths, state: dict) -> dict:
    baseline_cycle_name = state.get("baseline_cycle_name")
    if latest_cycle_dir_after_baseline(paths.config_dir, baseline_cycle_name) is not None:
        return state

    status = str(state.get("status", "")).lower()
    last_cycle_name = cycle_name_from_state(state)
    historical_or_missing_cycle = last_cycle_name is None or (
        baseline_cycle_name is not None and last_cycle_name <= baseline_cycle_name
    )
    completed = status in {"complete", "completed", "stop", "stopped", "done"} or state.get(
        "continue_next_iteration"
    ) is False

    if completed and historical_or_missing_cycle:
        state["status"] = "active"
        state["continue_next_iteration"] = True
        state["last_cycle"] = None
        state["last_decision"] = ""
        state["last_summary"] = (
            "No post-baseline cycle exists for this session yet; historical decisions are context only."
        )
        state["updated_at_utc"] = utc_now()

    return state


def is_complete_state(state: dict) -> bool:
    status = str(state.get("status", "")).lower()
    return status in {"complete", "completed", "stop", "stopped", "done"} or state.get(
        "continue_next_iteration"
    ) is False


def iteration_count_from_state(state: dict) -> int:
    raw_value = state.get("iteration_count", 0)
    try:
        return max(0, int(raw_value))
    except (TypeError, ValueError):
        return 0


def append_progress_entry(
    progress_file: Path,
    iteration: int,
    log_path: Path,
    state: dict,
    output_status: str,
    substantive: bool,
) -> None:
    lines = [
        f"## {utc_now()} - Iteration {iteration}",
        f"- Log: {log_path}",
        f"- Status: {state.get('status', 'unknown')}",
        f"- Continue next iteration: {state.get('continue_next_iteration')}",
        f"- Output status: {output_status}",
        f"- Last cycle: {state.get('last_cycle') or 'none'}",
        f"- Last decision: {state.get('last_decision') or 'none'}",
        f"- Last summary: {state.get('last_summary') or 'none'}",
        f"- Substantive decision observed: {substantive}",
        "---",
        "",
    ]
    with progress_file.open("a", encoding="utf-8") as handle:
        handle.write("\n".join(lines))


def bootstrap_state_for_session(paths: RunnerPaths, state: dict) -> tuple[dict, bool]:
    state = reconcile_state_with_session_scope(paths, state)
    state, substantive = sync_state_from_latest_cycle(paths, state)
    state["updated_at_utc"] = utc_now()
    write_json(paths.state_file, state)
    return state, substantive


def render_prompt(
    repo_root: Path,
    target_skill_path: Path,
    paths: RunnerPaths,
    task_text: str,
    task_source_path: Path | None,
    state: dict,
    iteration: int,
) -> str:
    latest_cycle = state.get("last_cycle") or "none"
    baseline_cycle_name = state.get("baseline_cycle_name") or "none"
    task_source_line = str(task_source_path) if task_source_path else "inline task"
    closeout_helper = SCRIPT_DIR / "sync_cycle_artifacts.py"
    init_command = (
        f"python {SCRIPT_DIR / 'init_public_surface.py'} "
        f"--repo-root {repo_root} "
        f"--target-skill-path {target_skill_path} "
        + (
            f"--task-file {task_source_path.name}"
            if task_source_path and task_source_path.is_file() and paths.config_dir / "tasks" / task_source_path.name == task_source_path
            else ""
        )
    ).strip()

    return f"""# Flywheel Runner Iteration {iteration}

You are running one autonomous `skill-flywheel` iteration for target skill:
`{target_skill_path}`

Repository root:
`{repo_root}`

Runner state file:
`{paths.state_file}`

Progress file:
`{paths.progress_file}`

Task source:
`{task_source_line}`

Latest known cycle:
`{latest_cycle}`

Baseline cycle for this session:
`{baseline_cycle_name}`

Real task:
{task_text}

Required workflow:

1. Read `{paths.state_file}` and `{paths.progress_file}` first.
2. Read the target skill `SKILL.md` and its local `.skill_flywheel/program.md`, `profile.md`, `public_surface.json` if present.
3. If `last_cycle` exists and is newer than the baseline cycle, inspect that cycle's `logs/pain-points.*`, `logs/root-cause.*`, and `logs/decision.*` before deciding whether to open a new cycle.
4. Use `$skill-flywheel` discipline. Do not leave placeholder `decision` / `root-cause` / `pain-points` files behind.
5. Historical cycles at or before the baseline cycle are context only. They do NOT authorize stop for this session.
6. If no post-baseline cycle exists yet, initialize one for this session instead of stopping.
7. If a post-baseline cycle already exists and its substantive decision says stop, keep the state complete and reply with `<promise>COMPLETE</promise>`.
8. Otherwise, perform exactly one minimal next flywheel step:
   - continue the active round to a real decision, or
   - initialize one new cycle and complete that round end-to-end.
   - if `last_cycle` already exists but its `logs/decision.json` is still placeholder content, do not open a new cycle; close out that active cycle first.
9. If you need to initialize a new cycle, use this command shape:
   `{init_command}`
   If the task source is not a local `.skill_flywheel/tasks/*.md` file, use `--task` instead of `--task-file`.
10. Prefer JSON-first closeout for an active cycle:
   - update `logs/pain-points.json`, `logs/root-cause.json`, `logs/decision.json` first
   - then run:
     `python {closeout_helper} --cycle-dir <cycle-dir> --require-non-placeholder-decision --sync-experiments`
   This regenerates the Markdown artifacts from JSON, syncs `experiments.jsonl`, and fails fast if `decision.json` is still placeholder.
11. Keep the result on disk:
   - cycle logs and decision
   - `.skill_flywheel/experiments.jsonl` when a round reaches a conclusion
   - `{paths.state_file}` with at least:
     - `status`: `active` | `continue` | `complete` | `blocked`
     - `continue_next_iteration`: true/false
     - `last_cycle`
     - `last_decision`
     - `last_summary`
     - `updated_at_utc`
12. If the round is blocked on missing user input, set `status` to `blocked`, explain the narrow blocker in `last_summary`, and stop opening new cycles.

Hard constraints:

- Do not pretend `weak-blind` is `clean-room`.
- Do not open multiple fresh cycles in one iteration.
- Do not rely on chat memory as the only state; use the on-disk files above.
- Prefer minimal next action over redesigning the whole system.

Stop condition:

- Stop only if a substantive decision from a cycle newer than the baseline cycle says not to continue, or if this session is genuinely blocked on missing external input.
- In the stop case, set `continue_next_iteration` to `false`, set `status` to `complete`, and reply with:
  `<promise>COMPLETE</promise>`
"""


def build_tool_command(tool: str) -> list[str]:
    if tool == "amp":
        return [resolve_executable("amp"), "--dangerously-allow-all"]
    if tool == "claude":
        return [resolve_executable("claude"), "--dangerously-skip-permissions", "--print"]
    return [
        resolve_executable("codex"),
        "exec",
        "-c",
        "mcp_servers.chrome-devtools.enabled=false",
        "-c",
        'model_reasoning_effort="medium"',
        "--dangerously-bypass-approvals-and-sandbox",
        "-",
    ]


def run_tool(tool: str, prompt_text: str, log_path: Path, cwd: Path, timeout_seconds: int) -> str:
    command = build_tool_command(tool)
    output_status = "ok"

    with log_path.open("w", encoding="utf-8") as log_handle:
        log_handle.write(f"Flywheel iteration log started at {utc_now()}\n")
        log_handle.write(f"Command: {' '.join(command)}\n")
        log_handle.write(f"Working directory: {cwd}\n")
        log_handle.write(f"Timeout seconds: {timeout_seconds}\n")
        log_handle.write("=" * 70 + "\n")
        log_handle.flush()
        process = subprocess.Popen(
            command,
            cwd=cwd,
            stdin=subprocess.PIPE,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
        )
        try:
            process.communicate(prompt_text, timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            output_status = f"timeout:{timeout_seconds}s"
            process.kill()
            process.communicate()
            log_handle.write("\n[runner] iteration timed out\n")
        else:
            if process.returncode != 0:
                output_status = f"exit:{process.returncode}"
    return output_status


def resolve_executable(base_name: str) -> str:
    candidates = [base_name]
    if sys.platform.startswith("win"):
        candidates = [
            f"{base_name}.cmd",
            f"{base_name}.exe",
            f"{base_name}.bat",
            f"{base_name}.ps1",
            base_name,
        ]

    for candidate in candidates:
        found = shutil.which(candidate)
        if found:
            return found

    raise FileNotFoundError(f"找不到底层工具可执行文件：{base_name}")


def maybe_write_prompt_copy(log_dir: Path, iteration: int, prompt_text: str) -> Path:
    prompt_copy = log_dir / f"iter_{iteration:03d}_prompt.md"
    prompt_copy.write_text(prompt_text, encoding="utf-8")
    return prompt_copy


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    target_skill_path = Path(args.target_skill_path).resolve()
    config_dir = resolve_config_dir(target_skill_path, None)
    paths = ensure_runner_paths(target_skill_path, config_dir, args)
    task_text, task_source_path = resolve_task_source(args.task, args.task_file, paths.config_dir)
    task_label = default_task_label(args)

    ensure_progress_file(paths.progress_file, target_skill_path, task_label)
    state = load_or_init_state(
        paths,
        repo_root,
        target_skill_path,
        task_text,
        task_source_path,
        task_label,
        args.tool,
        args.reset_state,
    )
    state, _ = bootstrap_state_for_session(paths, state)

    if is_complete_state(state) and not args.reset_state:
        print(f"[STOP] runner_state 已处于完成态：{paths.state_file}")
        return 0

    base_iteration_count = iteration_count_from_state(state)

    for local_iteration in range(1, args.max_iterations + 1):
        iteration = base_iteration_count + local_iteration
        state["iteration_count"] = iteration
        state["last_iteration_log"] = str(paths.log_dir / f"iter_{iteration:03d}.log")
        state["updated_at_utc"] = utc_now()
        write_json(paths.state_file, state)

        prompt_text = render_prompt(repo_root, target_skill_path, paths, task_text, task_source_path, state, iteration)
        prompt_copy = maybe_write_prompt_copy(paths.log_dir, iteration, prompt_text)
        if args.dry_run:
            print(f"[DRY-RUN] prompt 已写入：{prompt_copy}")
            print(f"[DRY-RUN] state 已写入：{paths.state_file}")
            return 0

        log_path = paths.log_dir / f"iter_{iteration:03d}.log"
        output_status = run_tool(args.tool, prompt_text, log_path, repo_root, args.iteration_timeout_seconds)

        state = read_json(paths.state_file) or state
        state = reconcile_state_with_session_scope(paths, state)
        state, substantive = sync_state_from_latest_cycle(paths, state)
        state["iteration_count"] = iteration
        if state.get("last_cycle"):
            sync_cycle_markdown(Path(str(state["last_cycle"])), sync_experiments=True)
        if not substantive and state.get("status") == "active" and state.get("last_cycle") is None:
            output_status = "state-reconciled"
        state["last_iteration_log"] = str(log_path)
        write_json(paths.state_file, state)
        append_progress_entry(paths.progress_file, iteration, log_path, state, output_status, substantive)

        if is_complete_state(state):
            print(f"[COMPLETE] iteration={iteration} state={paths.state_file}")
            return 0
        if state.get("status") == "blocked":
            print(f"[BLOCKED] iteration={iteration} state={paths.state_file}")
            return 1
        if int(state.get("idle_iteration_count", 0)) >= args.max_idle_iterations:
            state["status"] = "blocked"
            state["continue_next_iteration"] = False
            state["last_summary"] = (
                f"连续 {state['idle_iteration_count']} 轮未观察到新的非占位 decision；停止，避免空转。"
            )
            state["updated_at_utc"] = utc_now()
            write_json(paths.state_file, state)
            append_progress_entry(paths.progress_file, iteration, log_path, state, "idle-stop", False)
            print(f"[IDLE-STOP] iteration={iteration} state={paths.state_file}")
            return 1

        print(f"[CONTINUE] iteration={iteration} log={log_path}")

    print(f"[MAX-ITERATIONS] 达到上限 {args.max_iterations}，请检查：{paths.progress_file}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
