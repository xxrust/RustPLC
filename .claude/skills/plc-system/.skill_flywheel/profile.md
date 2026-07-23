# plc-system 的飞轮配置

这个目录只服务于 `plc-system` 的演化，不属于通用 `skill-flywheel`。

## 默认公开工件面

盲测时优先导出 `plc-system` 自己准备好的 task-specific 辅助工件，而不是把 reference 或仓库文档整包暴露出去。

当前默认导出：

- `system-day1-draft-workflow.md`
- `system-day1-required-sections.md`
- `system-day1-concurrency-guardrails.md`
- `system-day1-handoff-gate.md`
- `system-day1-checklist.md`

## 受保护路径

默认不要让盲测执行者读取：

- `src/`
- `crates/`
- `target/`
- `.git/`
- `vendor/`
- `web-ui/`
- 仓库根目录下的大范围 `docs/`、`examples/` 普通文件
- `plc-system/references/` 原始 reference 文档

## 推荐修复顺序

当 `plc-system` 在盲测中暴露问题时，优先按这个顺序修：

1. `.skill_flywheel/public/` 里的 task-specific 辅助工件
2. `plc-system` 自身
3. 仓库级长期语义源或导出脚本

除非明确判定为 `skill-gap`，否则不要把公开面问题直接堆进 `plc-system`。
