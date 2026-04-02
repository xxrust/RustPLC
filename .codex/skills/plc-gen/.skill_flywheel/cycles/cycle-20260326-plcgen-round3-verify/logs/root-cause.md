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

1. 痛点：无新的 Day-1 执行阻塞
   分类：task-ambiguity
   原因：当前唯一需要裁决的是“弱盲验证是否已经足够停止”，而不是是否还存在新的能力缺口。
   最小修复：在决策里明确把证据标成 `weak-blind`，但停止当前优化回合。

2. 痛点：不要把本轮写成 clean-room 成功
   分类：task-ambiguity
   原因：当前环境没有外部 clean-room 编排器，本轮只能提供单代理 fallback 证据。
   最小修复：在决策和最终汇报中明确说明证据边界，不继续追加无效轮次。
