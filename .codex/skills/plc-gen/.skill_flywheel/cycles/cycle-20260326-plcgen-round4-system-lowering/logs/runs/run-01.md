# Blind Runner run-01

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

以 `wafer_loader.system.md` 为靶子观察后，能明显看到执行者需要一份 lowering 摘要来决定 task、service、warning、fault、counter 与 resource 结构；补完后，这条主路径已经显式化。

## 假设观察

支持

## 痛点

1. 步骤：
   观察到的阻塞：原始 skill 没有显式 lowering 桶。
   缺少的工件或说明：`confirmed-system-lowering.md`
   影响：复杂 confirmed contract 容易停在“知道该做什么，但没说清怎么建模”。
