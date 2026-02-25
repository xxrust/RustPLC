# RustPLC 设备库设计方案

**版本**: 3.1 Final
**日期**: 2026-02-24
**核心变更**: 引入统一端口状态模型，所有设备都有端口，两段引用是三段引用的语法糖

---

## 1. 核心设计理念

### 1.1 统一端口模型

**所有设备都有端口，没有例外。**

- 显式端口设备（如双线圈阀）：端口在设备库中显式定义（`coil_A`, `coil_B`）
- 隐式端口设备（如气缸）：有一个隐式的 `"self"` 端口，承载设备整体状态

```
valve_A: solenoid_valve
  ├── port "coil_A" → states: ["on", "off"]
  ├── port "coil_B" → states: ["on", "off"]
  └── port "out" → states: ["pressurized", "vented"]

cyl_A: cylinder
  └── port "self" → states: ["extended", "retracted"]
```

### 1.2 两段引用是语法糖

**DSL 语法**：

```
cyl_A.extended           # 两段引用（语法糖）
valve_A.coil_A.on        # 三段引用（完整形式）
```

**内部表示**（解析后统一为三元组）：

```rust
StateReference {
    device: "cyl_A",
    port: "self",        // 两段引用自动填充为 "self"
    state: "extended",
}

StateReference {
    device: "valve_A",
    port: "coil_A",      // 三段引用直接映射
    state: "on",
}
```

### 1.3 数据结构统一

**device_index 键类型**：`(String, String)` — 没有 `Option`，没有特殊分支

```rust
device_index: HashMap<(String, String), usize>
// 例：
//   ("cyl_A", "self") → 0
//   ("valve_A", "coil_A") → 1
//   ("valve_A", "coil_B") → 2
```

**DeviceDomain 结构**：

```rust
struct DeviceDomain {
    device_name: String,
    port_name: String,      // 不是 Option，统一为 String
    states: Vec<String>,
    default_state: usize,
    is_analog: bool,
    region_bounds: Option<Vec<(f64, f64)>>,
}
```

---

## 2. 两层架构

```
第一层：设备库（devices/*.toml）
  ├── 面向对象：AI Agent + 编译器预处理器
  ├── 内容：设备语义、端口定义、使用模式、设备级约束
  └── 生命周期：独立于具体项目，可跨项目复用

第二层：.plc 文件（[topology] / [constraints] / [tasks]）
  ├── 面向对象：编译器 + 验证引擎
  ├── 内容：系统拓扑、系统级约束、控制逻辑
  └── 生命周期：属于具体项目
```

| 维度 | 设备级约束 | 系统级约束 |
|---|---|---|
| 来源 | 制造商规格 / 物理特性 | 工艺逻辑 / 安全规范 |
| 适用范围 | 任何使用该设备的系统 | 当前具体系统 |
| 引用层级 | 端口级（`device.port.state`） | 设备级或端口级 |
| 执行机制 | 预处理注入 ConstraintSet | 直接写入 .plc [constraints] |
| 验证报告来源标注 | `source: "device:<type>"` | `source: "system"` |

---

## 3. 关键设计决策

### 3.1 查找键规则

查找键统一为 `identity.type`，即 DSL 设备类型关键字。编译器加载时建立 `HashMap<String, DeviceDef>`。

### 3.2 错误处理三级策略

| 情况 | 处理方式 | 理由 |
|---|---|---|
| `devices/` 目录不存在 | warning，返回空库 | 允许无设备库运行 |
| 目录存在，某设备类型无定义 | warning，跳过该设备 | 允许增量建库 |
| 设备库文件 TOML 格式错误 | **error，终止编译** | 格式错误不能静默跳过 |

### 3.3 解析器规则

**pest 语法**：

```pest
state_reference = @{ identifier ~ "." ~ identifier ~ ("." ~ identifier)? }
```

**解析逻辑**：

```rust
fn parse_state_reference(pair: Pair<Rule>) -> Result<StateReference, PlcError> {
    let raw = pair.as_str();
    let parts: Vec<&str> = raw.split('.').collect();
    match parts.len() {
        2 => Ok(StateReference {
            device: parts[0].to_string(),
            port: "self".to_string(),      // 两段引用自动填充 "self"
            state: parts[1].to_string(),
        }),
        3 => Ok(StateReference {
            device: parts[0].to_string(),
            port: parts[1].to_string(),
            state: parts[2].to_string(),
        }),
        _ => Err(PlcError::parse(...)),
    }
}
```

