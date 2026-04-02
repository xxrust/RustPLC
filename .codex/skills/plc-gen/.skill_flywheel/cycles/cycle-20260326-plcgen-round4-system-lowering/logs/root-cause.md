# 根因分析

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
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 假设判断

支持

## 结论

1. 痛点：confirmed `.system.md` 缺少显式 lowering 摘要
   分类：skill-gap
   原因：`plc-gen` 会说“直接消费 `.system.md`”，但没有要求先把合同压成固定 lowering 桶，导致复杂系统只能靠执行者自行归纳。
   最小修复：在 skill 中加入 lowering 摘要步骤，并导出 `confirmed-system-lowering.md`。

2. 痛点：模式矩阵 / warning / fault 恢复模式缺少公开建模约束
   分类：public-surface-gap
   原因：这部分知识主要藏在现有大示例 `.plc` 中，没有作为通用工件导出。
   最小修复：导出 `control-mode-and-recovery-patterns.md`，并在 checklist 中加入对应检查项。
