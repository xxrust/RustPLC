请改进项目 `rust_plc` 的目标 skill：`E:\personal_project\rust_plc\.codex\skills\plc-system`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `E:\personal_project\rust_plc` 和目标 skill。

研究程序：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\program.md`
真实任务：
# System Day-1

任务目标：

仅基于真实 `plc-system` skill 和导出的公开工件面，回答下面这个问题：

> 当用户给出一段模糊的工业控制需求时，应该如何先给出一版 `.system.md` 建议稿，并且只追问 1 到 3 个真正会改变 system contract 结构的阻塞问题？

观察点：

- 是否先给具体建议稿，而不是先抛一长串问题
- 是否能稳定保留 `.system.md` 的关键章节
- 是否能正确描述并发 task 与 blocking step 语义
- 是否能说明什么情况下才允许 handoff 给 `plc-gen`

如果盲测执行者必须阅读仓库普通文档才能回答，就应记为 `public-surface-gap` 或 `code-gap`，而不是默默越界。


禁止读源码执行者使用的显式辅助工件包：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\public`
痛点记录路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\pain-points.md`
根因分析路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\root-cause.md`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\task.md`


要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\agent1-feedback.md`。
