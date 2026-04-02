# System Day-1 Required Sections

起草 `main.system.md` 时，优先保留这些稳定章节：

- project identity
- system mission
- safety and reliability level
- operating environment
- normal process flow
- abnormal handling
- startup and stop flow
- testing and maintenance modes
- concurrent task partition
- blocking step expectations
- key constraints
- AI generation guidance

当需求中存在 axis 时，还必须补充：

- parameter layering
- homing / soft limits
- fault policy
- propagation scope

判断标准：

- 每一节都要回答一个下游可执行问题
- 不要写成行业背景介绍
- `task partition` 用来定义怎么拆 task
- `blocking step expectations` 用来定义哪些 step 必须等待
- `key constraints` 用来定义 verification 应证明什么
