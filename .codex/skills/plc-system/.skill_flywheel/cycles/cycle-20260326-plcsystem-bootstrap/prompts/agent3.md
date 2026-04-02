请结合任务、盲测执行者的输出，以及必要的仓库源码进行分析。

研究程序：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\program.md`
任务：
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

痛点日志：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\pain-points.md`
目标 skill：`E:\personal_project\rust_plc\.codex\skills\plc-system`
仓库根目录：`E:\personal_project\rust_plc`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\task.md`


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

把结论写入：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\root-cause.md`。
把本轮决策写入：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\decision.md`。
