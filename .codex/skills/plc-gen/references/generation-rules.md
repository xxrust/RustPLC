# plc-gen Generation Rules

本文件记录 `.plc` 生成时不能偏离的硬约束。

## 1. task 与 step 基本约束

输出至少要满足：

- 至少一个 task
- 每个 task 至少一个 step
- task 名称唯一

不要生成“逻辑上像流程，但结构上不闭合”的 DSL。

## 2. 并发与 blocking 语义

必须遵守当前产品语义：

- 并发 = 多个 active task 持有独立 task context
- 不是“单执行点在 task.step 之间跳转”
- `wait`、`delay`、`timeout`、`axis.move_*`、外部反馈动作默认都是 blocking
- 一个 task 被 blocking step 挡住，不得阻塞其他 task

如果一个 station 阻塞时另一个 station 还要继续跑，就必须拆成独立 task。
不要把多工位流程压成一个大 `cycle` task。

## 3. wait / timeout 规则

- manual wait 必须显式使用 `allow_indefinite_wait: true`
- 非 manual wait 必须有 `timeout` 逃生路径
- recovery / fault target 必须是真实 `task.step` 路径，不是抽象描述

## 4. axis 规则

当存在 axis motion 时：

- `axis.move_relative` / `axis.move_absolute` 默认 blocking
- 必须带 `timeout`
- 必须带 `on_reject`
- 必须带 `on_motion_fault`
- 必须带 `on_safety_fault`
- motion 后续 effect 如果依赖“动作完成”，就拆到后续 step

不要把 axis move 写成“本 step 内立刻完成”的普通即时 action。

## 5. topology 与 device 质量

优先保证：

- 每个 device 都有非空 `purpose`
- 用显式 `relation { from, to, via }`
- 端口声明与 relation 真实闭合
- `requires` 用于依赖约束
- `conflicts_with` 只用于真实状态冲突

不要用 `conflicts_with` 表达执行顺序。

## 6. 生成 fault path 时的要求

不要写模糊的“异常处理”注释替代真实 task。
fault handling 必须成为实际 task/step，可被 semantic lowering、runtime bridge 与 verification 消费。