### 3.4 设备域建立规则

**无显式端口的设备**（Cylinder、Motor 等）：

```rust
// 设备库中 interfaces.ports 为空
// → 创建一个 "self" 端口的 DeviceDomain
DeviceDomain {
    device_name: "cyl_A",
    port_name: "self",
    states: vec!["extended", "retracted"],
    default_state: 1,  // "retracted"
    ...
}
```

**有显式端口的设备**（SolenoidValve、Plc 等）：

```rust
// 设备库中 interfaces.ports = [coil_A, coil_B, out]
// → 为每个端口创建一个 DeviceDomain
DeviceDomain { device_name: "valve_A", port_name: "coil_A", states: ["on", "off"], ... }
DeviceDomain { device_name: "valve_A", port_name: "coil_B", states: ["on", "off"], ... }
DeviceDomain { device_name: "valve_A", port_name: "out", states: ["pressurized", "vented"], ... }
```

### 3.5 action 驱动端口状态

**ActionStatement 结构**：

```rust
pub struct ActionTarget {
    pub device: String,
    pub port: String,      // 不是 Option，统一为 String
}

pub enum ActionStatement {
    Extend { target: ActionTarget },
    Retract { target: ActionTarget },
    Set { target: ActionTarget, value: BinaryValue },
    SetAnalog { target: ActionTarget, value: f64 },
    Log { message: String },
}
```

**action 语法**：

```
action: extend cyl_A              # 两段 → 解析为 ActionTarget { device: "cyl_A", port: "self" }
action: set valve_A.coil_A on     # target 为两段 device.port，on/off 是值 → ActionTarget { device: "valve_A", port: "coil_A" }
```

**action_effect 映射**：

```rust
fn action_effect(action: &TransitionAction) -> Option<(&str, &str, &str)> {
    match action {
        TransitionAction::Extend { target, port } =>
            Some((target, port, "extended")),
        TransitionAction::Set { target, port, value } => {
            let state = match value {
                BinaryValue::On => "on",
                BinaryValue::Off => "off",
            };
            Some((target, port, state))
        }
        // ...
    }
}
```

返回值：`(device, port, state)` 三元组，统一查找 `device_index[(device, port)]`。

### 3.6 设备库约束注入

预处理阶段（`preprocess_program`）调用 `inject_device_constraints`：

```rust
fn inject_device_constraints(
    program: &mut PlcProgram,
    library: &DeviceLibrary,
) -> Result<(), Vec<PlcError>> {
    for device in &program.topology.devices {
        let type_key = device_type_str(&device.device_type);
        let Some(def) = library.get(type_key) else { continue; };

        for constraint in &def.device_constraints.safety {
            // "coil_A.on" → StateReference { device: "valve_A", port: "coil_A", state: "on" }
            let left = expand_port_state_ref(&constraint.left, &device.name)?;
            let right = expand_port_state_ref(&constraint.right, &device.name)?;

            program.constraints.safety.push(SafetyConstraint {
                line: 0,
                left: SafetyOperand::State(left),
                relation: map_safety_relation(&constraint.relation),
                right: SafetyOperand::State(right),
                reason: Some(format!("[device:{}] {}", type_key, constraint.reason)),
                source: Some(format!("device:{}", type_key)),  // 新增：直接设置 source
            });
        }
    }
    Ok(())
}

fn expand_port_state_ref(
    port_state: &str,   // "coil_A.on"
    instance: &str,     // "valve_A"
) -> Result<StateReference, PlcError> {
    let (port, state) = port_state.split_once('.')
        .ok_or_else(|| PlcError::device_library_invalid_port_ref(port_state, instance))?;
    Ok(StateReference {
        device: instance.to_string(),
        port: port.to_string(),
        state: state.to_string(),
    })
}
```

**注意**：`SafetyConstraint`（AST 层）需要添加 `source: Option<String>` 字段，这样注入的约束就能携带来源信息。

`SafetyRule` 在 lowering 时直接传递 `source` 字段：

```rust
// src/semantic/mod.rs build_constraint_set_from_ast 中
constraint_set.safety.push(SafetyRule {
    left: map_safety_operand(&safety.left),
    relation: map_safety_relation(&safety.relation),
    right: map_safety_operand(&safety.right),
    reason: safety.reason.clone(),
    source: safety.source.clone(),  // 直接传递，不从 reason 提取
});
```

