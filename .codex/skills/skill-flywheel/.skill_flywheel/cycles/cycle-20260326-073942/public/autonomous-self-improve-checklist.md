# Autonomous Self-Improve Checklist

配合 `autonomous-self-improve-command.txt` 使用。目标不是只确认脚本能启动，而是确认外层 shell loop 的状态闭环真的成立。

## 启动前

- 确认任务使用 `autonomous-self-improve.md`
- 如果要从头验证本次 session，启动命令里保留 `-ResetState`
- 记录本次后台日志路径，后续只用磁盘状态判断，不依赖聊天上下文

## 运行中必须观察

1. `runner_state.json`
   - `status` 初始应为 `active`
   - `iteration_count` 应随 fresh-process 外层迭代递增，而不是每次都回到 `1`
   - `last_cycle` 只能指向本次 session baseline 之后的新 cycle
2. `progress.txt`
   - 每轮都应追加一段新的 iteration 记录
   - 记录中的 `Last cycle`、`Last decision`、`Last summary` 应与最新 cycle 的 `decision` 一致
3. `runner_logs/`
   - `background_*.out.log` 或 `session_*.log` 里应能看到 shell loop 的轮次推进
   - `iter_*.log` 里应能看到内层 fresh Codex 进程的真实输出，而不是空日志
4. `.skill_flywheel/cycles/`
   - 本次 session 至少要创建 baseline 之后的新 cycle
   - 新 cycle 不能只停在模板；至少要写完 `pain-points`、`root-cause` 和 `decision`

## 什么算通过

- 外层 shell 可以拉起 fresh process，并把轮次推进写回磁盘
- 新 cycle 的 `decision.json` 是非占位内容
- `runner_state.json` 消费的是本次 session 的新 cycle，而不是历史 cycle
- 如果决定继续，`continue_next_iteration` 为 `true`
- 如果决定停止，停止原因同时出现在：
  - 当前 cycle 的 `decision`
  - `runner_state.json`
  - `progress.txt`

## 什么不算通过

- 只看到了启动命令，没有配套状态观察点
- `iteration_count` 每次 fresh-process 都重置成 `1`
- 只更新 `experiments.jsonl`，但 `decision.json` 仍是模板
- 在第 5 轮之前提前停止，却没有明确的硬阻塞或“连续两轮无新证据”说明
- 使用了历史 cycle 的 stop 结论直接结束本次 session
