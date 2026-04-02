# System Day-1 Checklist

在认为这次 `plc-system` 起草可以交给 `plc-gen` 之前，至少检查：

- 已先给出一版具体建议稿，而不是先发问卷
- 追问数量不超过 3 个，且都是真正改变 system contract 结构的问题
- `.system.md` 已明确 task 划分
- `.system.md` 已明确 blocking / wait / timeout / fault 预期
- `.system.md` 已明确共享资源或互斥边界
- 若存在 axis，已明确 axis fault policy
- 没有把并发写成“单执行指针在 task.step 间跳转”
- 只有在 handoff gate 满足后才使用 handoff 句式
