# Subtype 标准与 Examples 资产清单（2026-02）

## 1. 建模规范（Subtype First）

RustPLC 拓扑 DSL 现在以 `subtype` 作为设备语义细分的标准字段：

```plc
[topology]

device X0: digital_input

device start_button: sensor {
    subtype: "push_button"
    reports_to: X0
    debounce: 20ms
}
```

`device` 的基础类型（如 `digital_input` / `sensor`）仍然保留；`subtype` 用于表达意图（按钮、限位、急停等）。

### 当前注册的 subtype

| subtype | 兼容基础类型 |
| --- | --- |
| `push_button` | `digital_input`, `sensor` |
| `e_stop_button` | `digital_input`, `sensor` |
| `limit_switch` | `digital_input`, `sensor` |
| `proximity_sensor` | `digital_input`, `sensor` |
| `selector_switch` | `digital_input`, `sensor` |
| `indicator_light` | `digital_output` |

> 来源：`src/device_subtype.rs`

## 2. 可执行 DSL 示例（与当前语法一致）

### 2.1 急停按钮（必须 NC 建模）

```plc
[topology]

device X0: digital_input

device estop_button: sensor {
    subtype: "e_stop_button"
    reports_to: X0
    inverted: true
}
```

`e_stop_button` 若缺少 `inverted: true`，语义门禁会报 `SEM-106`。

### 2.2 选择开关分支

```plc
[topology]

device mode_switch: digital_input { subtype: "selector_switch" }

[constraints]

[tasks]

task choose:
    step decide:
        if: mode_switch == true goto process_A else: goto process_B

task process_A:
    step run:
        action: log "process A selected"

task process_B:
    step run:
        action: log "process B selected"
```

## 3. legacy `type` 迁移说明

### 3.1 兼容规则

- 仅声明 `type`：解析器会自动映射到 `subtype`（兼容旧 DSL）。
- 同时声明 `type` 与 `subtype`：以 `subtype` 为准，并输出迁移提示。
- 未知 `subtype`：语义层给 warning，按基础类型继续处理（不阻断）。

### 3.2 迁移前后对照

```plc
// legacy（仍可解析）
device sensor_arrived: sensor {
    type: "proximity_sensor"
    reports_to: X0
}

// 推荐写法
device sensor_arrived: sensor {
    subtype: "proximity_sensor"
    reports_to: X0
}
```

## 4. Examples 资产清单与策略

### 4.1 保留（文件级 canonical 示例）

- `examples/two_cylinder.plc`
- `examples/assembly_station.plc`
- `examples/force_override_demo.plc`
- `examples/recovery_templates/estop_recovery.plc`
- `examples/stepper_collision_guard.plc`
- `examples/stepper_multi_sensor_consistency.plc`
- `examples/analog_pressure_demo.plc`
- `examples/rp2040_motion_minimal.plc`

### 4.2 内联到测试（不再保留独立 .plc 文件）

以下 DSL 已迁移到 `tests/examples_integration.rs`：

- `half_rotation`
- `delay_demo`
- `repeat_demo`
- `and_or_wait_demo`
- `if_else_demo`
- `custom_states_demo`

### 4.3 删除的重复大型工位示例

- `stamp_bend_line.plc`
- `drill_station.plc`
- `glue_station.plc`
- `grind_station.plc`
- `label_station.plc`

### 4.4 维护原则

- 面向用户学习路径：保留少量可直接运行的 canonical `.plc`。
- 面向行为回归覆盖：优先放在 `tests/*.rs` 内联 DSL fixture。
- 大型拓扑代表样例收敛到 `assembly_station`，其余规模压力通过测试内联生成。

## 5. 推荐入口

- 新 DSL 语法入门：`examples/two_cylinder.plc`
- 大型拓扑与场景工作流：`examples/assembly_station.plc`
- 急停恢复与 subtype 语义：`examples/recovery_templates/estop_recovery.plc`
- 在线强制/调试流程：`examples/force_override_demo.plc`
