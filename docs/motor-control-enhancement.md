# RustPLC 电机控制能力增强方案

**版本**: 2.5
**日期**: 2026-02-25
**状态**: 设计阶段

---

## 1. 问题陈述

### 1.1 当前电机控制的缺陷

| 问题 | 严重度 | 代码位置 |
|---|---|---|
| `motor` 仅有 `on/off` 两态，无法表达正反转 | 🔴 高 | `safety.rs:666` |
| DSL 层无 `stepper_motor` 类型，RP2040 HAL 层的步进支持无法声明 | 🔴 高 | `plc.pest:20-31` |
| 无 `vfd`（变频器）类型，工业最高频驱动方式缺失 | 🔴 高 | — |
| 无 `servo_drive` 类型，精确定位场景无法表达 | 🟡 中 | — |
| `set` 动作只支持 `on/off`，无法驱动枚举状态（如 `forward/reverse`） | 🔴 高 | `plc.pest:139-141` |
| 新设备参数（`steps_per_rev`、`accel_time` 等）未纳入语法 | 🔴 高 | `plc.pest:33-68` |

### 1.2 Codex 历次审查发现的文档问题

| 版本 | 问题 | 状态 |
|---|---|---|
| v1.0 | `set xxx.direction forward` 无法解析（`binary_output_value` 只接受 `on/off`） | 已修正 |
| v1.0 | 改动清单不完整（漏 `parse_device_type`、`implicit_ports_for_type`、`device_type_label` 等） | 已修正 |
| v1.0 | 设备库 TOML 字段名 `type` 应为 `port_type`（`device_library.rs:38`） | 已修正 |
| v1.0 | `alarm` 与 `fault` 命名冲突 | 已修正（统一为 `fault`） |
| v2.0 | runtime_bridge 枚举值映射破坏方向语义（`value != "off"` 把 `forward/reverse` 都映射为 `true`） | 已修正 |
| v2.0 | `validate_state_reference` 签名返回 `()`，不能 `return Ok(...)` | 已修正 |
| v2.0 | 属性策略矛盾（当前代码对未知属性报错，`parser/mod.rs:350`） | 已修正 |
| v2.0 | 改动清单写"10 处"但列了 12 项；函数名 `default_port_names` 不存在 | 已修正 |
| v2.1 | 前置能力 A 内部矛盾：`action_effect` 改为 String 与"verification 无需改"同时出现 | **本版修正** |
| v2.1 | 多端口语义未闭环：runtime_bridge 按 device 路由忽略 port，safety 验证也不吃 port | **本版修正** |
| v2.1 | `parse_state_reference` 兼容方案需要 `device_types` 参数，但调用链无上下文 | **本版调整：取消兼容方案，改为破坏性迁移** |
| v2.1 | 参数处理写"暂存到 extra_attrs"但该字段不存在，空分支等于参数被吃掉 | **本版修正** |
| v2.1 | 版本号顶部 2.0 底部 v2.1 不一致 | **本版修正** |

### 1.3 多端口语义缺口（系统性问题）

这是本版最重要的修正。Codex 指出：**多端口设备（`stepper_motor`、`vfd`、`servo_drive`）在执行和验证上仍然不正确**，原因是两个底层函数都忽略 `port`：

**`src/runtime_bridge.rs:756`**（`resolve_digital_output_id`）：

```rust
fn resolve_digital_output_id(&self, state_name: &str, device: &str)
    -> Result<DigitalOutputId, BridgeError>
```

只接受 `device`，不接受 `port`。`stepper_motor.enable` 和 `stepper_motor.direction` 会路由到同一个物理通道，产生歧义。

**`src/verification/safety.rs:1014`**（`safety_expr_states_with_reason`）：

```rust
let device_id = model.device_index.get(&state_expr.device).copied()...
```

`device_index` 的键是 `String`（设备名），不含 `port`。多端口设备的不同端口会映射到同一个 `device_id`，状态验证错误。

**结论**：多端口语义需要 runtime_bridge 和 safety 验证引擎同时重构，才能正确支持 `stepper_motor`/`vfd`/`servo_drive`。这是一个独立的大改动，不应与"新增设备类型关键字"耦合。

