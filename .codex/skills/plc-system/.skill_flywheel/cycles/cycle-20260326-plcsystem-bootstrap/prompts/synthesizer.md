请基于多个 blind-runner 实例的输出做跨实例聚合。

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

实例索引：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\run-index.json`
聚合输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\synthesis.md`
聚合输出 JSON：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\synthesis.json`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\synthesis.md`。
把机器可读聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\plc-system\.skill_flywheel\cycles\cycle-20260326-plcsystem-bootstrap\logs\synthesis.json`。
