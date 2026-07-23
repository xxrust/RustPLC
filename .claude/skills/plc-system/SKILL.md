---
name: plc-system
description: "在生成 `.plc` 之前，把工艺需求收敛成可供 RustPLC 下游消费的系统语义描述（`.system.md`）。当用户需要分析 PLC 需求、确定 task 划分、blocking 语义、fault 路径、资源边界、axis 策略，或起草/修复 `main.system.md` 时使用。"
---

# plc-system

把含糊工艺需求收敛成一个稳定、可执行、可验证的 system contract。

这个 skill 的任务不是生成 `.plc`，而是把后续 `plc-gen` 真正需要的关键信息钉死。

保持本文件精简。
按需加载 reference：

- `references/workflow.md`
  如何先给建议稿，再问最少阻塞问题。
- `references/sections.md`
  `main.system.md` 应包含哪些稳定章节。
- `references/concurrency-contract.md`
  并发 task、blocking step、wait/timeout/axis 这些绝不能漂移的约束。
- `references/handoff.md`
  交给 `plc-gen` 时必须明确哪些可执行信息。

## Core Rules

1. 先给一个具体系统解释，再补最多 1 到 3 个阻塞问题。
2. 不要把系统建模写成问卷。
3. 不要把并发解释成“单执行指针在 `task.step` 之间跳转”。
4. 对于拓扑已闭合的机构设备，system contract 必须描述“设备动作及其结果枚举”，不要把机构语义下沉成显式传感器 choreography。
5. “拓扑已闭合”的判定以及最小结果集合要求，服从 `AGENTS.md` 中“task 中的设备动作必须保持高层语义”的定义。
6. 当某个 task step 的本意是“让设备动作”，就按 `AGENTS.md` 定义的设备动作结果集合建模，不要把正常闭环写成一串底层 `wait sensor`。
7. 如果当前 DSL/IR 还承载不了某个设备动作结果，必须把它记成能力缺口与 blocker，而不是省略结果或改写成传感器 choreography。
8. 对加热器、夹爪、输送、泵、比例阀、视觉等过程设备，优先把动作写成设备族语义和结果集合；不要把工程量过程控制下沉成原始 AI/AO 阈值。
9. 必须明确 task 划分、blocking 预期、fault route、shared resource、axis policy、station handoff。
10. 离散工件流必须明确每个 `workpiece_location` / `workpiece_holder` / `workpiece_carrier` 的容量语义。
11. 名称或需求中出现储料盒、料仓、料盒、托盘、缓存位、废料盒、box、bin、rack、magazine、cassette、tray、buffer、hopper 时，默认它是有限多件容器；必须写出容量。未知容量时，写“容量待确认，临时按 N 建模”，不要静默写成 1。
12. 单件工位、夹持位、取料位、加工位、交接位才默认倾向 `capacity: 1`。
13. 输出必须能被 `plc-gen` 直接消费，而不是只留一堆模糊业务描述。
14. 必须区分“本 task 可控反馈”和“不受控他者”：`allow_indefinite_wait` 只适用于操作者、上游/下游 task 或外部 process readiness；home、limit、empty、vacuum、wafer_on 等本地反馈必须建模 timeout、恢复或 fault route。
15. 对每个会被 task 驱动的执行类设备，system contract 必须给出自检策略；如果设备不能自检，必须显式写出豁免理由和 proof_basis，交给 `state-proof-check` 消费。

## Process Operation Scheduling Intent

When the system has discrete workpiece flow, pipelined station behavior, or "which station may accept the next part" decisions, the system contract must include a process-operation scheduling-intent section.

This section must describe:

- candidate operation classes, such as feed, acquire, transfer, process, reject, finish
- source availability rules
- destination capacity rules
- predecessor completion rules
- semantic-resource / interference constraints
- scheduling policy, normally opportunistic admission rather than fixed part numbering

Do not describe the flow as "first workpiece, then second workpiece" unless the source contract truly requires that fixed batch order.
Prefer "when source is available, destination has capacity, required resources are free, and predecessor operation is complete, this operation is admissible."

Handoff to `plc-gen` should expect `plc-gen` to author a source-side model file before generating task/step flow:

```text
process_model/process_operation_model.toml
```

This file is peer-level authored project knowledge, not an `out/` artifact and not a reverse-extraction artifact. Prefer TOML for review; JSON is only for explicit machine interchange.

The handoff order is:

```text
confirmed system contract -> authored process_model/process_operation_model.toml -> task/step flow -> process-model-check
```

`operation-model` may be used only as a migration aid for an existing task/step program or as a comparison scaffold; it must not replace the authored process model when the system contract is still being generated.

The handoff must require `plc-gen` to run `process-model-check` after task/step generation. `OP-002` is not a naming problem; it means the generated program flow has serialized candidate process operations without a modeled endpoint/resource reason. Ordinary operator/program guards are admission facts, not predecessor-completion proof.

## Operator Boundary Rule

操作者不是普通设备。按钮、选择开关、复位、人工确认和 HMI 命令应作为 operator front-door 进入 system contract，而不是把人塞进设备拓扑闭环。

写 `.system.md` 时，凡是涉及人工输入，必须记录：

- actor / role
- command 名称
- 物理来源或 HMI 来源
- trigger 类型，默认按钮为 rising_edge
- allowed_when / rejects_when
- 禁止状态下的 reject_policy
- PLC 必须给操作者的 visible feedback，例如灯、蜂鸣器、HMI 状态或报警文本

底层 `relation { from: start_button.out, to: plc_main.start_cycle_cmd, via: reports_to }` 仍然只表达电气输入映射；front-door 契约表达人的操作语义。
复杂项目应在 `controller.plc` 中用 `controller_io plc_main { ... }` 给 PLC 物理点位定义业务别名；system.md 应优先写这些别名的语义名，而不是把 `X0/Y0` 散落在连接说明里。

设计源：`docs/architecture/operator-boundary-front-door.md`、`docs/architecture/controller-io-aliases.md`。

## Source of Truth

优先服从：

- `docs/architecture/signal-direction.md`
- `docs/architecture/operator-boundary-front-door.md`
- `docs/architecture/controller-io-aliases.md`
- `AGENTS.md`

如果这些长期语义源与临时直觉冲突，服从前者。

## 默认工作方式

1. 先读需求并形成一个具体推荐版本。
2. 若仍有关键歧义，只问 1 到 3 个真正改变系统结构的问题。
3. 按稳定章节写出 `.system.md`。
4. 如果涉及拓扑已闭合机构，明确写出“哪些结果由设备动作语义承担，哪些分支由 task 消费”，且不要把单个设备动作拆成 task 级传感器闭环。
5. 用简短 handoff 告诉下游可以继续 `.plc` generation。
