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
4. 必须明确 task 划分、blocking 预期、fault route、shared resource、axis policy。
5. 输出必须能被 `plc-gen` 直接消费，而不是只留一堆模糊业务描述。

## Source of Truth

优先服从：

- `docs/architecture/signal-direction.md`
- `AGENTS.md`

如果这些长期语义源与临时直觉冲突，服从前者。

## 默认工作方式

1. 先读需求并形成一个具体推荐版本。
2. 若仍有关键歧义，只问 1 到 3 个真正改变系统结构的问题。
3. 按稳定章节写出 `.system.md`。
4. 用简短 handoff 告诉下游可以继续 `.plc` generation。
