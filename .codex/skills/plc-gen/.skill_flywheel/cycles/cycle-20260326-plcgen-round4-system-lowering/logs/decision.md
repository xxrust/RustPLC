# 本轮决策

## 假设状态

支持

## 关键证据

- `wafer_loader.system.md` 的关键结构信号已经明确暴露出 lowering 缺口：并发 task、模式矩阵、warning/fault、计数器阈值、共享资源。
- 新增 `confirmed-system-lowering.md` 与 `control-mode-and-recovery-patterns.md` 后，这些信号已有公开建模路径。
- `plc-gen` 本体也已要求先做 lowering 摘要，再进入 `.plc` 交付。

## 本轮最小动作

- 保留新的 lowering 工件。
- 再做一轮验证，确认 `plc-gen` 现在更像“能生成真实 PLC 的 skill”，而不是只会给脚手架建议。

## 是否进入下一轮

是

## 下一轮研究问题

在不新增更多文档的前提下，当前 `plc-gen` 是否已经足以把 `wafer_loader.system.md` 这类确认版合同稳定解释为 `.plc` 结构，而不是只停在 Day-1 scaffold 建议。
