请结合任务、盲测执行者的输出，以及必要的仓库源码进行分析。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\context\program.md`
任务：
为 `skill-flywheel` 自己初始化一轮新的研究回合，并在不启动子 agent 的前提下完成一次最小 `weak-blind` 闭环。

要求：

1. 使用提供的命令初始化一个新的 cycle。
2. 盲测观察阶段只允许读取：
   - 目标 skill 的 `SKILL.md`
   - 新 cycle 下的 `context/`
   - 新 cycle 下的 `public/`
3. 基于这些输入，手工完成：
   - `logs/pain-points.md`
   - `logs/pain-points.json`
   - `logs/root-cause.md`
   - `logs/root-cause.json`
   - `logs/decision.md`
   - `logs/decision.json`
4. 如果当前流程缺少单代理 fallback、证据强度标注或闭环步骤说明，把它记录成痛点。
5. 不要把本轮结果写成 `clean-room` 通过；如果只是当前会话内完成的观察，必须明确标记为 `weak-blind`。

痛点日志：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\logs\pain-points.md`
目标 skill：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel`
仓库根目录：`E:\personal_project\rust_plc`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\context\task.md`


把每个痛点分类为以下之一：

- `skill-gap`
- `public-surface-gap`
- `code-gap`
- `task-ambiguity`

要求：

1. 优先建议稳定、显式导出的辅助工件，而不是让盲测执行者直接读取仓库普通文件，或继续往 skill 里塞大量源码知识。
2. 每个痛点都要给出分类、原因和最小修复。
3. 明确判断本轮研究假设是被支持、被削弱，还是证据不足。
4. 如果属于 `skill-gap`，明确写出 Agent 1 需要补上的最小改动。
5. 如果这轮存在多个 blind-runner，先区分“多数实例的共性问题”和“单个实例的偶发问题”，不要把单实例噪声直接升级成全局结论。

把结论写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\logs\root-cause.md`。
把本轮决策写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010148\logs\decision.md`。
