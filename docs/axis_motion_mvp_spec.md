# Axis Motion MVP 规范（冻结）

本文档冻结 Axis Motion MVP 语法与语义边界，作为 parser/semantic/runtime/verification 的统一实现合同。

## 1. 目标与范围

- 新增两类运动动作：
  - `axis.move_relative`
  - `axis.move_absolute`
- 两类动作都必须在同一步中显式给出异常分流分支，禁止隐式默认故障路径。
- 本文档只定义 MVP 必选字段、动作结果分类、诊断编号和禁用项。

## 2. 语法（MVP）

在 `task ... { step ... { ... } }` 的 `action:` 语句中使用：

```plc
action: axis.move_relative(<axis_device>, distance: <real>, speed: <real>, acc: <real>, dec: <real>)
  timeout: <ms> -> <timeout_step>
  on_reject -> <reject_step>
  on_motion_fault -> <motion_fault_step>
  on_safety_fault -> <safety_fault_step>

action: axis.move_relative(<axis_device>, distance: <real>, params: <motion_param_set>, speed: <real>)
  timeout: <ms> -> <timeout_step>
  on_reject -> <reject_step>
  on_motion_fault -> <motion_fault_step>
  on_safety_fault -> <safety_fault_step>

action: axis.move_absolute(<axis_device>, position: <real>, speed: <real>, acc: <real>, dec: <real>)
  timeout: <ms> -> <timeout_step>
  on_reject -> <reject_step>
  on_motion_fault -> <motion_fault_step>
  on_safety_fault -> <safety_fault_step>

action: axis.move_absolute(<axis_device>, position: <real>, params: <motion_param_set>, speed: <real>)
  timeout: <ms> -> <timeout_step>
  on_reject -> <reject_step>
  on_motion_fault -> <motion_fault_step>
  on_safety_fault -> <safety_fault_step>
```

字段白名单（严格）：

- `axis.move_relative` 仅允许：`distance/speed/acc/dec/params`
- `axis.move_absolute` 仅允许：`position/speed/acc/dec/params`
- 非白名单字段（如 `vel/jerk`）在 parser 阶段失败，错误码：`[AXIS-013]`
- 不提供并行别名写法（例如 `vel`、`a`、`d`）

## 3. 参数语义（MVP）

- `<axis_device>`：目标设备名，MVP 仅允许 `stepper_motor` 或 `servo_drive`。
- `distance`：相对位移目标（`move_relative`）。
- `position`：绝对位置目标（`move_absolute`）。
- `params`：动作参数集引用（可选），引用后仍可在动作内覆盖。
- `speed/acc/dec`：最终收敛参数；优先级固定为 `inline overrides > params > device.motion_param_set`。
- `timeout: <ms>`：动作超时上界，超时后跳转 `timeout_step`。

轴设备声明白名单（拓扑层）：

- 允许字段：`purpose/model_ref/config_ref/motion_param_set/tags`
- 非规范内联轴参数（如 `max_speed/steps_per_rev`）在语义阶段失败，错误码：`[AXP-006]`

## 4. 固定动作结果分类（必须）

Axis Motion 在 MVP 中固定为 4 类结果，且必须显式分流：

1. `timeout`：在超时上界内未完成。
2. `reject`：控制器在执行前/执行中拒绝该运动请求（参数、状态或调度拒绝）。
3. `motion_fault`：运动子系统故障（驱动、跟踪、编码器等运动域故障）。
4. `safety_fault`：安全域故障（急停、联锁、安全门等安全链触发）。

## 5. 禁用项（MVP）

`op.motor.move_to` 在 Axis Motion MVP 中保持禁用，不得作为等价替代路径继续扩展。

历史原因：
- 旧模板将“运动目标”耦合为“传感器目标”（以传感器触发作为运动完成判据），导致语义边界不清；
- 故障分类混叠在模板展开链路中，无法稳定区分 reject / motion_fault / safety_fault；
- 不利于后续在 IR、runtime 与 verification 中建立一致且可验证的运动故障模型。

## 6. 诊断编号与修复建议模板

### AXIS-001
- 触发条件：`axis.move_*` 缺失 `timeout` 分支。
- 诊断模板：`[AXIS-001] step '<step_name>' is missing timeout branch.`
- 修复建议：`添加 timeout <ms> -> <step>`。

### AXIS-002
- 触发条件：`axis.move_*` 缺失 `on_reject` 分支。
- 诊断模板：`[AXIS-002] step '<step_name>' is missing on_reject branch.`
- 修复建议：`添加 on_reject -> <step>`。

### AXIS-003
- 触发条件：`axis.move_*` 缺失 `on_motion_fault` 分支。
- 诊断模板：`[AXIS-003] step '<step_name>' is missing on_motion_fault branch.`
- 修复建议：`添加 on_motion_fault -> <step>`。

### AXIS-004
- 触发条件：`axis.move_*` 缺失 `on_safety_fault` 分支。
- 诊断模板：`[AXIS-004] step '<step_name>' is missing on_safety_fault branch.`
- 修复建议：`添加 on_safety_fault -> <step>`。

### AXIS-005
- 触发条件：`axis.move_*` 目标设备不是 `stepper_motor` 或 `servo_drive`。
- 诊断模板：`[AXIS-005] axis target '<device_name>' must be stepper_motor or servo_drive.`
- 修复建议：`将目标改为步进/伺服轴设备，避免使用 sensor 或普通 motor 作为 axis target`。

### AXIS-013
- 触发条件：`axis.move_*` 使用了非白名单参数字段。
- 诊断模板：`[AXIS-013] axis.move_* 参数字段 '<field>' 不在白名单中。`
- 修复建议：`仅使用 move_relative(distance/speed/acc/dec/params) 或 move_absolute(position/speed/acc/dec/params)`。

## 7. 非目标（MVP 外）

- 不定义 jerk、S 曲线、插补轨迹、多轴同步。
- 不定义隐式故障默认跳转。
- 不定义向后兼容 `op.motor.move_to` 的自动迁移规则。