**分阶段策略**：

| 阶段 | 内容 | 依赖 |
|---|---|---|
| 阶段 0 | 前置能力：`set` 枚举状态 + 属性白名单扩展 | 无 |
| 阶段 1 | 新增设备类型关键字（DSL 可解析，但运行时/验证仍按 on/off 回退） | 阶段 0 |
| 阶段 2 | 多端口语义闭环（runtime_bridge + safety 重构） | 阶段 1 |
| 阶段 3 | 新设备类型完整验证（依赖阶段 2 的端口路由） | 阶段 2 |

阶段 1 完成后，新设备类型可以在 DSL 中声明和编译，但运行时执行和安全验证仍有限制（见第 3 节）。阶段 2 是独立的系统性重构，本文档只描述其接口契约，不展开实现细节。

---

## 2. 前置能力改造（阶段 0）

### 2.1 前置能力 A：`set` 支持枚举状态

**现状**（`src/parser/plc.pest:139-141`）：

```pest
binary_output_value = { "on" | "off" }
action_set = { "set" ~ action_target ~ binary_output_value }
```

`set` 只能写 `on` 或 `off`，`forward`/`reverse`/`active`/`idle` 会解析失败。

**目标**：

```plc
action: set stepper_x.direction forward
action: set vfd_main.run on          # 标准写法
```

**改动方案**：

**`src/parser/plc.pest`**：

```pest
# 改前
binary_output_value = { "on" | "off" }
action_set = { "set" ~ action_target ~ binary_output_value }

# 改后：state_value 接受任意 identifier
state_value = @{ identifier }
action_set  = { "set" ~ action_target ~ state_value }
```

**`src/ast/mod.rs`**（`ActionStatement`）：

```rust
// 改前
pub enum ActionStatement {
    Set { target: ActionTarget, value: BinaryValue },
    // ...
}

// 改后：value 改为 String，在语义层做映射
pub enum ActionStatement {
    Set { target: ActionTarget, value: String },
    // ...
}
```

**`src/parser/mod.rs`**（`parse_action_statement`，约第 735 行）：

```rust
// 改前
Rule::action_set => {
    let mut inner = pair.into_inner();
    let target = parse_action_target(inner.next().unwrap())?;
    let value = match inner.next().unwrap().as_str() {
        "on" => BinaryValue::On,
        "off" => BinaryValue::Off,
        _ => unreachable!(),
    };
    Ok(ActionStatement::Set { target, value })
}

// 改后
Rule::action_set => {
    let mut inner = pair.into_inner();
    let target = parse_action_target(inner.next().unwrap())?;
    let value = inner.next().unwrap().as_str().to_string();
    Ok(ActionStatement::Set { target, value })
}
```

**`src/semantic/mod.rs`**（`action_to_transition_action`）：

在语义层将 `String` 映射回 `BinaryValue`，IR 层和 runtime_bridge 保持不变：

```rust
// action_to_transition_action 当前签名返回 TransitionAction（不是 Result），
// 不能在内部 return Err。非法枚举值的拦截由前置语义校验 pass 完成。
ActionStatement::Set { target, value } => {
    let ir_value = match value.as_str() {
        "on" | "forward" | "active" => IrBinaryValue::On,
        "off" | "reverse" | "idle"  => IrBinaryValue::Off,
        other => unreachable!(
            "set 枚举值应在 validate_set_enum_values 阶段被拦截，实际值: {other}"
        ),
    };
    TransitionAction::Set {
        target: target.device.clone(),
        port:   target.port.clone(),
        value:  ir_value,
    }
}
```

新增语义校验 pass `validate_set_enum_values`（在 lowering 前执行，返回 `Vec<PlcError>`），对 `ActionStatement::Set` 的值做白名单校验（`on/off/forward/reverse/active/idle`）。例如 `set x.direction diagonal` 会在该阶段直接报错，不会流入 `action_to_transition_action`。

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`semantic/mod.rs`（4 个文件）

---

### 2.2 前置能力 B：属性白名单扩展

**现状**（`src/parser/mod.rs:350`）：

```rust
_ => {
    return Err(PlcError::parse(line, format!("不支持的属性名: {attr_name}")));
}
```

