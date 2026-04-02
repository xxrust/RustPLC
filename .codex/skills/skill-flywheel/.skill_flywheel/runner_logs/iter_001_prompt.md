# Flywheel Runner Iteration 1

You are running one autonomous `skill-flywheel` iteration for target skill:
`E:\personal_project\rust_plc\.codex\skills\skill-flywheel`

Repository root:
`E:\personal_project\rust_plc`

Runner state file:
`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\runner_state.json`

Progress file:
`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\progress.txt`

Task source:
`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\tasks\autonomous-self-improve.md`

Latest known cycle:
`none`

Baseline cycle for this session:
`cycle-20260326-075638`

Real task:
让 `skill-flywheel` 像 Ralph 一样，通过外壳层驱动的 fresh-process 循环持续迭代自己。

今晚的目标只有两件事：

1. 学会像 Ralph 一样用外壳开启迭代，而不是只在单次会话里人工编排。
2. 让外层 runner 至少连续推进 5 轮外层迭代；除非出现硬阻塞，否则不要在第 5 轮之前提前收敛。

执行要求：

1. 优先修 shell runner、后台启动脚本、磁盘状态、进度日志和 stop condition。
2. 每轮都要把真正的研究判断落到 cycle 工件里，不要只写 runner 日志。
3. 每轮只做一个最小 next action，不要把整套系统重写成大工程。
4. 如果本轮只是 `weak-blind`，必须明确标记，不能伪装成 `clean-room`。
5. 如果连续两轮没有新证据，允许提前停止，但必须把原因写进 `runner_state.json`、`progress.txt` 和本轮 cycle 的 `decision`。


Required workflow:

1. Read `E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\runner_state.json` and `E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\progress.txt` first.
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
   `python E:\personal_project\rust_plc\.codex\skills\skill-flywheel\scripts\init_public_surface.py --repo-root E:\personal_project\rust_plc --target-skill-path E:\personal_project\rust_plc\.codex\skills\skill-flywheel --task-file autonomous-self-improve.md`
   If the task source is not a local `.skill_flywheel/tasks/*.md` file, use `--task` instead of `--task-file`.
10. Prefer JSON-first closeout for an active cycle:
   - update `logs/pain-points.json`, `logs/root-cause.json`, `logs/decision.json` first
   - then run:
     `python E:\personal_project\rust_plc\.codex\skills\skill-flywheel\scripts\sync_cycle_artifacts.py --cycle-dir <cycle-dir> --require-non-placeholder-decision`
   This regenerates the Markdown artifacts from JSON and fails fast if `decision.json` is still placeholder.
11. Keep the result on disk:
   - cycle logs and decision
   - `.skill_flywheel/experiments.jsonl` when a round reaches a conclusion
   - `E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\runner_state.json` with at least:
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
