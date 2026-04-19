# Device Library

设备库（Device Library）为 RustPLC 引入统一端口模型和设备级安全约束注入机制。

---

## 核心概念

### 统一端口模型

所有设备都有端口，没有例外：

- **显式端口设备**（如双线圈电磁阀）：端口在 `devices/*.toml` 中定义（`coil_A`, `coil_B`）
- **隐式端口设备**（如气缸）：有一个隐式的 `"self"` 端口，承载设备整体状态

### 三段引用

DSL 支持两种引用形式，解析后统一为三元组：

```
cyl_A.extended           # 两段引用 → device="cyl_A", port="self", state="extended"
valve_A.coil_A.on        # 三段引用 → device="valve_A", port="coil_A", state="on"
```

两段引用是三段引用的语法糖，解析器自动填充 `port: "self"`。

### 两层约束架构

| 维度 | 设备级约束（devices/*.toml） | 系统级约束（.plc [constraints]） |
|---|---|---|
| 来源 | 制造商规格 / 物理特性 | 工艺逻辑 / 安全规范 |
| 适用范围 | 任何使用该设备的系统 | 当前具体系统 |
| 引用层级 | 端口级（`port.state`） | 设备级或端口级 |
| 执行机制 | 预处理注入 ConstraintSet | 直接写入 .plc |

---

## 设备库文件格式

设备库文件放在 `devices/` 目录下，每个 `.toml` 文件定义一种设备类型。

### 示例：双线圈电磁阀

```toml
[identity]
name = "双线圈电磁阀"
type = "solenoid_valve"

[[interfaces.ports]]
name = "coil_A"
states = ["on", "off"]
default_state = "off"
direction = "output"
port_type = "digital"

[[interfaces.ports]]
name = "coil_B"
states = ["on", "off"]
default_state = "off"
direction = "output"
port_type = "digital"

[[device_constraints.safety]]
left = "coil_A.on"
right = "coil_B.on"
relation = "conflicts_with"
reason = "双线圈不能同时通电"
```

### 示例：气缸（隐式端口）

```toml
[identity]
name = "气缸"
type = "cylinder"

[[interfaces.ports]]
name = "self"
states = ["extended", "retracted"]
default_state = "retracted"
direction = "output"
port_type = "pneumatic"
```

### TOML 结构说明

| 字段 | 说明 |
|---|---|
| `identity.name` | 设备显示名称 |
| `identity.type` | 设备类型键（匹配 DSL 中的 `device_type`） |
| `interfaces.ports[]` | 端口定义列表 |
| `interfaces.ports[].name` | 端口名称 |
| `interfaces.ports[].states` | 端口状态域 |
| `interfaces.ports[].default_state` | 默认状态 |
| `device_constraints.safety[]` | 设备级安全约束 |
| `device_constraints.safety[].left` | 左操作数（`port.state` 格式） |
| `device_constraints.safety[].right` | 右操作数 |
| `device_constraints.safety[].relation` | `conflicts_with` 或 `requires` |
| `device_constraints.safety[].reason` | 约束原因说明 |

---

## 编译流程

1. 编译器从 `devices/` 目录加载所有 `.toml` 文件
2. 对每个 `.plc` 中声明的设备，按 `device_type` 查找设备库定义
3. 将设备库中的端口状态写入设备属性
4. 将设备库中的安全约束展开为 AST `SafetyConstraint`，注入 `program.constraints.safety`
5. 注入的约束标注 `source: "device:<type>"`，便于追溯

### 错误处理

| 场景 | 行为 |
|---|---|
| `devices/` 目录不存在 | 返回空库，正常编译 |
| TOML 格式错误 | 报错终止编译 |
| 设备类型无定义 | 跳过，不影响编译 |

---

## DSL 使用

### 三段引用在 wait/action/safety 中的使用

```plc
[constraints]
safety: valve_A.coil_A.on conflicts_with valve_A.coil_B.on

[tasks]
task cycle:
    step activate:
        action: set valve_A.coil_A on
        wait: valve_A.coil_A == on
```

### 向后兼容

现有 `.plc` 文件零改动——两段引用自动填充 `port: "self"`，`devices/` 目录不存在时返回空库。

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/device_library.rs` | 设备库加载器与 TOML 反序列化类型 |
| `src/ast/mod.rs` | `StateReference.port`、`ActionTarget`、`SafetyConstraint.source` |
| `src/ir/mod.rs` | `StateExpr.port`、`TransitionAction.port`、`SafetyRule.source` |
| `src/parser/plc.pest` | `state_reference` 三段规则、`action_target` 规则 |
| `src/semantic/mod.rs` | `inject_device_constraints`、`expand_port_state_ref` |
| `devices/*.toml` | 设备库定义文件 |
| `docs/已实现/device-library-design.md` | 完整设计方案（v3.1） |
