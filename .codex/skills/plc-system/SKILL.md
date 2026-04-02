---
name: plc-system
description: "在生成 `.plc` 之前，把工艺需求收敛成可供 RustPLC 下游消费的系统语义描述（`.system.md`）。当用户需要分析 PLC 需求、确定 task 划分、blocking 语义、fault 路径、资源边界、axis 策略，或起草/修复 `main.system.md` 时使用。"
---

# plc-system

把含糊工艺需求收敛成一个稳定、可执行、可验证的 system contract。

这个 skill 的任务不是生成 `.plc`，而是把后续 `plc-gen` 真正需要的关键信息钉死。

保持本文件精简。
按需加载 reference：

- `references/workflow.md`
  如何先给建议稿，再问最少阻塞问题。
- `references/sections.md`
  `main.system.md` 应包含哪些稳定章节。
- `references/concurrency-contract.md`
  并发 task、blocking step、wait/timeout/axis 这些绝不能漂移的约束。
- `references/handoff.md`
  交给 `plc-gen` 时必须明确哪些可执行信息。

## Core Rules

1. 先给一个具体系统解释，再补最多 1 到 3 个阻塞问题。
2. 不要把系统建模写成问卷。
3. 不要把并发解释成“单执行指针在 `task.step` 之间跳转”。
4. 对于拓扑已闭合的机构设备，system contract 必须描述“设备动作及其结果枚举”，不要把机构语义下沉成显式传感器 choreography。
5. “拓扑已闭合”的判定以及最小结果集合要求，服从 `AGENTS.md` 中“task 中的设备动作必须保持高层语义”的定义。
6. 当某个 task step 的本意是“让设备动作”，就按 `AGENTS.md` 定义的设备动作结果集合建模，不要把正常闭环写成一串底层 `wait sensor`。
7. 如果当前 DSL/IR 还承载不了某个设备动作结果，必须把它记成能力缺口与 blocker，而不是省略结果或改写成传感器 choreography。
8. 必须明确 task 划分、blocking 预期、fault route、shared resource、axis policy。
9. 输出必须能被 `plc-gen` 直接消费，而不是只留一堆模糊业务描述。

## Source of Truth

优先服从：

- `docs/architecture/signal-direction.md`
- `AGENTS.md`

如果这些长期语义源与临时直觉冲突，服从前者。

## 默认工作方式

1. 先读需求并形成一个具体推荐版本。
2. 若仍有关键歧义，只问 1 到 3 个真正改变系统结构的问题。
3. 按稳定章节写出 `.system.md`。
4. 如果涉及拓扑已闭合机构，明确写出“哪些结果由设备动作语义承担，哪些分支由 task 消费”，且不要把单个设备动作拆成 task 级传感器闭环。
5. 用简短 handoff 告诉下游可以继续 `.plc` generation。
