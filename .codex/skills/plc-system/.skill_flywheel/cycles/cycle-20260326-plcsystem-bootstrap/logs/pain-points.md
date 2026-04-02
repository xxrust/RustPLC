# 痛点记录

任务：
# System Day-1

任务目标：

仅基于真实 `plc-system` skill 和导出的公开工件面，回答下面这个问题：

> 当用户给出一段模糊的工业控制需求时，应该如何先给出一版 `.system.md` 建议稿，并且只追问 1 到 3 个真正会改变 system contract 结构的阻塞问题？

观察点：

- 是否先给具体建议稿，而不是先抛一长串问题
- 是否能稳定保留 `.system.md` 的关键章节
- 是否能正确描述并发 task 与 blocking step 语义
- 是否能说明什么情况下才允许 handoff 给 `plc-gen`

如果盲测执行者必须阅读仓库普通文档才能回答，就应记为 `public-surface-gap` 或 `code-gap`，而不是默默越界。


## 结果

这轮先完成了 `plc-system` 的 flywheel bootstrap：新增本地 `.skill_flywheel/` 配置、5 个 Day-1 task-specific public 工件，并通过真实 `init_public_surface.py` 导出和自动测试验证这些工件已可被当前 `skill-flywheel` 消费。当前证据证明 public surface 已经成形，但仍属于 weak-blind 的 bootstrap 证据，还没有在具体模糊需求上做 clean-room 回答一致性验证。

## 假设观察

partially-supported

## 痛点

1. 步骤：
   运行 `init_public_surface.py` 针对 `plc-system` 初始化 cycle
   观察到的阻塞：
   `plc-system` 原先没有本地 `.skill_flywheel/` 配置，也没有符合当前协议的 `public_surface.json`、任务模板和 experiments 索引。
   缺少的工件或说明：
   兼容当前 runner 的 `.skill_flywheel/` 目录结构，以及可导出的 task-specific public 工件。
   影响：
   没有稳定入口可对 `plc-system` 做真实 flywheel 研究，盲测执行者只能直接读 reference 或仓库普通文档。
2. 步骤：
   仅靠导出的公开工件回答 System Day-1 问题
   观察到的阻塞：
   影响回答稳定性的规则原先分散在 `references/workflow.md`、`sections.md`、`concurrency-contract.md` 和 `handoff.md` 中，没有被收敛成显式 public surface。
   缺少的工件或说明：
   面向 Day-1 的 workflow、required sections、concurrency guardrails、handoff gate 和 checklist 工件。
   影响：
   回答容易漂移成先提问卷、遗漏 `.system.md` 关键章节，或错误地把并发解释成单执行指针在 `task.step` 间跳转。
3. 步骤：
   判断这套 public surface 是否已经足够稳定
   观察到的阻塞：
   当前只验证了导出和协议兼容，没有在具体模糊需求上做 clean-room 盲测。
   缺少的工件或说明：
   下一轮基于真实模糊工艺需求的回答一致性验证。
   影响：
   目前只能确认 bootstrap 成功，不能把 `plc-system` 的 Day-1 public surface 视为最终收敛。
