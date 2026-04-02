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

结合 `wafer_loader.system.md` 与现有 `wafer_loader.plc`，可以确认 `plc-gen` 此前的主要短板不是 launcher 或 scaffold，而是没有显式告诉执行者：

- system contract 里的并发 task 如何直接保留为 `[tasks]`
- 控制模式矩阵何时应提升成独立 `mode_service` / supervisor 结构
- “刷新后续跑”的 warning 路径与“停机告警”的 fault 路径何时必须分开
- 掉片率、连续异常、轴恢复次数这类阈值状态应如何落为 `[topology] variable`
- 资源冲突何时应落为 `semantic_resource` / `claim`

补完 lowering 工件后，这些结构已经可以在不读源码的前提下被显式回答。

## 假设观察

本轮观察支持“`plc-gen` 需要一个 confirmed system -> plc lowering 主路径，而不仅是 Day-1 脚手架指导”这一假设。

## 痛点

1. 步骤：
   观察到的阻塞：原始 skill 没有显式的 lowering 桶，执行者必须自己从 `.system.md` 中提炼 task / mode / fault / counters / resource 结构。
   缺少的工件或说明：`confirmed-system-lowering.md`
   影响：面对 `wafer_loader.system.md` 这类复杂合同，容易退化成“会 scaffold，但不会稳定下沉到 `.plc` 结构”。

2. 步骤：
   观察到的阻塞：模式矩阵和恢复语义此前只隐含在现有示例或已有 `.plc` 里，没有成为公开建模约束。
   缺少的工件或说明：`control-mode-and-recovery-patterns.md`
   影响：执行者可能把 manual / step / maintenance 混进自动主 task，或把 warning / fault 错误合并。
