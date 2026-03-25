# plc-system Concurrency Contract

本文件记录 `plc-system` 绝不能漂移的并发与 blocking 语义。

## 1. 并发的正确定义

并发 = 多个 active task 持有独立 task context，由统一调度器按声明顺序遍历。

并发不是：

- 一个执行指针在 `task.step` 之间来回跳
- 一个 task 阻塞后所有 task 一起停

## 2. blocking 集合

当前产品语义下，以下内容默认是 blocking：

- `wait`
- `delay`
- `timeout` 驱动的等待阶段
- `axis.move_relative`
- `axis.move_absolute`
- 依赖外部反馈完成的动作

写 `.system.md` 时，必须把这些 blocking expectation 提前讲清楚。

## 3. 必须在 `.system.md` 钉死的并发信息

- 哪些活动应拆成独立 task
- 哪些 task 在别的 task blocking 时仍应继续推进
- 哪些等待是 manual wait
- 哪些等待必须 timeout
- 哪些 fault route 是真实恢复路径
- 哪些 actuator / resource 共享或互斥

## 4. axis 特别规则

当需求中存在 axis：

- move 默认视为长时 blocking 动作
- 需要说明 timeout / reject / motion fault / safety fault 的处理预期
- 如果 motion 完成后才能做下一件事，必须在系统 contract 中明确这是“后续 step 语义”

## 5. 不合格的 system contract 长什么样

以下写法都不合格：

- “流程从 A 跳到 B 再跳到 C”
- “如果卡住再处理”
- “多个工位以后再决定要不要拆 task”
- “等待应该差不多会结束”

`plc-system` 必须把这些含糊表达改造成可生成 `.plc` 的结构信息。
