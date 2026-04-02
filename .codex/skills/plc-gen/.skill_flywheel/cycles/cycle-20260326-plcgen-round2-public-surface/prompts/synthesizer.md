请基于多个 blind-runner 实例的输出做跨实例聚合。

研究程序：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\program.md`
任务：
# PLC Scaffold Day-1

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `plc/main.system.md`，而且这份 contract 的复杂度接近 `wafer_loader.system.md`，应该如何先指导一个不接触 RustPLC 源码的新手完成 Day-1 scaffold、`plc/main.plc` 生成或修复，以及最小验证？

观察点：

- 是否先判断 launcher，而不是直接甩一串 `cargo run` 命令
- 是否把 `.system.md` 当作主输入，而不是重新发散成大问卷
- 是否能说清 scaffold 后先改哪些文件
- 是否能把未冻结 contract 留在 assumptions / blockers，而不是默默补齐
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。

实例索引：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\run-index.json`
聚合输出 Markdown：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\synthesis.md`
聚合输出 JSON：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\synthesis.json`

局部配置路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\profile.md`

任务模板路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\synthesis.md`。
把机器可读聚合结果写入：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\synthesis.json`。
