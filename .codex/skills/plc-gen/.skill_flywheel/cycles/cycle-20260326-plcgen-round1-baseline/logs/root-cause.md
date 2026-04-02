# 根因分析

任务：
确认 `plc-gen` 在没有本地 flywheel 公开工件面的情况下，是否足以支撑 `wafer_loader.system.md` 级别的 Day-1 项目生成。

## 假设判断

支持

## 结论

1. 痛点：launcher / validation / 文件顺序必须靠 reference 二次拼装
   分类：public-surface-gap
   原因：`plc-gen` 没有本地 `.skill_flywheel/public/`，Day-1 主路径没有被显式导出，盲测执行者只能知道“要去读 reference”。
   最小修复：新增 `scaffold-day1-launchers.md`、`scaffold-day1-validation-order.md`、`scaffold-day1-checklist.md`。

2. 痛点：确认版 `.system.md` 的 ingest 规则不够显式
   分类：skill-gap
   原因：skill 只覆盖了“系统意图不稳定时怎么办”，没有显式约束“系统意图已经稳定时默认直接消费 `.system.md`，只把真正影响结构的未决项留在 assumptions / blockers”。
   最小修复：在 `plc-gen` 本体和 task-specific 工件中补 `system contract gate`。
