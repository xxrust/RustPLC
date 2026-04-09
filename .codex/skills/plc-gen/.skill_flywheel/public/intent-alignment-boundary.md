# Intent Alignment Boundary

## Current Rule

For complex RustPLC delivery, the intent sidecar is required by default.

It may be omitted only when one of these is true:
- the task is a tiny local repair
- the user explicitly asked to skip intent alignment
- there is a concrete blocker that is reported explicitly

Do not describe a scaffold placeholder contract as a validated sidecar.
Placeholder digests, unresolved source binding, or starter anchors mean the asset is still blocked.

这个工件只回答一个问题：

> `*.intent_alignment.contract.json` 什么时候属于本轮 authored sidecar，什么时候不该默认生成？

## 只在明确请求时生成或修复

只有以下任一情况成立时，`plc-gen` 才应额外写入 `*.intent_alignment.contract.json`：

- 用户明确要求 intent-alignment
- 用户明确要求 intent gate 或 comparator
- 用户明确要求 `project-check` 在基础 gate 之外再验证“程序是否做了对的事”
- 任务目标本身就是交付 canonical example、golden path 或可复用的 intent fixture

## 默认不要生成

普通 DSL 交付默认只需要：

- `plc/main.system.md`
- DSL source entry
- scenario

如果用户没有显式提出 intent 相关目标，就不要把 sidecar 当成默认交付物。

## sidecar 的身份

- `*.intent_alignment.contract.json` 是 authored sidecar
- 它不是编译器默认产物
- 它也不是 `project-check` 自动长出来的文件

## 需要同时讲清的工具链产物

如果本轮真的涉及 intent-alignment，还要区分：

- authored sidecar：`*.intent_alignment.contract.json`
- toolchain artifacts：`intent_alignment/report.json`、`sil_trace.jsonl`、其他 gate 报告

## 正确的回答边界

- 可以说“本轮未生成 intent sidecar，验证链只覆盖基础 gate”
- 可以说“已按用户要求补 sidecar，并在 `project-check` 中追加 intent-alignment 步骤”
- 不要把“可能以后会需要”包装成默认生成理由