未知属性名**直接报错**，不是静默忽略。新设备类型的参数（`steps_per_rev`、`accel_time` 等）必须加入白名单才能解析。

**方案：扩展 `plc.pest` 白名单 + `apply_attribute` 存储分支（写入 `extra_params`）**

**`src/parser/plc.pest:33`**：

```pest
attribute_name = {
    // 现有属性（保持不变）...
    | "pv" | "sp" | "kp" | "ki" | "kd" | "out" | "period_ms" | "limit"
    // 新增：电机控制参数
    | "steps_per_rev"
    | "max_speed"
    | "accel_time"
    | "decel_time"
    | "encoder_resolution"
    | "electronic_gear_num"
    | "electronic_gear_den"
    | "positioning_window"
    | "rated_power"
    | "rated_freq"
}
```

**`src/parser/mod.rs`**（`apply_attribute`，约第 264 行）：

```rust
// 新增分支：存入 extra_params，不做语义校验
"steps_per_rev" | "max_speed" | "accel_time" | "decel_time"
| "encoder_resolution" | "electronic_gear_num" | "electronic_gear_den"
| "positioning_window" | "rated_power" | "rated_freq" => {
    attributes.extra_params.insert(attr_name.to_string(), value.as_str().to_string());
}
```

**`src/ast/mod.rs`**（`DeviceAttributes`）：

```rust
pub struct DeviceAttributes {
    // 现有字段...

    // 新增：存储设备类型特定参数，key=属性名，value=原始字符串
    // 阶段 3 再做类型化解析和语义校验
    #[serde(default)]
    pub extra_params: HashMap<String, String>,
}
```

**说明**：Codex 指出空分支会让拼写错误（如 `steps_per_revv: 200`）静默通过。落 `extra_params` 后，参数被保存，后续可以：
1. 在设备库加载时对照 TOML 的 `[[parameters]]` 做已知参数名校验
2. 阶段 3 做类型化解析（整数/浮点/时长）

相比加 warning，落 `extra_params` 是更好的方案——warning 在 `apply_attribute` 里不好加（该函数只负责解析，不持有诊断上下文），而存储参数不增加复杂度，且为后续校验保留了数据。

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`（3 个文件）

---

## 3. 新设备类型改动集（阶段 1）

### 3.1 改动位置总览（15 处）

| # | 文件 | 函数/位置 | 改动内容 |
|---|---|---|---|
| 1 | `src/parser/plc.pest:20` | `device_type` 规则 | 新增关键字（长关键字排在 `motor` 之前） |
| 2 | `src/parser/mod.rs:212` | `parse_device_type` | 新增 `match` 分支 |
| 3 | `src/ast/mod.rs:28` | `DeviceType` 枚举 | 新增变量 |
| 4 | `src/ir/mod.rs` | `DeviceKind` 枚举 | 新增变量 |
| 5 | `src/semantic/mod.rs:61` | `device_type_str` | 新增分支 |
| 6 | `src/semantic/mod.rs:186` | `implicit_port_ids_for_device_type` | 新增分支（返回端口 ID 列表） |
| 7 | `src/semantic/mod.rs:479` | `device_type_name` | 新增分支（exhaustive match，漏掉编译失败） |
| 8 | `src/semantic/mod.rs:2344` | `default_states_for_kind` | 新增分支（exhaustive match on `DeviceKind`） |
| 9 | `src/semantic/mod.rs:3185` | `device_kind_name` | 新增分支（exhaustive match on `DeviceKind`） |
| 10 | `src/semantic/mod.rs:3134` | `map_device_kind` | 新增分支 |
| 11 | `src/verification/safety.rs:652` | `collect_device_domains` | 新增回退分支 |
| 12 | `src/topology_semantic_gate.rs:459` | `implicit_ports_for_type` | 新增端口定义（含类型和角色） |
| 13 | `src/topology_semantic_gate.rs:811` | `device_type_name` | 新增分支 |
| 14 | `src/device_subtype.rs:62` | `device_type_label` | 新增分支 |
| 15 | `devices/<type>.toml` | 设备库文件 | 新增文件 |

注：#7、#8、#9 是 Codex v3 审查新发现的遗漏。这三个函数都是 exhaustive match，Rust 编译器会在新增枚举变量后直接报错，不会静默跳过，但文档必须列出以确保实施者不遗漏。

### 3.2 阶段 1 的限制

阶段 1 完成后，新设备类型可以在 DSL 中声明和编译，但有以下限制：

| 限制 | 原因 | 解除条件 |
|---|---|---|
| 多端口设备的不同端口会路由到同一物理通道 | `resolve_digital_output_id` 只接受 `device`，不接受 `port` | 阶段 2 重构 runtime_bridge |
| 安全验证按设备整体状态，不区分端口 | `device_index` 键是设备名，不含 `port` | 阶段 2 重构 safety 引擎 |
| `set stepper_x.direction forward` 会被映射为 `On` | 语义层枚举到二值映射（`semantic/mod.rs`） | 阶段 2 支持多值状态 |

**实用性**：尽管有限制，阶段 1 仍有价值——新设备类型可以在 DSL 中声明，设备库 TOML 可以编写，示例代码可以解析，为阶段 2 提供测试用例。

---

## 4. `motor` 破坏性迁移策略（不做向后兼容）

用户已明确：如果兼容策略仅用于兼容旧写法且无实质收益，则不引入兼容层，直接废弃旧语法。

**策略调整**：

1. **不实现** `normalize_motor_compat` 之类的 AST 改写逻辑。
2. 旧写法直接判错（breaking change）：
   - 状态引用：`motor_x.on` / `motor_x.off`
   - 动作写法：`action: set motor_x on` / `off`
3. 统一使用显式端口写法：
   - `motor_x.run.on` / `motor_x.run.off`
   - `action: set motor_x.run on` / `off`
4. 在语义校验阶段新增专门错误提示，给出迁移建议（`self -> run`）。

**影响范围**：`semantic/mod.rs`（新增 legacy 写法拦截校验）、相关 fixture 与示例更新。

**预期结果**：规则更简单、行为更显式、避免隐式改写带来的维护和调试成本。

---

## 5. 设备库 TOML 文件

### 5.1 `devices/motor.toml`

```toml
[identity]
name = "通用电机"
type = "motor"

