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
   - `prompts/agent1.md`
   - `prompts/agent2.md`
   - `prompts/agent3.md`
3. 如果缺少关键输入、命令或边界说明，把它记录成痛点。
4. 不要读取仓库里的普通文件来补完流程。
