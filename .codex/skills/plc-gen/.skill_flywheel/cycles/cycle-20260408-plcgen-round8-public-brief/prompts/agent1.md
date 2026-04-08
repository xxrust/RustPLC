请改进项目 `rust_plc` 的目标 skill：`E:\personal_project\rust_plc\.codex\skills\plc-gen`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `E:\personal_project\rust_plc` 和目标 skill。

研究程序：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\program.md`
真实任务：
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


禁止读源码执行者使用的显式辅助工件包：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\public`
痛点记录路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\pain-points.md`
根因分析路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\root-cause.md`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\context\task.md`


要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260408-plcgen-round8-public-brief\logs\agent1-feedback.md`。
