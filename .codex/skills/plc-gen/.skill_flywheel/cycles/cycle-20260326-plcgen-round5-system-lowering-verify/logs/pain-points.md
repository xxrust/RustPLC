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
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 结果

在不新增新工件的前提下，当前 `plc-gen` 已经可以先给出一轮 lowering 摘要，再解释 `wafer_loader.system.md` 级别合同如何落成：

- `feed_prep` / `orient_stage` / `transfer_to_measure` 等并发 task
- supervisor / mode_service 这类全局模式与停机管理 task
- warning task 与 fault task 的分流
- `orient_reject_streak` / `drop_total` / `axis_recover_attempts` 这类变量
- `semantic_resource` / `claim` / `conflicts_with` / `requires` 的资源与互锁表达

本轮没有再暴露新的 confirmed-system lowering 缺口。

## 假设观察

本轮进一步支持当前假设：`plc-gen` 已经不只是“会给脚手架建议”，而是更接近“能把确认版合同稳定解释成真实 PLC 结构”的 skill。

## 痛点

1. 步骤：
   观察到的阻塞：无新的 lowering 阻塞。
   缺少的工件或说明：无。
   影响：满足停止条件。

2. 步骤：
   观察到的阻塞：本轮证据仍是 `weak-blind`。
   缺少的工件或说明：无新的 skill / public 工件缺口。
   影响：需要在决策中明确证据边界，但不需要继续追加同形态轮次。
