# Axis Fault DSL 合同（v1）

## 1. 目的

本文档冻结轴运动与故障处理 DSL 的可接受语法与字段白名单。任何未列出的字段均视为非法输入，必须在编译期失败。

## 2. 轴设备声明合同

轴设备（`stepper_motor` / `servo_drive`）必须使用分层引用：

```plc
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}
```

合同规则：

- `model_ref`：必填
- `config_ref`：必填
- `motion_param_set`：可选
- 禁止内联旧参数写法（如 `speed_default`/`acc_default` 等），违例 `AXP-006`

## 3. `axis_fault_contract` 合同

```plc
axis_fault_contract axis_x_fault {
    axis: axis_x
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: never
    manual_ack_required: true
    propagation_scope: self
}
```

字段白名单：

- `axis`
- `severity`
- `stop_mode`
- `auto_reset_policy`
- `manual_ack_required`
- `propagation_scope`
- `propagation_targets`（仅 `propagation_scope: custom` 时可用）

`propagation_scope` 枚举白名单：

- `self`
- `group`
- `all`
- `followers`
- `custom`

## 4. 轴动作合同

### 4.1 `axis.move_relative`

```plc
action: axis.move_relative(axis_x, distance: 20, params: stepper_default_fast, speed: 2, acc: 8, dec: 8)
    timeout: 400ms -> fault.timeout
    on_reject -> fault.reject_default
    on_reject(kind: vendor) -> fault.reject_vendor
    on_reject(code: 1201) -> fault.reject_code_1201
    on_motion_fault -> fault.motion_fault
    on_safety_fault -> fault.safety_fault
```

### 4.2 `axis.move_absolute`

```plc
action: axis.move_absolute(axis_x, position: 100, params: stepper_default_fast, speed: 2, acc: 8, dec: 8)
    timeout: 400ms -> fault.timeout
    on_reject -> fault.reject_default
    on_motion_fault -> fault.motion_fault
    on_safety_fault -> fault.safety_fault
```

参数白名单：

- `distance`（仅 relative）
- `position`（仅 absolute）
- `params`
- `speed`
- `acc`
- `dec`

未知参数统一报错：`AXIS-013`。

## 5. 路由语义合同

- 主桶 `on_reject/on_motion_fault/on_safety_fault` 为必填回退路径。
- 细分 matcher（`kind` / `code`）是优先匹配层，不可替代主桶。
- 同一桶内按声明顺序首条命中。
- 若无细分命中，回退主桶。

## 6. 参数解析优先级

语义阶段必须按如下优先级得到最终运动参数：

1. 动作内 inline 覆盖（`speed/acc/dec`）
2. 动作 `params` 引用
3. 设备 `motion_param_set`

缺失关键参数、越界、或 profile 不兼容必须在编译期失败（`AXIS-*` / `AXP-*`）。
