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
- 如果用户要走 scenario 工具链，是否会主动检查并暴露复合 wait guard 的兼容性风险
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 假设判断

支持

## 结论

1. 痛点：无新的 scenario-compatibility 缺口
   分类：task-ambiguity
   原因：当前只剩“是否继续追加同形态弱盲验证”这个问题。
   最小修复：停止，并保留证据边界说明。

2. 痛点：不要把这轮误写成“所有复杂 PLC 都已可直接 scenario-validate”
   分类：task-ambiguity
   原因：这轮提升的是 skill 的识别与应对能力，不是修复了底层工具链限制。
   最小修复：最终汇报中明确区分 skill 提升与产品能力边界。
