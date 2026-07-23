# System Day-1 Concurrency Guardrails

并发的正确定义：

- 多个 active task 持有独立 task context
- 统一调度器按声明顺序遍历这些 task
- 某个 task 命中 blocking step 只阻塞自己，不应把其他 task 一起停掉

并发不是：

- 一个执行指针在 `task.step` 之间来回跳
- 一个 task 阻塞后所有 task 一起停

当前产品语义下，以下内容默认是 blocking：

- `wait`
- `delay`
- `timeout` 驱动的等待阶段
- `axis.move_relative`
- `axis.move_absolute`
- 依赖外部反馈完成的动作

写 `.system.md` 时，必须明确：

- 哪些活动应拆成独立 task
- 哪些 task 在别的 task blocking 时仍应继续推进
- 哪些等待是 manual wait
- 哪些等待必须 timeout
- 哪些 fault route 是真实恢复路径
- 哪些 actuator / resource 共享或互斥

如果存在 axis，还必须写清：

- move 默认视为长时 blocking 动作
- timeout / reject / motion fault / safety fault 的处理预期
- “运动完成后才能进行下一步”属于后续 step 语义，应在 system contract 中明确
