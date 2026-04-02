请改进项目 `rust_plc` 的目标 skill：`E:\personal_project\rust_plc\.codex\skills\plc-gen`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `E:\personal_project\rust_plc` 和目标 skill。

研究程序：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\program.md`
真实任务：
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


禁止读源码执行者使用的显式辅助工件包：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\public`
痛点记录路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\pain-points.md`
根因分析路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\root-cause.md`

局部配置路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\profile.md`

任务模板路径：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\context\task.md`


要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`\\?\E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round2-public-surface\logs\agent1-feedback.md`。
