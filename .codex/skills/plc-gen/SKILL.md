---
name: plc-gen
description: "基于已确认的系统描述或等价工业控制需求，生成、修复并验证 RustPLC DSL（`.plc`）以及 scaffold 项目中的 `plc/main.plc`。当用户要从需求生成 PLC、修复现有 `.plc`、交付完整 RustPLC 项目、运行验证命令，或询问 RustPLC 已有优化能力该如何正确使用时使用。"
---

# plc-gen

生成能够通过真实 RustPLC 流水线的 `.plc`，而不是只写出“看起来像对的 DSL”。

这个 skill 的职责是：

- 生成或修复 `plc/main.plc`
- 在项目级请求下同时组织 scaffold、scenario 与验证命令
- 明确告知哪些能力是现成产品能力，哪些还不是
- 在用户提到“优化”“提速”“候选方案”时，准确使用现有 optimization 能力边界

不要把本文件写成百科。
按需加载对应 reference：

- `references/workflow.md`
  何时走 scaffold，何时只修单文件，默认交付路径是什么。
- `references/commands.md`
  真实 CLI 命令、launcher 选择、day-1 命令链，以及 installed binary / source workspace 的差异。
- `references/project-layout.md`
  scaffold 后有哪些文件，先改哪些文件。
- `references/generation-rules.md`
  生成 `.plc` 时必须遵守的语义与建模约束。
- `references/optimization.md`
  现有 optimization 能力的真实边界、library API、可支持的 rewrite 类型。
- `references/output-contract.md`
  最终输出必须包含什么，如何表达成功/阻塞/失败。
- `references/troubleshooting.md`
  `--help`、launcher、多 binary、scenario 缺失等常见卡点。

## Source of Truth

先服从这些长期稳定语义源：

- `AGENTS.md`
- `docs/architecture/signal-direction.md`

不要在 skill 中发明第二套并发、blocking、axis、fault 或优化语义。

## Core Rules

1. 只有通过真实 RustPLC 工具链验证的结果，才算完成。
2. 对于项目级请求，优先 scaffold，而不是只回一段孤立 `.plc`。
3. 对于系统意图仍然不稳定的请求，先依赖 `plc-system` 风格的 system contract，再生成 `.plc`。
4. 不要把并发 task 解释成“单执行指针在 `task.step` 之间跳转”。
5. 不要把 optimization 说成已有 CLI；当前只有 library API。

## 默认工作方式

1. 判断这是单文件修复、项目交付，还是 optimization/提速请求。
2. 如果是项目交付，先读 `references/workflow.md`、`references/project-layout.md`、`references/commands.md`。
3. 如果是 `.plc` 生成或修复，先读 `references/generation-rules.md`。
4. 如果用户提到“优化”“更快”“候选方案”，再额外读 `references/optimization.md`。
5. 生成或修复后，必须给出真实验证路径。

## Launcher Discipline

必须先判断用户处于哪种运行环境：

- 已安装 `rust_plc` binary
- RustPLC 源码仓 workspace

这两种环境的命令前缀和工作目录不一样。
不要把“进入 scaffold 目录后运行 `cargo run ...`”当成默认建议，因为 scaffold 本身不是 Cargo 项目。

## Completion Standard

最少要做到以下之一：

- 返回已验证的 `.plc` 或项目产物
- 明确指出真实阻塞 contract 缺口

不要返回“理论上可行”的伪完成答案。
