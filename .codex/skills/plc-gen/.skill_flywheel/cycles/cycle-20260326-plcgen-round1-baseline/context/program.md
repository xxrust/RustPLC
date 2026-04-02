# 本轮研究程序

## 研究问题

在没有本地 `.skill_flywheel/` 配置、也没有显式 Day-1 公开工件时，`plc-gen` 能否仅靠自身稳定指导“已给确认版 `.system.md`”的陌生用户完成 scaffold、`plc/main.plc` 生成和最小验证。

## 当前假设

基线阶段最大的问题不是 reference 不存在，而是 Day-1 主路径没有被显式导出；执行者必须自己在 `references/` 中二次拼接 launcher、文件顺序和 validation order。

## 成功信号

- 仅靠 `plc-gen` 本体就能给出稳定的 launcher 选择
- 能明确把 `.system.md` 当主输入，而不是重新发散成问卷
- 能说清 scaffold 后先改哪些文件，以及最小验证链

## 失败信号

- 执行者必须额外阅读 `references/`
- `.system.md` 已确认时仍不知道哪些项该留在 assumptions
- launcher、验证顺序或项目文件顺序仍需猜测

## 决策规则

- `public-surface-gap`：补 Day-1 工件
- `skill-gap`：补 `.system.md` ingest 规则

## 停止条件

- 得到足够清晰的第一轮改动清单
