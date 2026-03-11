# Axis Fault Formal v1 架构说明

## 1. 目标范围

本架构文档覆盖轴运动与故障语义在 RustPLC 的端到端落地路径，作为以下模块的统一参照：

- Parser / AST / Semantic / IR
- Verification（safety / liveness / timing / causality）
- Runtime Bridge / runtime-core
- 示例与回归门禁

## 2. 分层责任

1. **Parser（`src/parser/plc.pest` + `src/parser/mod.rs`）**
   - 解析 `axis.move_relative/axis.move_absolute`、`axis_fault_contract`。
   - 对动作参数执行白名单约束，未知字段统一输出 `AXIS-013`。
2. **AST（`src/ast/mod.rs`）**
   - 保留原始语法结构，不做跨字段推导。
3. **Semantic（`src/semantic/mod.rs`）**
   - 注入设备库与轴参数层级。
   - 计算运动参数优先级：`inline overrides > params > device.motion_param_set`。
   - 将 fault contract 归一化为 IR 可执行策略（含传播目标收敛）。
4. **IR（`src/ir/mod.rs`）**
   - 固化 `AxisFaultKind/AxisFaultCategory`、fault 路由匹配和状态机动作。
5. **Verification（`src/verification/*.rs`）**
   - timing 统计 `axis.move_*` 的内联 timeout。
   - causality 覆盖 `timeout + on_reject/on_motion_fault/on_safety_fault` 全分支。
6. **Runtime Bridge（`src/runtime_bridge.rs`）**
   - 校验 tick 对齐、I/O 可解析性与动作支持边界。
   - 将 fault 策略矩阵降级到 `runtime_core::Program.axis_fault_policies`。
7. **runtime-core（`crates/runtime-core/src/lib.rs`）**
   - 执行 stop 迁移：`Running -> (Controlled|Quick|Immediate)Stopping -> Stopped`。
   - 输出策略日志与停机阶段日志，供回归与现场审计。

## 3. 信号与故障方向

- 拓扑主链：`plc_main.Y* -> axis.enable/pulse -> motion feedback -> plc_main.X*`。
- 故障主桶：`on_reject / on_motion_fault / on_safety_fault`（必填回退）。
- 细分 matcher：`on_* (kind: ...)` / `on_* (code: ...)`，按声明顺序首条命中。
- 未命中细分 matcher 时，始终回退到主桶目标。

## 4. 严格合同（Whitelist）

- `axis.move_relative`：`distance/params/speed/acc/dec` + fault 路由字段。
- `axis.move_absolute`：`position/params/speed/acc/dec` + fault 路由字段。
- `axis_fault_contract.propagation_scope` 仅允许：`self/group/all/followers/custom`。
- 仅 `custom` 可声明 `propagation_targets`，且目标必须是轴设备。

## 5. 运行时可观测性

- 故障策略审计：`axis_fault_policy_log_message_id`。
- 停机迁移审计：`axis_stop_transition_log_message_id(stop_mode, phase)`。
- 推荐回归入口：`tests/runtime_bridge_us006.rs`。

## 6. 可运行调试命令

以下命令可在仓库根目录直接运行：

```bash
cargo check
cargo run --bin rust_plc -- examples/two_cylinder.plc
cargo test --test axis_fault_routing_trace_snapshot_us016
cargo test --test runtime_bridge_us006
```

## 7. 变更门禁清单

当修改轴故障语义时，至少同步检查：

1. `src/parser/plc.pest`
2. `src/parser/mod.rs`
3. `src/semantic/mod.rs`
4. `src/ir/mod.rs`
5. `src/runtime_bridge.rs`
6. `crates/runtime-core/src/lib.rs`
7. `tests/runtime_bridge_us006.rs`
8. `tests/axis_fault_routing_trace_snapshot_us016.rs`
