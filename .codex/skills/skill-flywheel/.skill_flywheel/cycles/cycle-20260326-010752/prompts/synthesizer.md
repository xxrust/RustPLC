请基于多个 blind-runner 实例的输出做跨实例聚合。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\context\program.md`
任务：
为 `skill-flywheel` 自己初始化一轮新的研究回合，并在不启动子 agent 的前提下完成一次最小 `weak-blind` 闭环。

要求：

1. 使用 `public/single-agent-closeout-command.txt` 中提供的命令初始化一个新的 cycle。
2. 盲测观察阶段只允许读取：
   - 目标 skill 的 `SKILL.md`
   - 新 cycle 下的 `context/`
   - 新 cycle 下的 `public/`
   - 如果需要步骤清单，优先使用 `public/single-agent-closeout-checklist.md`
3. 基于这些输入，手工完成：
   - `logs/pain-points.md`
   - `logs/pain-points.json`
   - `logs/root-cause.md`
   - `logs/root-cause.json`
   - `logs/decision.md`
   - `logs/decision.json`
4. 如果当前流程缺少单代理 fallback、证据强度标注或闭环步骤说明，把它记录成痛点。
5. 不要把本轮结果写成 `clean-room` 通过；如果只是当前会话内完成的观察，必须明确标记为 `weak-blind`。

实例索引：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\logs\run-index.json`
聚合输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\logs\synthesis.md`
聚合输出 JSON：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\logs\synthesis.json`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\logs\synthesis.md`。
把机器可读聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-010752\logs\synthesis.json`。
