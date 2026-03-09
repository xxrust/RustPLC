# 轴参数分层说明（Axis Profile Hierarchy）

## 1. 分层模型

轴参数资源按 5 层组织，必须逐层引用：

1. `axis_motor_classes`
2. `axis_families`
3. `axis_models`
4. `axis_configs`
5. `axis_motion_param_sets`

## 2. 关联约束

- `axis_models/*.toml` 必须声明 `family_id`
- `axis_configs/*.toml` 必须声明 `model_id`
- `axis_motion_param_sets/*.toml` 必须声明 `config_id`

设备声明仅允许：

```plc
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}
```

## 3. 动作参数解析优先级

在语义阶段，`axis.move_*` 最终参数按如下顺序决议：

1. 动作内 `speed/acc/dec` inline 覆盖
2. 动作 `params` 引用
3. 设备 `motion_param_set`

示例：

```plc
action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
```

## 4. 编译期诊断对照

| 错误码 | 含义 | 常见修复 |
|---|---|---|
| `AXP-006` | 轴设备出现非规范内联参数 | 改为 `model_ref/config_ref/motion_param_set` |
| `AXP-007~010` | 层级 ID 链不一致 | 修正 family/model/config 引用链 |
| `AXP-011` | 软限位配置不完整或范围反转 | 成对声明 `soft_limit_min/max` 并修正范围 |
| `AXIS-007` | 动作最终参数缺失 | 补齐 speed/acc/dec 来源 |
| `AXIS-009` | 动作参数超过 profile 上限 | 降低 acc/dec 或切换参数集 |
| `AXIS-011` | 绝对位置静态越界 | 修正目标位置或软限位配置 |
| `AXIS-013` | 动作参数包含未知字段 | 仅使用白名单字段 |

## 5. 推荐实施流程

1. 先定 `model_ref` 与 `config_ref`（机械与安装基线）。
2. 再挑默认 `motion_param_set`（工艺节拍基线）。
3. 最后在动作内只做必要的 `speed/acc/dec` 微调。
4. 严禁把“项目临时参数”回写成设备内联字段。

## 6. 调试命令（可直接运行）

```bash
cargo check
cargo test --lib axis_profile
cargo test --test runtime_bridge_us006
cargo run --bin rust_plc -- examples/two_cylinder.plc
```
