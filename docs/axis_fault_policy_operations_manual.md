# Axis Fault 策略操作手册

## 1. 适用对象

- 现场实施工程师
- 调试与运维工程师
- 验证与回归维护人员

## 2. 上线前最小检查

1. 轴设备使用分层引用（`model_ref/config_ref/motion_param_set`）。
2. 每个 `axis.move_*` 都有 `timeout + on_reject + on_motion_fault + on_safety_fault`。
3. 若使用细分 matcher（`kind/code`），保留主桶回退。
4. 已配置 `axis_fault_contract`（severity、stop_mode、ack、传播范围）。

## 3. 故障策略矩阵建议

| 故障等级 | 推荐 stop_mode | 复位策略 | 备注 |
|---|---|---|---|
| recoverable | controlled | manual ack + never auto reset | 先记录原因，再由人工确认恢复 |
| non_recoverable | quick | manual ack + never auto reset | 禁止自动重启，要求检修 |
| safety_critical | immediate | manual ack + never auto reset | 立即停机，强制安全态 |

## 4. 传播范围选择建议

- `self`：单轴独立工位，优先默认。
- `group`：功能分组设备（同 `tags.functional_group`）联锁停机。
- `followers`：电子凸轮主从轴链路联停。
- `all`：整线统一停机，仅用于高风险站点。
- `custom`：精确指定目标轴，必须提供 `propagation_targets`。

## 5. 常见故障诊断路径

### 路径 A：`on_reject` 高频触发

1. 检查 `axis.move_*` 参数是否超出 profile（速度/加减速）。
2. 检查 `params` 指向的参数集是否与设备配置匹配。
3. 查看策略日志 message id 是否符合预期 contract。

### 路径 B：`on_motion_fault` 触发

1. 检查驱动器/编码器反馈和使能链路。
2. 校验 `followers` 传播是否导致联动轴一并停机。
3. 回放 `runtime_bridge_us006` 对应场景断言。

### 路径 C：`on_safety_fault` 触发

1. 检查硬件急停、安全回路、门禁联锁输入。
2. 确认 `stop_mode` 是否为现场要求（controlled/quick/immediate）。
3. 检查 `axis_stop_transition_enter/completed` 双阶段日志是否成对出现。

### 路径 D：`timeout` 触发

1. 验证动作 timeout 是否与机械行程匹配。
2. 检查 tick 对齐（timeout 与 `tick_ms` 必须整除）。
3. 若是可恢复路径，确认 `fault.timeout -> handler` 的回退链完整。

## 6. 调试命令清单（可直接运行）

```bash
cargo check
cargo run --bin rust_plc -- examples/two_cylinder.plc
cargo test --test examples_integration
cargo test --test axis_fault_routing_trace_snapshot_us016
cargo test --test runtime_bridge_us006
```

## 7. 现场排障最短流程

1. 先看 `axis_fault_policy` 日志定位策略选择是否正确。
2. 再看 `axis_stop_transition` 日志确认停机阶段是否完整。
3. 对照 DSL 合同检查是否使用了非白名单字段。
4. 用示例 PLC 最小复现（normal/recoverable/nonrecoverable/safety）。
5. 回归 `runtime_bridge_us006` 与 trace 快照测试后再发布。
