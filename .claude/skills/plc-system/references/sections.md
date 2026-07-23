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
- process operation scheduling intent
- blocking step expectations
- key constraints
- AI generation guidance

## process operation scheduling intent

When the process moves discrete workpieces or has pipelined station admission, include a section that can be lowered forward into `process_model/process_operation_model.toml` before task/step generation.

It should specify:

- operation classes: feed / acquire / transfer / process / reject / finish
- source and destination locations
- source availability and destination capacity rules
- predecessor completion rules
- shared resources and interference constraints
- scheduling policy, normally opportunistic admission

This section should not be a fixed "part 1, part 2" narrative unless the machine contract truly requires fixed numbering.

The generated task/step flow must later pass `process-model-check`. A system contract that implies opportunistic admission should explicitly say that unrelated candidate operations must not be serialized merely because they appear in one written sequence.

Do not write this section as a retrospective summary of the final task/step code. It is the scheduling source that constrains how task/step may be written.

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
