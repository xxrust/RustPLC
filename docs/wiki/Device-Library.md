# 设备库（Device Library）

设备库为 RustPLC 引入统一端口模型和设备级安全约束自动注入。设备的物理特性（端口、状态、互锁）定义一次，所有使用该设备的程序自动继承。

---

## 核心概念

### 统一端口模型

所有设备都有端口：

- **显式端口设备**（如双线圈电磁阀）：端口在 `devices/*.toml` 中定义（`coil_A`, `coil_B`）
- **隐式端口设备**（如气缸）：有一个隐式 `"self"` 端口，承载设备整体状态

### 三段引用

DSL 支持两种引用形式，解析后统一为三元组：

```
cyl_A.extended           → device="cyl_A", port="self", state="extended"
valve_A.coil_A.on        → device="valve_A", port="coil_A", state="on"
```

两段引用是三段引用的语法糖，解析器自动填充 `port: "self"`。

### 两层约束架构

| 维度 | 设备级约束（devices/*.toml） | 系统级约束（.plc [constraints]） |
|---|---|---|
| 来源 | 制造商规格 / 物理特性 | 工艺逻辑 / 安全规范 |
| 适用范围 | 任何使用该设备的系统 | 当前具体系统 |
| 执行机制 | 预处理自动注入 | 工程师手写 |

设备级约束在编译期自动展开为 AST `SafetyConstraint`，注入 `program.constraints.safety`，标注 `source: "device:<type>"` 便于追溯。

---

## 设备定义格式

设备库文件放在 `devices/` 目录，每个 `.toml` 文件定义一种设备类型。

### 双线圈电磁阀

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

### 气缸（隐式端口）

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

### 字段说明

| 字段 | 说明 |
|---|---|
| `identity.type` | 设备类型键（匹配 DSL 中的设备类型声明） |
| `interfaces.ports[].name` | 端口名称 |
| `interfaces.ports[].states` | 端口状态域 |
| `interfaces.ports[].default_state` | 默认状态 |
| `device_constraints.safety[].relation` | `conflicts_with` 或 `requires` |
| `device_constraints.safety[].reason` | 约束原因（出现在诊断信息中） |

---

## DSL 中使用

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

现有 `.plc` 文件零改动 — 两段引用自动填充 `port: "self"`，`devices/` 目录不存在时返回空库。

---

## 编译流程

1. 编译器从 `devices/` 加载所有 `.toml` 文件
2. 对每个 `.plc` 中声明的设备，按类型查找设备库定义
3. 将设备库中的端口状态写入设备属性
4. 将设备级安全约束展开为 AST 约束，注入编译流水线
5. 注入的约束标注来源，便于诊断追溯

---

## 扩展设备库

添加新设备类型：

1. 在 `devices/` 下创建 `.toml` 文件
2. 定义 `identity`、`interfaces.ports`、`device_constraints`
3. 在 `.plc` 中使用 `device my_device: <type>` 引用
4. 编译器自动加载并注入约束

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/device_library.rs` | 设备库加载器 |
| `src/parser/plc.pest` | 三段引用语法规则 |
| `src/semantic/mod.rs` | 约束注入逻辑 |
| `devices/*.toml` | 设备库定义 |