---

## 4. 设备库文件格式（TOML）

```toml
# ============================================================
# 设备库文件模板  v3.1
# 查找键：identity.type（必须与 DSL 设备类型关键字完全一致）
# ============================================================

[identity]
name = ""        # 人类可读名称
type = ""        # 查找键，e.g., "solenoid_valve"
category = ""    # 分类路径，仅用于组织
version = "1.0.0"

[semantics]
description = ""
primary_function = ""
role = ""                  # "actuator" | "sensor" | "controller"
typical_applications = []
key_features = []

[physical]
response_time = {}
operating_pressure = {}
power_supply = {}

# 端口定义
# 无端口设备（Cylinder 等）不填此段，编译器自动创建 "self" 端口
[[interfaces.ports]]
name = ""          # 端口名，用于三段引用 device.port.state
direction = ""     # "input" | "output" | "bidirectional"
type = ""          # "digital" | "analog" | "pneumatic" | "logical"
states = []        # 端口状态列表，e.g., ["on", "off"]
default_state = "" # 默认状态，e.g., "off"
description = ""

[[parameters]]
name = ""
type = ""          # "time" | "length" | "pressure" | "boolean" | "enum"
required = false
default = ""
unit = ""
options = []       # 仅 type = "enum" 时填写
description = ""

# 设备级约束
# 引用格式："<port>.<state>"（相对引用）
# 展开规则：device = instance, port = port, state = state
[device_constraints]

[[device_constraints.safety]]
left = ""       # e.g., "coil_A.on"
right = ""      # e.g., "coil_B.on"
relation = ""   # "conflicts_with" | "requires"
reason = ""     # 必填：引用规格书或物理原理

# timing 约束（第一期暂不实现）
# [[device_constraints.timing]]

[usage_guidance]
when_to_use = ""
when_not_to_use = ""

[[usage_guidance.common_patterns]]
name = ""
description = ""
example_code = """
"""

[[usage_guidance.common_mistakes]]
mistake = ""
consequence = ""
solution = ""

[metadata]
standards = []
alternatives = []
tags = []
```

---

## 5. 实践案例

### 案例 1：双线圈电磁阀

```toml
# devices/solenoid_valve.toml

[identity]
name = "双线圈五口两位电磁阀"
type = "solenoid_valve"
category = "actuators/pneumatic/valves"
version = "1.0.0"

[semantics]
description = "通过两个独立线圈控制气路方向，coil_A 通电伸出，coil_B 通电缩回，断电保持当前位置。"
primary_function = "direction_control"
role = "actuator"
typical_applications = ["双作用气缸驱动", "气路换向控制"]

[physical]
response_time = { typical = "15ms", max = "25ms" }
power_supply = { voltage = "24VDC", current_per_coil = "0.5A" }

[[interfaces.ports]]
name = "coil_A"
direction = "input"
type = "digital"
states = ["on", "off"]
default_state = "off"
description = "线圈 A，通电驱动气缸伸出方向"

[[interfaces.ports]]
name = "coil_B"
direction = "input"
type = "digital"
states = ["on", "off"]
default_state = "off"
description = "线圈 B，通电驱动气缸缩回方向"

[[interfaces.ports]]
name = "out"
direction = "output"
type = "pneumatic"
states = ["pressurized", "vented"]
default_state = "vented"
description = "气路输出口"

[[parameters]]
name = "response_time"
type = "time"
required = false
default = "20ms"
unit = "ms"
description = "线圈通电到气路切换完成的时间"

[device_constraints]

[[device_constraints.safety]]
left = "coil_A.on"
right = "coil_B.on"
relation = "conflicts_with"
reason = "双线圈同时通电产生短路，损坏线圈绕组（IEC 60947-5-1 §4.3）"

[usage_guidance]
when_to_use = "需要双向控制且断电保持位置时使用。"
when_not_to_use = "不要用于需要弹簧复位的单作用场景。"

[[usage_guidance.common_patterns]]
name = "驱动双作用气缸"
example_code = """
device valve_A: solenoid_valve { response_time: 20ms }
device cyl_A: cylinder { stroke_time: 300ms, retract_time: 300ms }

relation { from: plc_main.Y0, to: valve_A.coil_A, via: driven_by }
relation { from: plc_main.Y1, to: valve_A.coil_B, via: driven_by }
relation { from: valve_A.out, to: cyl_A.self, via: driven_by }

[tasks]
task extend:
    step go:
        action: set valve_A.coil_A on
        action: set valve_A.coil_B off
        wait: cyl_A.extended
"""
```

