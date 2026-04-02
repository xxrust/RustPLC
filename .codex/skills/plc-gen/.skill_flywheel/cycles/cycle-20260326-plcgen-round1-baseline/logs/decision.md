# 本轮决策

## 假设状态

支持

## 关键证据

- 基线阶段没有显式 Day-1 公开工件，盲测执行者无法只靠目标 skill 稳定回答 launcher、文件顺序和最小验证链。
- `wafer_loader.system.md` 这类确认版输入仍缺少明确的 ingest / assumptions 规则。

## 本轮最小动作

- 为 `plc-gen` 新增本地 `.skill_flywheel/` 配置。
- 导出 Day-1 launcher、system contract gate、validation order、checklist 工件。
- 在 `plc-gen` 本体补充“确认版 `.system.md` 默认直接消费”的规则。

## 是否进入下一轮

是

## 下一轮研究问题

补出公开工件面后，盲测执行者是否已经不再需要越界读取 `references/` 才能回答 scaffold Day-1 问题。
