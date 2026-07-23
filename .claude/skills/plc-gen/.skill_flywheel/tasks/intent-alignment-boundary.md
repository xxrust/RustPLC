# PLC Intent Alignment Boundary

## Current Observation Points

- can the public surface state that complex delivery requires a sidecar by default
- can it distinguish authored sidecar from scaffold placeholder
- can it state that placeholder digest, unresolved source binding, or starter anchors block validation
- can it explain that this boundary is about project delivery, not about `skill-flywheel` optimization parallelism

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户请求 Day-1 项目、bundle 修复或复杂项目交付时，主 agent 应如何判断是否需要 `*.intent_alignment.contract.json`？它和普通 DSL source、scenario、`project-check` 产物的边界是什么？

观察点：

- 是否能给出“只在明确请求时生成”的稳定规则
- 是否能区分 authored sidecar 与 toolchain artifacts
- 是否会把 intent sidecar 误说成 scaffold 默认产物
- 是否能说明未涉及 intent 时应如何表达

如果盲测执行者必须阅读 `references/workflow.md` 或 `references/output-contract.md` 才能回答，就应记为 `public-surface-gap`。
