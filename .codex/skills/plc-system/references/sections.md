# plc-system Sections

当起草 `main.system.md` 时，使用本文件。

## 始终包含

- project identity
- system mission
- safety and reliability level
- operating environment
- normal process flow
- abnormal handling
- concurrent task partition
- blocking step expectations
- startup and stop flow
- testing and maintenance modes
- key constraints
- AI generation guidance

## 存在 motion 时补充

- parameter layering
- homing and soft limits
- fault policy
- propagation scope

## Blocking Semantics

system 文档必须明确：

- 哪些活动应拆成独立 task
- 哪些等待属于 blocking step
- 哪些 task 必须在其他 task 阻塞时继续运行
- 哪些资源共享或互斥