[semantics]
description = "通用电机，支持启停和正反转控制，无位置反馈。"
role = "actuator"
typical_applications = ["输送带驱动", "风机", "泵", "搅拌机"]

[[interfaces.ports]]
name = "run"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "运行指令"

[[interfaces.ports]]
name = "direction"
port_type = "digital"
states = ["forward", "reverse"]
default_state = "forward"
direction = "input"
description = "方向指令，切换前必须先停止"

[[interfaces.ports]]
name = "running"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "运行反馈，接触器辅助触点确认电机实际运行"

[[interfaces.ports]]
name = "fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "故障反馈，热继电器动作或断路器跳闸时置位"

[device_constraints]

[[device_constraints.safety]]
left = "direction.forward"
right = "direction.reverse"
relation = "conflicts_with"
reason = "正转和反转指令不能同时有效，否则产生相间短路（IEC 60947-4-1）"

[[device_constraints.safety]]
left = "run.on"
right = "fault.on"
relation = "conflicts_with"
reason = "电机故障时不得发运行指令（IEC 60947-4-1）"

[usage_guidance]
when_to_use = "不需要精确定位、只需要启停和方向控制的场景。"
when_not_to_use = "需要调速请使用 vfd；需要精确定位请使用 stepper_motor 或 servo_drive。"
```

### 5.2 `devices/stepper_motor.toml`

```toml
[identity]
name = "步进电机驱动器"
type = "stepper_motor"

[semantics]
description = "开环步进电机驱动器，脉冲+方向接口，支持使能控制和故障反馈。"
role = "actuator"
typical_applications = ["XY 平台定位", "输送带索引", "旋转分度台"]

[[interfaces.ports]]
name = "enable"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "使能信号，off 时电机失电，on 时电机锁定并响应脉冲"

[[interfaces.ports]]
name = "direction"
port_type = "digital"
states = ["forward", "reverse"]
default_state = "forward"
direction = "input"
description = "方向信号"

