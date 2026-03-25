# plc-system Sections

起草 `main.system.md` 时，优先保持这些稳定章节。

## 基础章节

- project identity
- system mission
- safety and reliability level
- operating environment
- normal process flow
- abnormal handling
- startup and stop flow
- testing and maintenance modes

## 生成 `.plc` 所必需的章节

- concurrent task partition
- blocking step expectations
- key constraints
- AI generation guidance

## 有 axis 时必须补充的章节

- parameter layering
- homing / soft limits
- fault policy
- propagation scope

## 写章节时的判断标准

每一节都要回答一个下游可执行问题，而不是写成行业背景介绍。

例如：

- task partition 是为了让 `.plc` 知道怎么拆 task
- blocking step expectations 是为了让 `.plc` 知道哪些 step 必须等待
- key constraints 是为了让 verification 知道要证明什么
