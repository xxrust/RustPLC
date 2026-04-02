# Single-Agent Weak-Blind Checklist

当你不能或不应启动子 agent 时，按这个顺序完成最小闭环：

1. 先只读取：
   - 目标 skill 的 `SKILL.md`
   - 新 cycle 下的 `context/program.md`
   - 新 cycle 下的 `context/task.md`
   - 新 cycle 下的 `public/`
2. 在这个边界里完成盲测观察，并先写：
   - `logs/pain-points.md`
   - `logs/pain-points.json`
3. 只有 pain-points 写完后，才切回可读源码视角，完成：
   - `logs/root-cause.md`
   - `logs/root-cause.json`
   - `logs/decision.md`
   - `logs/decision.json`
4. `logs/*.md` 和对应的 `logs/*.json` 必须保持同一轮结论同步落盘，不能只改 Markdown：
   - `decision.json` 不是附属品；shell runner 的 `last_cycle`、`continue_next_iteration` 和 stop condition 以它为准
   - 如果 `decision.md` 已经写出真实结论，但 `decision.json` 仍保留 `hypothesis_status: "unknown"` / 空 `key_evidence` / 空 `minimal_actions`，这轮仍算未完成 closeout
   - 至少同步这些字段：`hypothesis_status`、`key_evidence`、`minimal_actions`、`continue_next_cycle`、`next_question`
5. 在 `decision` 里明确写出：
   - 本轮是 `weak-blind`
   - 为什么没有启动子 agent
   - 这会怎样降低证据强度
6. 不要把这轮结果写成 `clean-room`，也不要把未执行的并行实例当作已存在证据。

完成后，再补充检查：

- `public/README_BOUNDARY.md` 是否明确了读取边界
- `manifest.json` 是否记录了结构化日志路径
- 本轮最小修复是否已经落在正确层级，而不是只停留在空结论
- `logs/decision.json` 是否已经脱离 placeholder，且与 `logs/decision.md` 的继续 / 停止结论一致
