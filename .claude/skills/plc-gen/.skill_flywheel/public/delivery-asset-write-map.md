# Delivery Asset Write Map

这个工件只回答一个问题：

> 从 confirmed `.system.md` 到 complex delivery 的时候，每个关键文件到底该写什么？

## 文档层

- `docs/*.system.md`
  写 authoritative intent、工艺目标、关键约束、整机或整线边界
- `docs/*.architecture.md`
  写任务划分、source shape、关键设备/工位分层、fault / warning / mode 结构
- `docs/*.verification.md`
  写 safety / timing / causality / liveness 关注点和验收口径

## 可编译入口

- `plc/main.bundle.toml`
  写 authoritative compile entry；决定哪些 fragments 被真正纳入交付入口
- `plc/target_semantics_fragments/**`
  写 topology、constraints、tasks、faults、operator interface 等可编译语义

## 场景与意图对齐

- `scenarios/nominal/normal.yaml`
  写 nominal 运行入口与场景假设
- `docs/*.intent_alignment.contract.json`
  写 authored sidecar；绑定真实 intent source、真实 milestone、真实 source binding

## root scaffold 文件的职责

- root `plc/main.system.md`
  只做项目级 bridge / 索引
- root `plc/main.target_semantics.bundle.toml`
  只做项目级 compile surface 入口

对 complex delivery，不要把 root scaffold 文件当成唯一交付面。

## 三工位装配线这类输入的提醒

如果 `.system.md` 描述的是多工位整线节拍：

- 先冻结 delivery layer
- 再决定是 `line` asset 还是用户明确要求的单 `station`
- 不要在 `docs/*.system.md` 仍写整线语义，却把交付入口降成默认单 station 壳子