**预处理展开效果**：

```
device_index 注册：
  ("valve_A", "coil_A") → DeviceDomain { states: ["on", "off"], default: 1 }
  ("valve_A", "coil_B") → DeviceDomain { states: ["on", "off"], default: 1 }
  ("valve_A", "out")    → DeviceDomain { states: ["pressurized", "vented"], default: 1 }
  ("cyl_A",   "self")   → DeviceDomain { states: ["extended", "retracted"], default: 1 }

constraints 注入：
  safety: valve_A.coil_A.on conflicts_with valve_A.coil_B.on
      source: "device:solenoid_valve"
      reason: "[device:solenoid_valve] 双线圈同时通电产生短路..."
```

### 案例 2：感应式接近传感器

```toml
# devices/sensor.toml

[identity]
name = "感应式接近传感器"
type = "sensor"
category = "sensors/proximity/inductive"
version = "1.0.0"

[semantics]
description = "非接触检测金属物体的存在，输出数字信号。"
primary_function = "position_detection"
role = "sensor"
typical_applications = ["气缸行程末端检测", "金属工件到位检测"]

[physical]
response_time = { max = "2ms" }

[[interfaces.ports]]
name = "sense"
direction = "input"
type = "logical"
states = ["active", "inactive"]
default_state = "inactive"
description = "被检测物体的物理状态（由 relation detects 连接）"

[[interfaces.ports]]
name = "out"
direction = "output"
type = "digital"
states = ["on", "off"]
default_state = "off"
description = "检测到金属物体时输出 HIGH"

[[parameters]]
name = "subtype"
type = "enum"
options = ["limit_switch", "proximity_sensor", "push_button"]
required = false
description = "传感器子类型"

[[parameters]]
name = "debounce"
type = "time"
required = false
default = "0ms"
unit = "ms"
description = "消抖时间"

[device_constraints]
# 纯输出设备，无设备级安全约束

[usage_guidance]
when_to_use = "检测金属物体到位状态。"
when_not_to_use = "不能检测非金属物体，不能用于长距离检测（>8mm）。"

[[usage_guidance.common_patterns]]
name = "气缸行程末端检测"
example_code = """
device cyl_A: cylinder { stroke_time: 300ms }
device sensor_A_ext: sensor { subtype: "limit_switch" }

relation { from: cyl_A.self, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: plc_main.X0, via: reports_to }
"""
```

---

## 6. 底层改动清单

按依赖顺序排列，每层改动完成后才能进行下一层。

### 6.1 解析器层

**`src/parser/plc.pest`**

```pest
# 改前（第 10 行）
state_reference = @{ identifier ~ "." ~ identifier }
# 改后
state_reference = @{ identifier ~ "." ~ identifier ~ ("." ~ identifier)? }

# 改前（第 136-140 行）：action target 只接受 identifier
action_extend    = { "extend"    ~ identifier }
action_retract   = { "retract"   ~ identifier }
action_set_analog = { "set_analog" ~ identifier ~ number }
action_set       = { "set" ~ identifier ~ binary_output_value }

# 改后：action target 支持 identifier 或 identifier.identifier（两段）
action_target    = @{ identifier ~ ("." ~ identifier)? }
action_extend    = { "extend"    ~ action_target }
action_retract   = { "retract"   ~ action_target }
action_set_analog = { "set_analog" ~ action_target ~ number }
action_set       = { "set" ~ action_target ~ binary_output_value }
```

**`src/parser/mod.rs`**：

- `parse_state_reference`：两段引用填 `port = "self"`，三段引用直接映射
- `parse_action_target`（新增）：解析 `action_target` 规则，生成 `ActionTarget { device, port }`，单段填 `port = "self"`，两段直接映射

**`relation` endpoint 解析规则**：

`relation` 语句中的 `from` / `to` 字段是**设备端口引用**，格式为 `device.port`（两段），不是状态引用，不含 `state`。

```
relation { from: cyl_A.self, to: sensor_A_ext.sense, via: detects }
```

`relation` endpoint 格式为 `device.port`（两段），不含 `state`。对于隐式端口设备（如 cylinder），端口名固定为 `"self"`；对于显式端口设备（如 solenoid_valve），端口名为实际端口（如 `"coil_A"`、`"out"`）。

