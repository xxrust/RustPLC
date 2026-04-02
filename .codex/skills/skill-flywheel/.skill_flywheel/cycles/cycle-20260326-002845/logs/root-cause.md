# 根因分析

任务：
为 `skill-flywheel` 自己初始化一轮最小研究回合，并检查输出是否足以支撑一次盲测。

要求：

1. 使用提供的命令初始化一个新的 cycle。
2. 只根据目标 skill 与导出的辅助工件，判断初始化输出是否包含：
   - `context/program.md`
   - `context/task.md`
   - `logs/pain-points.md`
   - `logs/pain-points.json`
   - `logs/root-cause.md`
   - `logs/root-cause.json`
   - `logs/decision.md`
   - `logs/decision.json`
   - `logs/run-index.json`
   - `logs/synthesis.json`
   - `logs/runs/<run-id>.md`
   - `logs/runs/<run-id>.json`
   - `prompts/agent1.md`
   - `prompts/agent2.md`
   - `prompts/agent3.md`
   - `prompts/runs/<run-id>-agent2.md`
3. 如果缺少关键输入、命令或边界说明，把它记录成痛点。
4. 不要读取仓库里的普通文件来补完流程。

## 假设判断

支持。

## 结论

1. 痛点：并行实例元数据是否自动生成
   分类：已验证通过
   原因：`run-index.json` 已自动列出 `smoke-run-01` 和 `smoke-run-02`
   最小修复：无

2. 痛点：并行实例 prompt 与日志骨架是否自动生成
   分类：已验证通过
   原因：`logs/runs/` 和 `prompts/runs/` 已生成实例级文件
   最小修复：无
