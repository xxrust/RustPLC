# 本轮研究程序

## 研究问题

`plc-system` 是否已经具备最小但稳定的 Day-1 公开工件面，让盲测执行者不读仓库源码也能回答：

- 应该先给出一版怎样的 `.system.md` 建议稿
- 最多只该追问哪 1 到 3 个真正改变 system contract 的阻塞问题
- 并发 / blocking / handoff 的边界该如何稳定表达

## 当前假设

如果把 `plc-system` 的回答顺序、稳定章节、并发 guardrail 和 handoff gate 收敛成显式导出的 task-specific 工件，而不是让盲测执行者自行翻 reference 或仓库文档，那么它更容易稳定产出可被 `plc-gen` 直接消费的 `.system.md` 草稿。

## 成功信号

- `init_public_surface.py` 能在 `plc-system` 上正常初始化 cycle
- 导出的 `public/` 只包含少量 task-specific 辅助工件，而不是仓库普通文档
- 盲测执行者仅靠这些工件就能知道要先给建议稿，再问最多 3 个阻塞问题
- 并发语义不会漂移成“单执行指针在 task.step 间跳转”

## 失败信号

- 本地 `public_surface.json` 与当前 `skill-flywheel` 脚本不兼容
- Day-1 任务仍需要越界读取 `docs/` 或 `AGENTS.md`
- 公开工件不能稳定约束问题数量、章节结构或 handoff 收口

## 决策规则

- 如果属于 `skill-gap`：补 `plc-system` 本体中的回答约束
- 如果属于 `public-surface-gap`：补 `.skill_flywheel/public/` 工件，不再依赖仓库普通文档
- 如果属于 `code-gap`：补 `skill-flywheel` 或导出脚本

## 停止条件

- `plc-system` 的本地 flywheel 配置升级到当前协议，并通过一次真实 `init_public_surface.py` 导出验证