解析时使用独立的 `relation_endpoint` 规则，不复用 `state_reference`：

```pest
# relation endpoint：device.port（两段，不含 state）
relation_endpoint = @{ identifier ~ "." ~ identifier }
```

```rust
pub struct RelationEndpoint {
    pub device: String,
    pub port: String,   // 隐式端口设备填 "self"，显式端口设备填端口名
}
```

校验时检查 `port` 是否存在于该设备的端口列表（隐式端口设备只有 `"self"`）。`cyl_A.extended` 在 relation 中是非法的——`extended` 是 `self` 端口上的状态，不是端口名。

### 6.2 AST 层

**`src/ast/mod.rs`**

```rust
// StateReference：port 改为 String（不是 Option）
pub struct StateReference {
    pub device: String,
    pub port: String,    // 两段引用填 "self"，三段引用填端口名
    pub state: String,
}

// DevicePort：添加 states 和 default_state 字段
// 端口状态在预处理阶段从设备库写入，验证层直接从 AST 读取
pub struct DevicePort {
    pub id: String,
    pub port_type: PortType,
    pub role: PortRole,
    pub states: Vec<String>,          // 新增：端口状态列表，e.g., ["on", "off"]
    pub default_state: String,        // 新增：默认状态，e.g., "off"
}

// SafetyConstraint：添加 source 字段
pub struct SafetyConstraint {
    pub line: usize,
    pub left: SafetyOperand,
    pub relation: SafetyRelation,
    pub right: SafetyOperand,
    pub reason: Option<String>,
    pub source: Option<String>,       // 新增："system" | "device:solenoid_valve"
}

// 新增 ActionTarget
pub struct ActionTarget {
    pub device: String,
    pub port: String,    // 单段 action 填 "self"，两段直接映射
}

// ActionStatement 的 target 改为 ActionTarget
pub enum ActionStatement {
    Extend { target: ActionTarget },
    Retract { target: ActionTarget },
    Set { target: ActionTarget, value: BinaryValue },
    SetAnalog { target: ActionTarget, value: f64 },
    Log { message: String },
}
```

**`inject_device_constraints` 在预处理阶段同时写入端口状态到 `DevicePort`**：

```rust
// 预处理时，把设备库中的端口状态写入 topology 的 DevicePort
for port_def in &def.interfaces.ports {
    if let Some(port) = device.attributes.ports.iter_mut()
        .find(|p| p.id == port_def.name)
    {
        port.states = port_def.states.clone();
        port.default_state = port_def.default_state.clone();
    } else {
        // 设备库定义了端口但 .plc 中未声明：自动注册
        device.attributes.ports.push(DevicePort {
            id: port_def.name.clone(),
            port_type: map_port_type(&port_def.r#type),
            role: map_port_role(&port_def.direction),
            states: port_def.states.clone(),
            default_state: port_def.default_state.clone(),
        });
    }
}
```

这样 `collect_device_domains` 不需要 `library` 参数，直接从 `device.attributes.ports` 读取状态。

### 6.3 IR 层

**`src/ir/mod.rs`**

```rust
// StateExpr：port 改为 String
pub struct StateExpr {
    pub device: String,
    pub port: String,    // "self" 或端口名
    pub state: String,
}

// TransitionAction：target 添加 port
pub enum TransitionAction {
    Extend { target: String, port: String },
    Retract { target: String, port: String },
    Set { target: String, port: String, value: BinaryValue },
    SetAnalog { target: String, port: String, value_raw: String },
    Log { message: String },
}

// SafetyRule：添加 source 字段
pub struct SafetyRule {
    pub left: SafetyExpr,
    pub relation: SafetyRelation,
    pub right: SafetyExpr,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,  // "system" | "device:solenoid_valve"
}
```

### 6.4 语义层

**`src/semantic/mod.rs`**

| 函数 | 改动内容 |
|---|---|
| `map_safety_operand` | 传递 `port` 字段到 `StateExpr` |
| `action_to_transition_action` | 传递 `port` 字段到 `TransitionAction` |
| `collect_known_states` | key 改为 `(String, String)`，支持端口级状态 |
| `validate_state_reference` | 验证端口存在性（从 `device.attributes.ports` 查找） |
| `build_constraint_set_from_ast` | `SafetyRule` 构造时直接传递 `source: safety.source.clone()` |
| `preprocess_program` | 增加 `device_library: Option<&DeviceLibrary>` 参数 |

