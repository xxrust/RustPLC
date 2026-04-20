# Optimization Surface

这个工件只回答一个问题：

> 当用户提到“优化”“候选方案”“节拍更短”时，`plc-gen` 现在真实能承诺什么？

## 当前真实能力

RustPLC 当前提供的是 `library` 级 optimization pipeline，不是 CLI `subcommand`。

不要承诺：

- `rust_plc optimize ...`
- `rust_plc optimization ...`
- 任何不存在的 optimization CLI

## 正确说法

- optimization 入口是 `library` API
- 当前没有 optimization CLI `subcommand`
- legality / timing 复用现有 pipeline

## 什么时候提 optimization

只有当用户明确要求：

- 优化现有 PLC
- 比较候选方案
- 讨论 timing 改写

普通“从 `.system.md` 生成项目”的请求，不要把 optimization 当主路径。
