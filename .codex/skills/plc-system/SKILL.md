---
name: plc-system
description: "在 PLC code generation 之前，生成或修复 RustPLC 系统语义描述（`.system.md`）。当用户需要分析 PLC 需求、定义项目范围、创建 `main.system.md`，或把工艺意图收敛成稳定 system contract 时使用。"
---

# plc-system

生成一个经过确认、可供下游 PLC generation 信任的 `.system.md`。

保持这个 skill 的边界清晰：
- 只定义 system identity、safety level、process intent、task 边界与关键约束
- 不在这里生成 `.plc`
- 不要输出一整套问卷

保持本文件精简。
按需加载对应 reference 文件：

- `references/workflow.md`
  用于系统确认流程与阻塞问题策略。
- `references/sections.md`
  用于起草或修复 `main.system.md`。
- `references/handoff.md`
  用于生成干净的下游 `plc-gen` 交接说明。

## Required Semantics

把 `docs/architecture/signal-direction.md` 视为以下语义的 source of truth：
- concurrent tasks
- blocking steps
- blocking isolation

系统描述必须能进入以下链路：
- semantic checks
- runtime
- safety / liveness / timing / causality verification

不要把系统描述成“单一执行指针在 `task.step` 间跳转”。

## Default Workflow

1. 先读需求，再先给出一个具体系统解释。
2. 只有在 safety、task 边界或 fault handling 仍有歧义时，才问 1 到 3 个阻塞问题。
3. 产出结构稳定的 `.system.md`。
4. 获取确认，或明确记录 assumptions。
5. 将结果交给 PLC generation。

当信息大体清晰时，优先使用这种响应形态：

```text
当前建议：...
原因：...
请确认。如果不对，请直接给出真实约束。
```

只有在无法负责任地给出建议时，才使用这种响应形态：

```text
我现在还不能负责任地给出建议，因为我仍缺少：...
这会直接影响：...
请确认：...
```

## Preferred Output Sections

默认总是包含：
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

当存在 motion axis 时，补充 axis section：
- parameter layering（`model_ref` / `config_ref` / `motion_param_set`）
- homing / soft limits
- fault policy
- propagation scope

## Task and Blocking Rules

`.system.md` 必须明确：
- 哪些活动应拆成独立 task
- 哪些等待属于 blocking step
- 哪些 task 必须在其他 task 阻塞时继续运行
- 哪些资源共享或互斥

至少明确指出：
- `wait`
- `delay`
- `timeout`
- `axis.move_*`
- 人工确认等待
- 外部反馈等待

## High-Impact Topics

优先处理以下问题或建议：
- system safety class 与 failure consequence
- start mode 与 cycle mode
- startup / reset / e-stop policy
- manual intervention point
- task partition 与 blocking isolation
- shared-resource conflict
- timeout 与 fault routing expectation

第一轮不要把时间花在精确 I/O 编号这种低影响细节上。

## Scaffold Rule

如果请求是完整项目而不是单独 artifact，优先使用 scaffold 布局：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

然后把 system 文件写到：
- `plc/main.system.md`

如果不走 scaffold，就把 `.system.md` 放在目标 `.plc` 旁边。

## Handoff Contract to plc-gen

完成后的 `.system.md` 应该能让 PLC generation 明确决定：
- topology shape
- safety constraints
- task structure
- timeout strategy
- failure tasks
- scenario 与 validation baseline

结尾附上一句简短 handoff：

```text
系统 contract 已确认。继续进行 `.plc` generation。
```