新增函数：

```rust
fn inject_device_constraints(program: &mut PlcProgram, library: &DeviceLibrary)
fn expand_port_state_ref(port_state: &str, instance: &str) -> Result<StateReference, PlcError>
```

### 6.5 验证层

**`src/verification/safety.rs`**

| 位置 | 改动内容 |
|---|---|
| `DeviceDomain`（第 105 行） | `port_name: String`（不是 Option） |
| `SafetyModel.device_index`（第 128 行） | 类型改为 `HashMap<(String, String), usize>` |
| `collect_device_domains`（第 639 行） | 无端口设备创建 `"self"` 域；有端口设备按端口创建；状态从 `device.attributes.ports` 读取，不需要 `library` 参数 |
| `safety_expr_states_with_reason`（第 1009 行） | 查找 key 改为 `(device, port)` |
| `transition_effects`（第 795 行） | action 查找 key 改为 `(target, port)` |
| `action_effect`（第 837 行） | 返回值改为 `(&str, &str, &str)` 三元组 |
| `SafetyRuleStatus` | 添加 `source: Option<String>` 字段 |

**`collect_device_domains` 核心逻辑**（不需要 `library` 参数）：

```rust
fn collect_device_domains(
    program: &PlcProgram,
    constraints: &ConstraintSet,
) -> (...) {
    for device in &program.topology.devices {
        if device.attributes.ports.is_empty() {
            // 无端口设备：创建 "self" 端口域，状态从 device_type 推断
            let states = default_states_for_type(&device.device_type);
            let default = default_state_index(&states, &device.device_type);
            device_index.insert((device.name.clone(), "self".to_string()), devices.len());
            devices.push(DeviceDomain {
                device_name: device.name.clone(),
                port_name: "self".to_string(),
                states,
                default_state: default,
                is_analog: false,
                region_bounds: None,
            });
        } else {
            // 有端口设备：每个端口一个域，状态从 DevicePort.states 读取
            for port in &device.attributes.ports {
                let states = port.states.clone();  // 预处理阶段已从设备库写入
                let default = states.iter()
                    .position(|s| s == &port.default_state)
                    .unwrap_or(0);
                device_index.insert(
                    (device.name.clone(), port.id.clone()),
                    devices.len()
                );
                devices.push(DeviceDomain {
                    device_name: device.name.clone(),
                    port_name: port.id.clone(),
                    states,
                    default_state: default,
                    is_analog: matches!(port.port_type, PortType::Analog),
                    region_bounds: None,
                });
            }
        }
    }
    // ensure_device_state：key 改为 (device, port)
    for rule in &constraints.safety {
        if let SafetyExpr::State(expr) = &rule.left {
            let key = (expr.device.clone(), expr.port.clone());
            if let Some(&id) = device_index.get(&key) {
                ensure_device_state(&mut devices[id], &expr.state);
            }
        }
        // right 同理
    }
}
```

### 6.6 新增模块

**`src/device_library.rs`**：

```rust
pub struct DeviceLibrary {
    defs: HashMap<String, DeviceDef>,
}

impl DeviceLibrary {
    pub fn empty() -> Self { Self { defs: HashMap::new() } }

    pub fn load(dir: impl AsRef<Path>) -> Result<Self, Vec<PlcError>> {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Ok(Self::empty());  // 目录不存在：空库
        }
        let mut defs = HashMap::new();
        let mut errors = Vec::new();
        for path in glob_toml_files(dir) {
            match toml::from_str::<DeviceDef>(&fs::read_to_string(&path)?) {
                Ok(def) => { defs.insert(def.identity.r#type.clone(), def); }
                Err(e) => { errors.push(PlcError::device_library_parse_error(&path, e)); }
            }
        }
        if !errors.is_empty() { return Err(errors); }  // 格式错误：终止
        Ok(Self { defs })
    }

    pub fn get(&self, type_key: &str) -> Option<&DeviceDef> {
        self.defs.get(type_key)
    }

    pub fn is_empty(&self) -> bool { self.defs.is_empty() }
}
```

**`src/main.rs`** 改动：

```rust
let plc_dir = plc_path.parent().unwrap_or(Path::new("."));
let device_library = match DeviceLibrary::load(plc_dir.join("devices")) {
    Ok(lib) => {
        if lib.is_empty() {
            eprintln!("WARNING: 未找到设备库，设备级约束不会被注入");
        }
        lib
    }
    Err(errors) => {
        for e in &errors { eprintln!("{e}"); }
        std::process::exit(1);  // 格式错误：终止，不降级
    }
};

let program = preprocess_program(&raw_program, Some(&device_library))?;
let constraints = build_constraint_set(&program)?;  // 签名不变
```

