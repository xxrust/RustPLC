# 工艺操作调度层

RustPLC 的拓扑只描述设备、工件位置、容量、连接和资源边界；它不能直接推出唯一正确的 PLC 程序流。
程序流应被视为调度意图的一种可执行投影，而不是调度意图本身。

## 位置

稳定链路为：

```text
Topology + Device Semantics + Workpiece Model + System Contract
    -> Authored Process Operation Model
    -> task/step Program Flow
    -> Runtime / Verification / Codegen
```

`Process Operation Model` 把可调度动作归一成：

- `operation`：一个可执行工艺动作候选；源侧模型应先人工/生成器从 system contract 写出，当前工具可从携带 `effect:` 的 transition 反推作迁移草稿。
- `admission`：动作允许启动的条件，包括 source 可用、destination 有容量、程序 guard、operator edge 和 semantic resource 空闲。
- `effect`：动作对工件 token 的影响，如 acquire、transfer、finish、mount、split、merge。
- `resource`：由 `claim: action_tag <tag> occupies <resource>` 归一出的资源占用意图。

## 边界

这一层不负责直接驱动 IO，也不替代 runtime。

- 拓扑回答“物理上可能/不可能”。
- 工艺操作层回答“什么时候允许某个操作进入调度候选集”。
- task/step 回答“PLC 如何执行这个候选操作”。
- verification 应逐步从只验证程序流，升级为验证程序流是否 refine 工艺操作模型。

## 当前入口

公共代码入口（用于从现有 task/step 反推或校验当前程序流）：

```rust
rust_plc::process_operation::build_process_operation_model(&state_machine, &constraints)
```

CLI 入口：

```bash
rust_plc operation-model <source.plc|source.bundle.toml> --out <process_operation_model.toml|json>
rust_plc process-model-check <source.plc|source.bundle.toml> --model process_model/process_operation_model.toml
```

项目内推荐把它放在源侧阶段目录，例如：

```text
process_model/process_operation_model.toml
```

## Refinement check

`process-model-check` reads the authored `process_model/process_operation_model.toml`, rebuilds the current task/step-derived operation model, and fails when:

- an authored operation is missing from task/step flow
- task/step flow introduces an extra operation not declared in the model
- operation effects, admission rules, resource assumptions, or task/step binding drift
- derived diagnostics report an unjustified same-task predecessor
- split/merge/transform-carrier operations appear before their admission and lineage semantics are modeled completely

The first enforced scheduling diagnostic is `OP-002`: directly connected same-task process operations are serialized without shared endpoint or shared resource.
Program/operator guards remain admission facts; they do not by themselves justify predecessor ordering.
`OP-003` marks split/merge/transform-carrier operations as unsupported by the current refinement checker until type-level source, output capacity, input multiset, and lineage constraints are represented.

`project-check` auto-runs this refinement step when the source project contains `process_model/process_operation_model.toml`.

`operation-model` should be treated as a migration/audit scaffold generator. A generated model becomes authoritative only after review because the command derives an initial contract from current task/step flow.

新项目的正向顺序固定为：

```text
plc/main.system.md
    -> process_model/process_operation_model.toml
    -> 02_process/ task/step
    -> process-model-check
```

不要默认放入 `out/`。`out/` 是可重建产物目录；工艺操作模型是拓扑之后、程序流之前的调度意图输入，应和 `00_topology/`、`02_process/` 等目录平级。

该模型是后续 `plc-system`、`plc-gen`、verification 和调度优化共用的调度意图锚点。
