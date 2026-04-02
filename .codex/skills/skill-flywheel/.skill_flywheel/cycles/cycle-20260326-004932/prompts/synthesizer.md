请基于多个 blind-runner 实例的输出做跨实例聚合。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\context\program.md`
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

实例索引：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\logs\run-index.json`
聚合输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\logs\synthesis.md`
聚合输出 JSON：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\logs\synthesis.json`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\logs\synthesis.md`。
把机器可读聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-004932\logs\synthesis.json`。