---

## 7. source 链路完整追踪

```
设备库 TOML
  device_constraints.safety[i].reason = "双线圈同时通电..."
    ↓ inject_device_constraints（预处理）
AST SafetyConstraint
  .reason = "[device:solenoid_valve] 双线圈同时通电..."
  .source = Some("device:solenoid_valve")   ← 直接设置，不从 reason 提取
    ↓ build_constraint_set_from_ast（lowering）
IR SafetyRule
  .reason = Some("[device:solenoid_valve] 双线圈同时通电...")
  .source = Some("device:solenoid_valve")   ← 直接传递 safety.source.clone()
    ↓ verify_safety
SafetyRuleStatus
  .source = Some("device:solenoid_valve")   ← 直接传递
    ↓ 验证报告输出
```

系统级约束（来自 .plc [constraints]）：

```
.plc [constraints]
  safety: cyl_A.extended conflicts_with cyl_B.extended
    ↓ build_constraint_set_from_ast
AST SafetyConstraint
  .source = None   ← 用户写的约束不设置 source
    ↓ lowering
IR SafetyRule
  .source = None   ← 传递 None
    ↓ 验证报告输出：来源显示 "system"（None 时的默认显示）
```

---

## 8. 验证报告中的来源区分

```
[safety] PASS  valve_A.coil_A.on conflicts_with valve_A.coil_B.on
         来源: device:solenoid_valve
         原因: [device:solenoid_valve] 双线圈同时通电产生短路

[safety] PASS  cyl_A.extended conflicts_with cyl_B.extended
         来源: system
         原因: A缸和B缸不能同时伸出
```

违反时：

```
ERROR [safety] 安全约束违反
  约束: valve_A.coil_A.on conflicts_with valve_A.coil_B.on
  来源: device:solenoid_valve（设备固有约束）
  建议: 此约束来自设备库，反映设备物理限制，请修改控制逻辑而非删除约束
```

---

## 9. 改动规模总结

| 层级 | 文件 | 改动内容 |
|---|---|---|
| 解析器 | `plc.pest` | `state_reference` + `action_target` + `relation_endpoint` 规则 |
| 解析器 | `parser/mod.rs` | `parse_state_reference` + `parse_action_target` + `parse_relation_endpoint` |
| AST | `ast/mod.rs` | `StateReference`、`DevicePort`、`SafetyConstraint`、`ActionTarget`、`ActionStatement` |
| IR | `ir/mod.rs` | `StateExpr`、`TransitionAction`、`SafetyRule` |
| 语义 | `semantic/mod.rs` | 6 个函数修改，2 个函数新增 |
| 验证 | `verification/safety.rs` | `DeviceDomain`、`device_index`、5 个函数 |
| 新增 | `device_library.rs` | 新模块 |
| 入口 | `main.rs` | 加载设备库，传入预处理 |

**核心原则**：
- `port = "self"` 统一无端口设备，端口字段不再用 `Option`（注：`SafetyConstraint.source` 等元数据字段仍为 `Option<String>`）
- `source` 在 AST 层设置，lowering 直接传递，不从字符串提取
- 端口状态在预处理阶段写入 `DevicePort`，验证层只读 AST，不需要 `library` 参数
- `action_target` 规则与 `state_reference` 规则对称，都支持两段形式

---

## 10. 字段优先级

| 优先级 | 字段 | 目的 |
|---|---|---|
| **1. 必需** | `identity.type`、`semantics.description` | 编译器查找键；AI 理解设备结构 |
| **1. 必需（显式端口设备）** | `interfaces.ports`（含 `states`、`default_state`） | 端口状态域建立；隐式端口设备可省略，编译器自动补全 `"self"` 端口 |
| **2. 高** | `device_constraints.safety` | 设备固有安全约束，预处理注入，强制验证 |
| **3. 高** | `usage_guidance.common_patterns` | AI 生成 .plc 的直接参考 |
| **4. 中** | `parameters`、`physical.response_time` | 支持可配置设备，提供时序建模数据 |
| **5. 低** | `metadata`、`usage_guidance.common_mistakes` | 辅助信息 |
