# 根因分析

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


## 假设判断

支持

## 结论

1. 痛点：Day-1 公开面此前缺失导致必须越界读 reference
   分类：public-surface-gap
   原因：`plc-gen` 缺少本地 `.skill_flywheel/public/` 导出面，launcher、system contract gate、validation order 只能散落在 reference 里。
   最小修复：已通过 `scaffold-day1-*` 工件关闭。

2. 痛点：本轮是否足以直接宣布闭环
   分类：task-ambiguity
   原因：问题不再是能力缺口，而是“这轮混合了实现与观察，是否应该直接停止”。
   最小修复：收窄下一轮问题，只验证 final config 是否稳定闭环。
