# 痛点记录

任务：
# PLC Scaffold Day-1

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `plc/main.system.md`，而且这份 contract 的复杂度接近 `wafer_loader.system.md`，应该如何先指导一个不接触 RustPLC 源码的新手完成 Day-1 scaffold、`plc/main.plc` 生成或修复，以及最小验证？

观察点：

- 是否先判断 launcher，而不是直接甩一串 `cargo run` 命令
- 是否把 `.system.md` 当作主输入，而不是重新发散成大问卷
- 是否先给出一轮从 `.system.md` 到 `.plc` 的 lowering 摘要
- 是否能说清 scaffold 后先改哪些文件
- 是否能把未冻结 contract 留在 assumptions / blockers，而不是默默补齐
- 是否能正确处理并发 task、模式矩阵、warning/fault 分流和计数器
- 如果用户要走 scenario 工具链，是否会主动检查并暴露复合 wait guard 的兼容性风险
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 结果

当前 `plc-gen` 已经能更准确地表达这条现实路径：

- `.system.md` / `.plc` 本身可能是可生成、可维护的
- 但当前 scenario 工具链可能因为复合 wait guard 而无法立即验证
- 这时应报告 toolchain blocker，或提前采用 scenario-friendly lowering

本轮没有再暴露新的 scenario 兼容性盲点。

## 假设观察

本轮支持当前假设：`plc-gen` 更接近一个真实可交付的生成 skill，因为它已经会主动区分 DSL 交付能力与 scenario 工具链能力。

## 痛点

1. 步骤：
   观察到的阻塞：无新的兼容性盲点。
   缺少的工件或说明：无。
   影响：满足停止条件。

2. 步骤：
   观察到的阻塞：本轮仍属 `weak-blind` 证据。
   缺少的工件或说明：无新的 skill / public gap。
   影响：需在最终说明中保留证据边界，但不需要继续追加同形态轮次。