[[interfaces.ports]]
name = "pulse"
port_type = "digital"
states = ["active", "idle"]
default_state = "idle"
direction = "input"
description = "脉冲输入，active 时持续发送脉冲序列"

[[interfaces.ports]]
name = "fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "驱动器故障输出，过流/过压/过热时置位"

[device_constraints]

[[device_constraints.safety]]
left = "enable.off"
right = "pulse.active"
relation = "conflicts_with"
reason = "使能关闭时不得发送脉冲，否则可能导致失步或机械冲击"

[usage_guidance]
when_to_use = "低成本中等精度定位，负载惯量较小时使用。"
when_not_to_use = "高速高精度大惯量负载应使用 servo_drive。"
```

### 5.3 `devices/vfd.toml`

```toml
[identity]
name = "变频器"
type = "vfd"

[semantics]
description = "变频调速驱动器，支持正反转、模拟量调速和故障反馈。"
role = "actuator"
typical_applications = ["输送带调速", "风机水泵节能", "搅拌机变速"]

[[interfaces.ports]]
name = "run"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "运行指令"

[[interfaces.ports]]
name = "direction"
port_type = "digital"
states = ["forward", "reverse"]
default_state = "forward"
direction = "input"
description = "方向指令，切换前必须先停止"

[[interfaces.ports]]
name = "running"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "运行反馈，变频器实际输出时置位"

[[interfaces.ports]]
name = "fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "故障反馈，过流/过压/过热等故障时置位"

[[interfaces.ports]]
name = "freq_arrive"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "频率到达信号，输出频率到达设定值时置位"

[device_constraints]

[[device_constraints.safety]]
left = "direction.forward"
right = "direction.reverse"
relation = "conflicts_with"
reason = "正反转指令互斥（IEC 61800-5-1 §4.3.5）"

[[device_constraints.safety]]
left = "run.on"
right = "fault.on"
relation = "conflicts_with"
reason = "变频器故障时不得发运行指令"

[usage_guidance]
when_to_use = "需要调速控制、软启动、节能运行的感应电机场景。"
when_not_to_use = "需要精确定位请使用 servo_drive；简单启停无需调速可使用 motor。"
```

### 5.4 `devices/servo_drive.toml`

```toml
[identity]
name = "伺服驱动器"
type = "servo_drive"

[semantics]
description = "闭环伺服驱动器，脉冲+方向接口，内置编码器反馈，支持到位信号和故障保护。"
role = "actuator"
typical_applications = ["机械手定位", "数控机床进给轴", "贴片机 XY 平台"]

[[interfaces.ports]]
name = "enable"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "伺服使能"

[[interfaces.ports]]
name = "direction"
port_type = "digital"
states = ["forward", "reverse"]
default_state = "forward"
direction = "input"
description = "方向信号"

[[interfaces.ports]]
name = "pulse"
port_type = "digital"
states = ["active", "idle"]
default_state = "idle"
direction = "input"
description = "脉冲输入"

[[interfaces.ports]]
name = "clear_fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "故障复位，上升沿触发"

[[interfaces.ports]]
name = "ready"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "伺服就绪，使能后自检完成时置位"

[[interfaces.ports]]
name = "in_position"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "到位信号，位置偏差在定位窗口内时置位"

[[interfaces.ports]]
name = "fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "驱动器故障，过流/编码器断线/跟随误差超限时置位"

[[interfaces.ports]]
name = "zero_speed"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "零速信号，转速低于零速阈值时置位"

[device_constraints]

[[device_constraints.safety]]
left = "enable.off"
right = "pulse.active"
relation = "conflicts_with"
reason = "伺服未使能时不得发送脉冲"

[[device_constraints.safety]]
left = "fault.on"
right = "enable.on"
relation = "conflicts_with"
reason = "驱动器故障时不得使能（IEC 61800-5-1 §4.3）"

[[device_constraints.safety]]
left = "direction.forward"
right = "direction.reverse"
relation = "conflicts_with"
reason = "方向信号互斥"

