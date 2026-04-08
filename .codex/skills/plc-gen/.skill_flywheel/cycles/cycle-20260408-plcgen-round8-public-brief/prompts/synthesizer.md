请基于多个 blind-runner 实例的输出做跨实例聚合。

研究程序：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\program.md`
任务：
# PLC Complex Project Public Brief

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `.system.md`，而且任务同时涉及多文件 bundle 倾向、scenario/gate、并可能要求 intent-alignment 时，主 agent 必须先准备什么 `public brief`，才能把任务 one-shot 地拆给 architect、多个 implementer 和 reviewer？

观察点：

- 是否先说清 `public brief`，而不是直接开始角色分工
- 是否能列出 brief 的最低字段
- 是否能区分 authored artifacts 与 toolchain artifacts
- 是否能说明 architect / implementer / reviewer 各自基于 brief 交付什么
- 如果 brief 不足，是否会要求补 brief，而不是默默越界读源码

如果盲测执行者必须阅读 `references/` 或仓库普通文件才能回答，就应记为 `public-surface-gap`，而不是默默越界。

实例索引：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\run-index.json`
聚合输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\synthesis.md`
聚合输出 JSON：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\synthesis.json`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\synthesis.md`。
把机器可读聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\synthesis.json`。
