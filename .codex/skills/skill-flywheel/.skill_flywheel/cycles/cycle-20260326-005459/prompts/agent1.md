请改进项目 `rust_plc` 的目标 skill：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `E:\personal_project\rust_plc` 和目标 skill。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\context\program.md`
真实任务：
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


禁止读源码执行者使用的显式辅助工件包：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\public`
痛点记录路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\logs\pain-points.md`
根因分析路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\logs\root-cause.md`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\context\task.md`


要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-005459\logs\agent1-feedback.md`。