[usage_guidance]
when_to_use = "需要高精度定位、闭环控制、快速响应的场景。"
when_not_to_use = "简单启停用 motor；调速不定位用 vfd；低成本中等精度用 stepper_motor。"
```

---

## 6. 阶段 2 接口契约（多端口语义闭环）

本节只描述阶段 2 需要满足的接口契约，不展开实现细节。实现者可以参考此契约设计重构方案。

### 6.1 runtime_bridge 需要满足的契约

`resolve_digital_output_id` 需要接受 `port` 参数，能够区分同一设备的不同端口：

```rust
// 改前
fn resolve_digital_output_id(&self, state_name: &str, device: &str)
    -> Result<DigitalOutputId, BridgeError>

// 改后（契约）
fn resolve_digital_output_id(&self, state_name: &str, device: &str, port: &str)
    -> Result<DigitalOutputId, BridgeError>
```

调用点（`convert_action`，`runtime_bridge.rs:678`）需要传入 `port`：

```rust
TransitionAction::Set { target, port, value } => {
    let id = resolver.resolve_digital_output_id(state_name, target, port)?;
    // ...
}
```

### 6.2 safety 验证引擎需要满足的契约

`device_index` 的键需要包含 `port`，能够区分同一设备的不同端口状态：

```rust
// 改前
device_index: HashMap<String, usize>  // 键是设备名

// 改后（契约）
device_index: HashMap<(String, String), usize>  // 键是 (设备名, 端口名)
```

`action_effect` 需要返回三元组，包含 `port`：

```rust
// 改前（safety.rs:837）
fn action_effect(action: &TransitionAction) -> Option<(&str, &str)>
// 返回 (device, state)

// 改后（契约）
fn action_effect(action: &TransitionAction) -> Option<(&str, &str, &str)>
// 返回 (device, port, state)
```

### 6.3 阶段 2 的前提条件

阶段 2 依赖阶段 1 完成，原因：
- 阶段 1 建立了新设备类型的端口定义（`implicit_ports_for_type`）
- 阶段 2 的路由重构需要知道每个端口对应的物理通道，这依赖设备库 TOML 的端口定义

---

## 7. 实施顺序

```
阶段 0（前置，无新类型）
  ├── 前置能力 A：set 支持枚举状态
  │     plc.pest + parser/mod.rs + ast/mod.rs + semantic/mod.rs
  │     （4 个文件，原子提交）
  └── 前置能力 B：attribute_name 白名单扩展 + extra_params
        plc.pest + parser/mod.rs + ast/mod.rs
        （3 个文件，可单独提交）

阶段 1（新设备类型关键字，依赖阶段 0）
  ├── motor 正反转改造（破坏性迁移，不保留旧写法）
  │     devices/motor.toml + 15 处代码改动 + legacy 语义拦截
  ├── stepper_motor
  │     devices/stepper_motor.toml + 15 处代码改动
  ├── vfd
  │     devices/vfd.toml + 15 处代码改动
  └── servo_drive
        devices/servo_drive.toml + 15 处代码改动
  （四个子任务相互独立，可分批提交）

阶段 2（多端口语义闭环，依赖阶段 1）
  ├── runtime_bridge 重构：resolve_digital_output_id 接受 port 参数
  └── safety 验证引擎重构：device_index 键改为 (device, port)
  （独立大改动，本文档不展开）

阶段 3（参数语义，依赖阶段 2）
  └── extra_params 类型化解析 + 设备库参数名校验逻辑
