# plc-gen 的飞轮配置

这个目录只服务于 `plc-gen` 的演化，不属于通用 `skill-flywheel`。

## 默认公开工件面

盲测时优先导出 `plc-gen` 自己准备好的 Day-1 辅助工件，而不是把 `references/` 或仓库普通文档整包暴露出去。

当前默认导出：

- `scaffold-day1-launchers.md`
- `scaffold-day1-system-contract-gate.md`
- `scaffold-day1-validation-order.md`
- `scaffold-day1-checklist.md`
- `complex-project-public-brief.md`
- `source-shape-selection.md`
- `delivery-asset-placeholder-replacement.md`
- `delivery-asset-write-map.md`
- `controller-io-modeling-guardrails.md`
- `legacy-io-model-removal.md`
- `operator-command-modeling.md`
- `confirmed-system-lowering.md`
- `intent-alignment-boundary.md`
- `delivery-status-contract.md`
- `optimization-surface.md`
- `control-mode-and-recovery-patterns.md`
- `scenario-toolchain-limitations.md`
- `scenario-friendly-guard-patterns.md`

## 受保护路径

默认不要让盲测执行者读取：

- `src/`
- `crates/`
- `target/`
- `.git/`
- `vendor/`
- `web-ui/`
- 仓库根目录下的大范围 `docs/`、`examples/` 普通文件
- `plc-gen/references/` 原始 reference 文档

## 推荐修复顺序

当 `plc-gen` 在盲测中暴露问题时，优先按这个顺序修：

1. `.skill_flywheel/public/` 里的 Day-1 辅助工件
2. `plc-gen` 自身
3. `skill-flywheel` 导出脚本或仓库级长期语义源

除非明确判定为 `skill-gap`，否则不要把公开面问题直接堆进 `plc-gen`。
