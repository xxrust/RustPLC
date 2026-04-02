# 目标 Skill 的本地配置

`skill-flywheel` 本身应保持通用。与某个具体 skill、具体项目、具体仓库相关的配置，应放在目标 skill 自己目录下的 `.skill_flywheel/` 中。

例如：

```text
plc-gen/
├── SKILL.md
├── references/
├── .skill_flywheel/
│   ├── program.md
│   ├── profile.md
│   ├── public/
│   ├── public_surface.json
│   ├── experiments.jsonl
│   └── tasks/
│       └── scaffold-day1.md
```

## 推荐内容

### `program.md`

写给本轮研究看的稳定程序。`init_public_surface.py` 会把它复制到本轮 cycle 的 `context/program.md`，供所有角色共享。建议最少包含：

- 本轮研究问题
- 当前假设
- 成功信号
- 失败信号
- 决策规则
- 停止条件

### `profile.md`

写给人看的局部说明。`init_public_surface.py` 会把它复制到本轮 cycle 的 `context/profile.md`，供 Agent 1 和 Agent 3 参考。例如：

- 这个 skill 默认服务哪个项目
- 哪些事实应显式导出为辅助工件
- 哪些仓库路径一律视为受保护路径
- 遇到痛点时推荐的修复顺序

### `experiments.jsonl`

可选的跨轮索引文件。适合长期研究时记录每一轮的 program、关键结论、是否继续和最终落点。

一行可以长这样：

```json
{"cycle":"cycle-20260325-234134","question":"最小 smoke test 是否闭环","decision":"stop","reason":"self-smoke stable"}
```

### `public_surface.json`

写给脚本读的结构化配置。当前脚本会读取它来决定 `.skill_flywheel/public/` 里的哪些辅助工件需要复制到本轮 cycle 的 `public/`。

建议至少包含：

```json
{
  "artifact_paths": [
    "cli/help.txt",
    "diagnostics/schema.json"
  ],
  "parallel_runs": 3,
  "run_id_prefix": "run"
}
```

这些路径都相对于目标 skill 目录下的 `.skill_flywheel/public/`。不要把仓库里的 `README.md`、`docs/`、`examples/` 之类普通文件直接列进来。

可选字段：

- `parallel_runs`
  本轮默认生成多少个 blind-runner 实例脚手架。默认是 `1`。
- `run_id_prefix`
  blind-runner 实例 id 前缀。默认是 `run`。

### `.skill_flywheel/public/`

存放准备显式导出给盲测执行者的辅助工件。这些工件应当是经过挑选或生成的对外输入，而不是把仓库普通文件直接复制过来。

### `tasks/`

存放常用盲测任务模板。`init_public_surface.py` 可通过 `--task-file` 读取这里的模板，并复制到本轮 cycle 的 `context/task.md`。例如：

- day-1 脚手架任务
- 单文件修复任务
- 验证命令辨识任务

## 版本控制建议

适合纳入版本控制：

- `.skill_flywheel/program.md`
- `.skill_flywheel/profile.md`
- `.skill_flywheel/public/**`
- `.skill_flywheel/public_surface.json`
- `.skill_flywheel/experiments.jsonl`
- `.skill_flywheel/tasks/*.md`

不建议默认纳入版本控制：

- `.skill_flywheel/cycles/`
- `.skill_flywheel/logs/`
- `.skill_flywheel/tmp/`

否则每一轮盲测产物都会把仓库弄脏。
