# 异常恢复模板与顺控 lint

本页提供无实板阶段的顺控恢复基线：

1. 可直接复用的恢复模板（急停 / 掉电 / 传感器卡死）
2. `sequence-lint` 规则：关键路径 wait 必须具备 `timeout: ... -> goto ...`

## 恢复模板示例

- 急停恢复：`examples/recovery_templates/estop_recovery.plc`
- 掉电恢复：`examples/recovery_templates/power_loss_recovery.plc`
- 传感器卡死恢复：`examples/recovery_templates/sensor_stuck_recovery.plc`

这些模板都具备统一结构：

- 正常流程 task (`cycle`)
- 恢复流程 task (`*_recovery`)
- 人工待命 task (`ready`)

其中恢复流程包含“安全位动作 + 告警 + 复位等待”三段式，便于后续扩展到具体设备。

## 关键路径定义

`sequence-lint` 采用如下定义：

- **关键路径 wait**：任意包含 `wait:` 的 step，且不满足以下豁免条件
- **豁免条件 1（DSL 内）**：step 显式声明 `allow_indefinite_wait: true`
- **豁免条件 2（CLI 外挂）**：通过 `--critical-wait-exempt <task.step>` 或 `--critical-wait-exempt <task.*>` 标记

## lint 用法

```bash
cargo run -- sequence-lint examples/recovery_templates/estop_recovery.plc
cargo run -- sequence-lint examples/recovery_templates/estop_recovery.plc \
  --critical-wait-level error
cargo run -- sequence-lint examples/recovery_templates/estop_recovery.plc \
  --critical-wait-level error \
  --critical-wait-exempt ready.wait_start
```

### 级别说明

- `--critical-wait-level warn`：打印告警，进程退出码为 0
- `--critical-wait-level error`：打印错误，存在命中时退出非 0

### 建议实践

- 设备动作相关 wait 默认都应配置 `timeout + goto recovery`
- 人工等待点（如 `ready.wait_start`）优先使用 `allow_indefinite_wait: true`
- 仅在无法修改 DSL 的临时场景下使用 CLI 豁免，并在评审记录中说明原因