```

---

## 8. 验收测试矩阵

每个阶段合并前必须通过以下测试：

### 阶段 0

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 枚举状态解析 | `action: set x.direction forward` 能解析为 AST | 新增 fixture |
| 枚举状态映射 | `forward` → `BinaryValue::On`，`reverse` → `BinaryValue::Off` | 单元测试 `action_to_transition_action` |
| 非法枚举值报错 | `action: set x.foo bar` 报语义错误 | 新增 error fixture |
| 新参数解析通过 | `device x: motor { steps_per_rev: 200 }` 不报错 | 新增 fixture |
| 现有测试不回归 | 所有现有 `.plc` 示例仍可编译 | `cargo test --test examples_integration` |

### 阶段 1

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 新类型解析 | `device x: stepper_motor { ... }` 能解析 | 新增 fixture |
| 设备库约束注入 | `enable.off conflicts_with pulse.active` 自动注入 | 单元测试 `inject_device_constraints` |
| 拓扑门通过 | `relation { from: stepper_x.fault, to: plc_main.X0, via: reports_to }` 合法 | 集成测试 |
| motor 旧写法失效 | `motor.on` / `motor.off` 触发语义错误并给迁移提示 | 新增 error fixture 断言报错文案 |
| 示例可编译 | `cargo run -- examples/stepper_single_axis.plc --no-print-ir` 无错误 | CI |

### 阶段 2

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 多端口路由 | `stepper_x.enable` 和 `stepper_x.direction` 路由到不同物理通道 | 单元测试 `resolve_digital_output_id` |
| 多端口安全验证 | `enable.off conflicts_with pulse.active` 在 safety 引擎中正确验证 | 新增 verification 测试 |

---

## 9. Codex 审查问题修正记录（v2.5）

| 版本 | 问题 | 严重度 | 修正方案 |
|---|---|---|---|
| v1.0 | `set xxx.direction forward` 无法解析 | 🔴 | 前置能力 A：`state_value` 规则 + 语义层映射 |
| v1.0 | 改动清单不完整（漏 `parse_device_type`、`implicit_ports_for_type`、`device_type_label`） | 🔴 | 改动清单扩展至 12 处 |
| v1.0 | 设备库 TOML 字段名 `type` 应为 `port_type` | 🟠 | 所有 TOML 示例修正 |
| v1.0 | `alarm` 与 `fault` 命名冲突 | 🟠 | 统一为 `fault` |
| v2.0 | runtime_bridge 枚举值映射破坏方向语义 | 🔴 | IR 层保持 `BinaryValue`，映射在 `semantic/mod.rs` 完成 |
| v2.0 | `validate_state_reference` 签名返回 `()`，不能 `return Ok(...)` | 🔴 | 不在该函数做兼容映射，兼容策略统一取消 |
| v2.0 | 属性策略矛盾（当前代码对未知属性报错） | 🟠 | 改用方案 B（扩展白名单） |
| v2.0 | 改动清单写"10 处"但列了 12 项；函数名 `default_port_names` 不存在 | 🟠 | 修正为 12 处；函数名改为 `implicit_port_ids_for_device_type` |
| v2.1 | 前置能力 A 内部矛盾（`action_effect` 改 String 与"verification 无需改"同时出现） | 🟠 | 明确：`TransitionAction::Set.value` 保持 `BinaryValue`，`safety.rs` 和 `runtime_bridge.rs` 无需改 |
| v2.1 | 多端口语义未闭环（runtime_bridge 和 safety 都忽略 port） | 🔴 | 拆分为独立的阶段 2，明确接口契约；阶段 1 标注限制 |
| v2.1 | `parse_state_reference` 兼容方案需要 `device_types` 参数但调用链无上下文 | 🟠 | v2.5 决策为不做兼容，直接将旧写法定义为错误 |
| v2.1 | 参数处理写"暂存到 extra_attrs"但该字段不存在，空分支等于参数被吃掉 | 🟡 | 落 `extra_params: HashMap<String, String>`，阶段 3 再做类型化校验 |
| v2.1 | 版本号不一致 | 🟡 | 统一版本号 |
| v2.2 | 改动清单漏 `semantic/mod.rs:479`、`:2344`、`:3185` 三个 exhaustive match | 🔴 | 改动清单扩展至 15 处 |
| v2.2 | motor 兼容映射放在"验证调用点"不覆盖 lowering 路径 | 🟠 | v2.5 决策为取消映射，改为语义阶段直接报错 |
| v2.2 | 参数空分支让拼写错误静默通过 | 🟠 | 落 `extra_params` 存储参数，不是空分支 |
| v2.3 | `action_to_transition_action` 示例用了 `return Err` 和 `line`，但函数签名是 `-> TransitionAction` | 🔴 | 改为前置语义校验 pass（`validate_set_enum_values`）拦截非法值；`action_to_transition_action` 仅处理合法枚举 |
| v2.5 | 用户决策：兼容旧 motor 仅增加复杂度、收益不足 | 🟠 | 删除兼容改写方案，采用破坏性迁移并补充迁移提示 |

---

**文档状态**：v2.5 完整版，已根据 Codex 审查与用户迁移策略决策更新。
