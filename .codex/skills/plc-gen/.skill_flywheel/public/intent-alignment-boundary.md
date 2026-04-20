# Intent Alignment Boundary

## Current Rule

For complex RustPLC delivery, the intent sidecar is required by default.

It may be omitted only when one of these is true:
- the task is a tiny local repair
- the user explicitly asked to skip intent alignment
- there is a concrete blocker that is reported explicitly

Do not describe a scaffold placeholder contract as a validated sidecar.
Placeholder digests, unresolved source binding, or starter anchors mean the asset is still blocked.
The bound `source_digest.value` must be the real lowercase SHA-256 hex of the authored source.

这个工件只回答一个问题：

> `*.intent_alignment.contract.json` 什么时候属于本轮 authored sidecar，什么时候可以被显式 blocker 替代？

## complex delivery 默认要生成或修复

只要属于以下任一情况，`plc-gen` 默认就应把 `*.intent_alignment.contract.json` 当成 authored sidecar：

- scaffolded station / module / line 项目
- structured fragment source set
- bundle-based complex delivery
- canonical example、golden path 或 intent fixture 交付
- 用户明确要求 `project-check` 在基础 gate 之外再验证“程序是否做了对的事”

## 可以不生成的例外

只有以下场景才默认不要求 sidecar：

- 普通单文件局部 repair
- 用户明确说“先不要做 intent-alignment”
- 还拿不到真实 authored intent source、source binding 或 anchor evidence，且该 blocker 已被显式报告

## sidecar 的身份

- `*.intent_alignment.contract.json` 是 authored sidecar
- 它不是编译器默认产物
- 它也不是 `project-check` 自动长出来的文件

## 需要同时讲清的工具链产物

如果本轮真的涉及 intent-alignment，还要区分：

- authored sidecar：`*.intent_alignment.contract.json`
- toolchain artifacts：`intent_alignment/report.json`、`sil_trace.jsonl`、其他 gate 报告

## 正确的回答边界

- 可以说“本轮因 blocker 未完成 intent sidecar authoring，因此验证链只覆盖基础 gate”
- 可以说“已按复杂项目默认要求补 sidecar，并在 `project-check` 中追加 intent-alignment 步骤”
- 不要把 scaffold placeholder sidecar 包装成已经完成的 authored sidecar
