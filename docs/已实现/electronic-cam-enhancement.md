# RustPLC 电子凸轮能力增强方案

**版本**: 1.3
**日期**: 2026-02-25
**状态**: 设计阶段

---

## 1. 问题陈述

### 1.1 什么是电子凸轮

电子凸轮（Electronic Cam）是用软件定义的主从轴位置同步关系，替代传统机械凸轮。核心原理：每个伺服周期读取主轴位置，从凸轮表中查找（或计算）对应的从轴位置，并将位置指令发送给从轴驱动器。

与机械凸轮相比，电子凸轮的关键优势是运行时可即时切换凸轮曲线、调整相位偏移、缩放运动幅度，无需更换物理硬件。

**典型应用场景**：

| 场景 | 主轴 | 从轴 | 凸轮特征 |
|---|---|---|---|
| 飞剪（Flying Shear） | 输送带编码器 | 旋转刀伺服 | 驻停→加速→等速切割→减速→返回 |
| 印刷套色 | 料卷编码器 | 印刷辊伺服 | 1:1 比例 + 相位修正 |
| 旋转灌装 | 转台编码器 | 灌装阀伺服 | 凸轮控制阀门开闭时序 |
| 包装封切 | 薄膜送料编码器 | 封切刀伺服 | 同步追踪 + 定长切割 |
| 装配机 | 虚拟主轴 | 多轴取放机构 | 多从轴协调运动 |

### 1.2 当前系统的计算能力缺口

| 问题 | 严重度 | 代码位置 | 说明 |
|---|---|---|---|
| 无变量/寄存器存储 | 🔴 高 | `runtime-core/src/lib.rs:192-197` | Runtime 仅有 `loc` + `pid_states`，无通用内存模型 |
| 无算术表达式 | 🔴 高 | `plc.pest:168-175` | `condition_value` 只接受字面量，不支持 `+`/`-`/`*`/`/` |
| 无查找表/插值 | 🔴 高 | — | 无 `cam_table` 类型，无插值函数 |
| 无数学函数 | 🔴 高 | — | 无 `sin`/`cos`/`sqrt`/`abs`/`fmod` |
| `set_analog` 只接受字面量 | 🔴 高 | `plc.pest:155` | `action_set_analog` 规则为 `"set_analog" ~ action_target ~ number` |
| 条件不支持表达式 | 🟡 中 | `plc.pest:178-180` | `simple_condition` 只能 `operand op literal`，不能 `expr op expr` |
| 无数组/集合类型 | 🟡 中 | `ast/mod.rs` | 无法存储凸轮表数据点 |
| 运行时无状态 | 🔴 高 | `runtime-core/src/lib.rs:150-166` | `Program` 只有 `tasks` + `pid_loops`，无变量存储 |

**根本矛盾**：电子凸轮的核心是"每个 tick 读主轴位置 → 查表插值 → 计算从轴指令"，而当前 RustPLC 的 DSL 和运行时完全不具备数值计算能力。

### 1.3 电子凸轮的核心计算需求

```
主轴编码器 ──→ 读取 master_pos
                    │
                    ▼
              gear_ratio × master_pos + phase_offset
                    │
                    ▼
              fmod(adjusted_pos, table_period)  ← 周期凸轮取模（周期从表的 master[last]-master[0] 自动推导）
                    │
                    ▼
              cam_table 查找 + 插值              ← 二分查找 + 三次样条
                    │
                    ▼
              slave_cmd_pos                      ← 从轴位置指令
                    │
                    ├──→ 速度前馈 = d(slave)/d(master) × master_vel
                    ├──→ 跟随误差 = |slave_cmd - slave_actual|
                    └──→ 写入从轴驱动器
```

**计算原语需求清单**：

| 原语 | 用途 | 实时性要求 |
|---|---|---|
| 变量读写 | 存储 master_pos、slave_pos、中间结果 | 每 tick |
| 四则运算 | gear_ratio 缩放、phase_offset 偏移 | 每 tick |
| 取模运算 | 周期凸轮的 master_pos 归一化 | 每 tick |
| 二分查找 | 在凸轮表中定位区间 | 每 tick，O(log n) |
| 多项式求值 | 三次样条插值 `a + b·dx + c·dx² + d·dx³` | 每 tick |
| 微分求值 | 速度/加速度前馈 | 每 tick |
| 比较与分支 | 跟随误差超限检测 | 每 tick |
| `abs()` / `clamp()` | 误差计算、输出限幅 | 每 tick |
| `sin()` / `cos()` | 修正正弦等标准运动规律 | 编译期（曲线生成） |

### 1.4 分阶段策略

| 阶段 | 内容 | 依赖 | 产出 |
|---|---|---|---|
| 阶段 0 | 表达式引擎基础：变量、算术、数学函数 | 无 | DSL 可声明变量并做数值计算 |
| 阶段 1 | 查找表与插值原语：表声明、线性/样条插值 | 阶段 0 | 可定义凸轮表并在运行时插值 |
| 阶段 2 | 凸轮配置系统：`cam_coupling` 设备类型、凸轮动作 | 阶段 1 | DSL 可声明凸轮耦合关系 |
| 阶段 3 | 凸轮运行时集成：tick 循环求值、跟随误差、凸轮切换 | 阶段 2 | 运行时可执行凸轮同步 |
| 阶段 4 | 凸轮验证：安全约束、时序验证、静态分析 | 阶段 2 | 编译期验证凸轮安全性 |

阶段 3 和阶段 4 相互独立，可并行开发。

---

## 2. 阶段 0：表达式引擎基础

### 2.1 变量声明与存储

**目标 DSL 语法**：

```plc
[topology]
device encoder_main: sensor { detects: conveyor }
device servo_x: servo_drive { ... }

variable master_pos: float = 0.0
variable slave_pos: float = 0.0
variable slave_vel: float = 0.0
variable cycle_count: int = 0
variable cam_active: bool = false
```

**`src/parser/plc.pest` 改动**：

```pest
# 新增：变量类型
variable_type = { "float" | "int" | "bool" }

# 新增：变量声明
variable_declaration = {
    "variable" ~ identifier ~ ":" ~ variable_type ~ "=" ~ (number | integer | boolean_value)
}

# 修改：topology_entry 增加 variable_declaration
topology_entry = _{ device_declaration | relation_declaration | variable_declaration }
```

**`src/ast/mod.rs` 改动**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VariableType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDeclaration {
    pub line: usize,
    pub name: String,
    pub var_type: VariableType,
    pub initial_value: String,  // 原始字符串，语义层做类型校验
}

// TopologySection 增加字段
pub struct TopologySection {
    pub devices: Vec<DeviceDeclaration>,
    pub connections: Vec<TopologyConnection>,
    pub variables: Vec<VariableDeclaration>,  // 新增
}
```

**`src/ir/mod.rs` 改动**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariableDef {
    pub name: String,
    pub var_type: VariableType,
    pub initial_value: f32,  // 统一用 f32 存储（bool: 0.0/1.0, int: 整数值）
    pub index: u16,          // 运行时数组索引
}

// TopologyGraph 增加字段
pub struct TopologyGraph {
    pub graph: DiGraph<Device, ConnectionType>,
    pub pid_loops: Vec<PidLoop>,
    pub links: Vec<TopologyLink>,
    pub variables: Vec<VariableDef>,  // 新增
}
```

**`crates/runtime-core/src/lib.rs` 改动**：

```rust
pub const MAX_VARIABLES: usize = 64;

// Program 增加变量初始值
pub struct Program<'a> {
    pub tasks: &'a [Task<'a>],
    pub pid_loops: &'a [PidConfig],
    pub var_init: &'a [f32],  // 新增：变量初始值数组，长度 <= MAX_VARIABLES
}

// Runtime 增加变量存储
pub struct Runtime<'a> {
    program: &'a Program<'a>,
    loc: Location,
    step_entered_at: Option<Tick>,
    pid_states: [PidState; MAX_PID_LOOPS],
    variables: [f32; MAX_VARIABLES],  // 新增：运行时变量存储
}
```

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`ir/mod.rs`、`semantic/mod.rs`、`runtime-core/src/lib.rs`（6 个文件）

### 2.2 算术表达式

**目标 DSL 语法**：

```plc
[tasks]
task cam_control:
  step compute_slave:
    action: compute slave_pos = master_pos * gear_ratio + phase_offset
    action: compute adjusted = fmod(slave_pos, 360.0)
    action: compute error = abs(cmd_pos - actual_pos)
    action: set_analog servo_x slave_pos
```

**`src/parser/plc.pest` 改动 — 表达式语法**：

```pest
# 表达式原语
expr_literal = @{ number }
expr_variable = @{ identifier }
expr_func_call = { identifier ~ "(" ~ expression ~ ("," ~ expression)* ~ ")" }
expr_atom = { expr_func_call | expr_literal | expr_variable | "(" ~ expression ~ ")" }

# 一元运算
expr_unary = { "-" ~ expr_atom | expr_atom }

# 乘除（高优先级）
expr_mul_op = { "*" | "/" | "%" }
expr_mul = { expr_unary ~ (expr_mul_op ~ expr_unary)* }

# 加减（低优先级）
expr_add_op = { "+" | "-" }
expression = { expr_mul ~ (expr_add_op ~ expr_mul)* }

# 计算语句
compute_statement = { "compute" ~ identifier ~ "=" ~ expression }

# action_command 增加 compute
action_command = {
    action_extend
    | action_retract
    | action_set_analog_expr   // 新增：支持表达式
    | action_set_analog
    | action_set
    | action_log
    | compute_statement        // 新增
}

# set_analog 支持表达式
action_set_analog_expr = { "set_analog" ~ action_target ~ expression }
```

**`src/ast/mod.rs` 改动 — Expression 枚举**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    Literal(f32),
    Variable(String),
    BinaryOp {
        op: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    UnaryNeg(Box<Expression>),
    FunctionCall {
        name: String,
        args: Vec<Expression>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

// ActionStatement 增加变体
pub enum ActionStatement {
    // 现有...
    Compute { target: String, expr: Expression },       // 新增
    SetAnalogExpr { target: ActionTarget, expr: Expression }, // 新增
}
```

**`crates/runtime-core/src/lib.rs` 改动 — 表达式求值器**：

```rust
/// 编译期将 Expression AST 展平为后缀指令序列（栈式虚拟机）
/// 运行时求值无需递归，栈深度有界，no_std 兼容
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExprOp {
    PushLiteral(f32),
    PushVariable(u16),          // 变量索引
    PushAnalogInput(AnalogInputId),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    CallAbs,
    CallMin,
    CallMax,
    CallSin,
    CallCos,
    CallSqrt,
    CallPow,
    CallFmod,
    CallClamp,
}

pub const MAX_EXPR_OPS: usize = 32;
pub const MAX_EXPR_STACK: usize = 16;

/// 栈式表达式求值，确定性、无分配、有界
pub fn eval_expr(ops: &[ExprOp], vars: &[f32; MAX_VARIABLES], io: &impl Io) -> f32 {
    let mut stack = [0.0f32; MAX_EXPR_STACK];
    let mut sp: usize = 0;
    for op in ops {
        match *op {
            ExprOp::PushLiteral(v) => { stack[sp] = v; sp += 1; }
            ExprOp::PushVariable(idx) => { stack[sp] = vars[idx as usize]; sp += 1; }
            ExprOp::PushAnalogInput(id) => { stack[sp] = io.read_analog_input(id); sp += 1; }
            ExprOp::Add => { sp -= 1; stack[sp - 1] += stack[sp]; }
            ExprOp::Sub => { sp -= 1; stack[sp - 1] -= stack[sp]; }
            ExprOp::Mul => { sp -= 1; stack[sp - 1] *= stack[sp]; }
            ExprOp::Div => { sp -= 1; stack[sp - 1] /= stack[sp]; }
            ExprOp::Mod => { sp -= 1; stack[sp - 1] = fmod(stack[sp - 1], stack[sp]); }
            ExprOp::Neg => { stack[sp - 1] = -stack[sp - 1]; }
            ExprOp::CallAbs => { stack[sp - 1] = abs(stack[sp - 1]); }
            ExprOp::CallSin => { stack[sp - 1] = sin(stack[sp - 1]); }
            ExprOp::CallCos => { stack[sp - 1] = cos(stack[sp - 1]); }
            ExprOp::CallSqrt => { stack[sp - 1] = sqrt(stack[sp - 1]); }
            ExprOp::CallMin => { sp -= 1; stack[sp - 1] = min(stack[sp - 1], stack[sp]); }
            ExprOp::CallMax => { sp -= 1; stack[sp - 1] = max(stack[sp - 1], stack[sp]); }
            ExprOp::CallPow => { sp -= 1; stack[sp - 1] = pow(stack[sp - 1], stack[sp]); }
            ExprOp::CallFmod => { sp -= 1; stack[sp - 1] = fmod(stack[sp - 1], stack[sp]); }
            ExprOp::CallClamp => {
                sp -= 2;
                stack[sp - 1] = clamp(stack[sp - 1], stack[sp], stack[sp + 1]);
            }
        }
    }
    stack[0]
}
```

新增 `Instr` 变体：

```rust
pub enum Instr<'a> {
    // 现有变体...
    Compute {
        target_var: u16,           // 目标变量索引
        ops: &'a [ExprOp],        // 后缀表达式指令
        next: StepId,
    },
    SetAnalogExpr {
        id: AnalogOutputId,
        ops: &'a [ExprOp],
        next: StepId,
    },
}
```

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`ir/mod.rs`、`semantic/mod.rs`、`runtime-core/src/lib.rs`、`runtime_bridge.rs`（7 个文件）

### 2.3 内置数学函数

**依赖**：`libm` crate（纯 Rust 实现，no_std 兼容）

```toml
# crates/runtime-core/Cargo.toml
[dependencies]
libm = "0.2"
```

**函数清单**：

| 函数 | 签名 | 用途 |
|---|---|---|
| `abs(x)` | `f32 -> f32` | 跟随误差计算 |
| `min(x, y)` | `(f32, f32) -> f32` | 输出限幅 |
| `max(x, y)` | `(f32, f32) -> f32` | 输出限幅 |
| `sin(x)` | `f32 -> f32` | 修正正弦运动规律 |
| `cos(x)` | `f32 -> f32` | 运动曲线生成 |
| `sqrt(x)` | `f32 -> f32` | 距离计算 |
| `pow(x, y)` | `(f32, f32) -> f32` | 多项式求值 |
| `fmod(x, y)` | `(f32, f32) -> f32` | 周期凸轮取模 |
| `clamp(x, lo, hi)` | `(f32, f32, f32) -> f32` | 输出限幅 |

**语义校验**：在 `validate_function_calls` pass 中检查函数名合法性和参数个数。

### 2.4 表达式条件

**目标 DSL 语法**：

```plc
wait: abs(master_pos - slave_pos) < 0.5
wait: encoder_x.position >= target_pos AND cam_active == true
```

**`src/parser/plc.pest` 改动**：

```pest
# 改前
simple_condition = { condition_operand ~ comparison_operator ~ condition_value }

# 改后：左侧支持表达式
condition_expr = { expression ~ comparison_operator ~ expression }
simple_condition = { condition_expr | condition_operand ~ comparison_operator ~ condition_value }
```

**运行时改动**：`WaitDigital` / `WaitAnalog` 之外新增 `WaitExpr`：

```rust
pub enum Instr<'a> {
    // 现有...
    WaitExpr {
        left_ops: &'a [ExprOp],
        op: CompareOp,
        right_ops: &'a [ExprOp],
        next: StepId,
        timeout: Option<Timeout>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp { Eq, Ne, Gt, Lt, Ge, Le }
```

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`runtime-core/src/lib.rs`、`runtime_bridge.rs`（5 个文件）

---

## 3. 阶段 1：查找表与插值原语

### 3.1 查找表声明

**目标 DSL 语法**：

```plc
[topology]
cam_table linear_cam: periodic [
    (0.0, 0.0),
    (90.0, 50.0),
    (180.0, 50.0),
    (270.0, 0.0),
    (360.0, 0.0)
]

cam_table shear_profile: oneshot [
    (0.0, 0.0),
    (30.0, 5.0),
    (60.0, 45.0),
    (90.0, 90.0),
    (120.0, 135.0),
    (150.0, 175.0),
    (180.0, 180.0)
]
```

**`src/parser/plc.pest` 改动**：

```pest
cam_table_mode = { "periodic" | "oneshot" }
cam_point = { "(" ~ number ~ "," ~ number ~ ")" }
cam_point_list = { "[" ~ cam_point ~ ("," ~ cam_point)* ~ ","? ~ "]" }
cam_table_declaration = { "cam_table" ~ identifier ~ ":" ~ cam_table_mode ~ cam_point_list }

# topology_entry 增加
topology_entry = _{ device_declaration | relation_declaration | variable_declaration | cam_table_declaration }
```

**`src/ast/mod.rs` 改动**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamPoint {
    pub master: f32,
    pub slave: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CamTableMode { Periodic, Oneshot }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamTableDeclaration {
    pub line: usize,
    pub name: String,
    pub mode: CamTableMode,
    pub points: Vec<CamPoint>,
}

// TopologySection 增加
pub struct TopologySection {
    pub devices: Vec<DeviceDeclaration>,
    pub connections: Vec<TopologyConnection>,
    pub variables: Vec<VariableDeclaration>,
    pub cam_tables: Vec<CamTableDeclaration>,  // 新增
}
```

**语义校验**（`semantic/mod.rs`）：
- 主轴位置必须严格递增
- 周期表首尾从轴值必须相等（`points[0].slave == points[last].slave`）
- 点数 >= 2 且 <= `MAX_CAM_POINTS`（256）

### 3.2 IR 层凸轮表

**`src/ir/mod.rs` 改动**：

```rust
pub const MAX_CAM_POINTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SplineCoeff {
    pub a: f32,  // 常数项
    pub b: f32,  // 一次项
    pub c: f32,  // 二次项
    pub d: f32,  // 三次项
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CamTableIr {
    pub name: String,
    pub periodic: bool,
    pub num_points: usize,
    pub master_positions: Vec<f32>,
    pub slave_positions: Vec<f32>,
    pub spline_coeffs: Vec<SplineCoeff>,  // 编译期预计算
}
```

样条系数在语义降级阶段（`semantic/mod.rs`）用 Thomas 算法预计算，运行时只做多项式求值。

### 3.3 线性插值（运行时）

```rust
// crates/runtime-core/src/lib.rs

pub struct CamTableData {
    pub periodic: bool,
    pub num_points: u16,
    pub master: [f32; MAX_CAM_POINTS],
    pub slave: [f32; MAX_CAM_POINTS],
    pub coeffs: [SplineCoeff; MAX_CAM_POINTS],  // 三次样条系数
    pub last_index: u16,  // 缓存上次查找索引
}

/// 二分查找定位区间，返回左端点索引
fn binary_search_interval(table: &CamTableData, x: f32) -> u16 {
    let n = table.num_points as usize;
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo < hi - 1 {
        let mid = (lo + hi) / 2;
        if table.master[mid] <= x { lo = mid; } else { hi = mid; }
    }
    lo as u16
}

/// 线性插值
pub fn linear_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = table.master[i + 1] - table.master[i];
    if dx == 0.0 { return table.slave[i]; }
    let t = (x - table.master[i]) / dx;
    table.slave[i] + t * (table.slave[i + 1] - table.slave[i])
}

/// 主轴位置归一化：周期表用 floor-based wrap（避免负值），非周期表做边界钳制
fn normalize_master(table: &CamTableData, master_pos: f32) -> f32 {
    let x0 = table.master[0];
    let xn = table.master[table.num_points as usize - 1];
    if table.periodic {
        let period = xn - x0;
        if period <= 0.0 { return x0; }
        // floor-based wrap：保证结果在 [x0, xn) 范围内，即使输入为负
        let offset = master_pos - x0;
        x0 + offset - (offset / period).floor() * period
    } else {
        // oneshot 表：钳制到 [x0, xn]，防止外推
        if master_pos < x0 { x0 }
        else if master_pos > xn { xn }
        else { master_pos }
    }
}
```

### 3.4 三次样条插值

**编译期：Thomas 算法预计算系数**（`semantic/mod.rs`）：

```rust
/// 自然三次样条系数计算（Thomas 算法，O(n)）
/// 输入：n 个数据点 (x[i], y[i])
/// 输出：n-1 组系数 (a, b, c, d)，使得
///   S_i(x) = a_i + b_i*(x-x_i) + c_i*(x-x_i)^2 + d_i*(x-x_i)^3
fn compute_spline_coefficients(
    x: &[f32], y: &[f32], periodic: bool
) -> Vec<SplineCoeff> {
    // Thomas 算法求解三对角方程组
    // 周期边界条件：S'(x_0) = S'(x_{n-1}), S''(x_0) = S''(x_{n-1})
    // 自然边界条件：S''(x_0) = S''(x_{n-1}) = 0
    // ... 实现细节省略，标准数值方法
}
```

**运行时：Horner 法多项式求值**（`runtime-core`）：

```rust
/// 三次样条插值（Horner 法，4 次乘法 + 3 次加法）
pub fn cubic_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = &table.coeffs[i];
    // Horner: a + dx*(b + dx*(c + dx*d))
    c.a + dx * (c.b + dx * (c.c + dx * c.d))
}

/// 一阶导数（速度比）
pub fn cubic_derivative(table: &CamTableData, master_pos: f32) -> f32 {
    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = &table.coeffs[i];
    // S'(x) = b + 2c*dx + 3d*dx^2
    c.b + dx * (2.0 * c.c + 3.0 * c.d * dx)
}
```

**连续性保证**：三次样条提供 C2 连续性（位置、速度、加速度连续），避免机械冲击。

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`ir/mod.rs`、`semantic/mod.rs`、`runtime-core/src/lib.rs`（6 个文件）

---

## 4. 阶段 2：凸轮配置系统

### 4.1 新设备类型 `cam_coupling`

**目标 DSL 语法**：

```plc
[topology]
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_x,
    table: linear_cam,
    interpolation: cubic_spline,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 2.0
}
```

**改动位置总览（同 motor-control-enhancement 模式，16 处）**：

| # | 文件 | 函数/位置 | 改动内容 |
|---|---|---|---|
| 1 | `plc.pest:20` | `device_type` | 新增 `"cam_coupling"` 关键字 |
| 2 | `plc.pest:36` | `attribute_name` | 新增 `"master"` / `"slave"` / `"table"` / `"interpolation"` / `"gear_ratio"` / `"phase_offset"` / `"following_error_limit"` |
| 3 | `parser/mod.rs` | `parse_device_type` | 新增 `match` 分支 |
| 4 | `ast/mod.rs:29` | `DeviceType` | 新增 `CamCoupling` 变体 |
| 5 | `ir/mod.rs:13` | `DeviceKind` | 新增 `CamCoupling` 变体 |
| 6 | `semantic/mod.rs` | `device_type_str` | 新增分支 |
| 7 | `semantic/mod.rs` | `implicit_port_ids_for_device_type` | 新增端口列表 |
| 8 | `semantic/mod.rs` | `device_type_name` | 新增分支 |
| 9 | `semantic/mod.rs` | `default_states_for_kind` | 新增分支 |
| 10 | `semantic/mod.rs` | `device_kind_name` | 新增分支 |
| 11 | `semantic/mod.rs` | `map_device_kind` | 新增分支 |
| 12 | `verification/safety.rs` | `collect_device_domains` | 新增回退分支 |
| 13 | `topology_semantic_gate.rs` | `implicit_ports_for_type` | 新增端口定义 |
| 14 | `topology_semantic_gate.rs` | `device_type_name` | 新增分支 |
| 15 | `device_subtype.rs` | `device_type_label` | 新增分支 |
| 16 | `devices/cam_coupling.toml` | 设备库文件 | 新增 |

### 4.2 凸轮动作语句

```plc
action: cam_engage cam_xy          # 启动凸轮耦合
action: cam_disengage cam_xy       # 解除凸轮耦合
action: cam_switch cam_xy new_table # 切换凸轮表
action: cam_phase cam_xy 15.0      # 调整相位偏移
```

**`src/parser/plc.pest` 改动**：

```pest
action_cam_engage = { "cam_engage" ~ identifier }
action_cam_disengage = { "cam_disengage" ~ identifier }
action_cam_switch = { "cam_switch" ~ identifier ~ identifier }
action_cam_phase = { "cam_phase" ~ identifier ~ expression }

# action_command 增加（长关键字在前）
action_command = {
    action_extend | action_retract
    | action_cam_disengage | action_cam_engage
    | action_cam_switch | action_cam_phase
    | action_set_analog_expr | action_set_analog
    | action_set | action_log | compute_statement
}
```

**`src/ast/mod.rs` 改动**：

```rust
pub enum ActionStatement {
    // 现有...
    CamEngage { target: String },
    CamDisengage { target: String },
    CamSwitch { target: String, new_table: String },
    CamPhase { target: String, offset: Expression },  // 支持变量和表达式
}
```

### 4.3 凸轮状态查询

`cam_coupling` 暴露以下端口供 `wait` / `if` 使用：

| 端口 | 类型 | 说明 |
|---|---|---|
| `engage` | digital | 凸轮耦合使能（`on`=已耦合，`off`=未耦合） |
| `in_sync` | digital | 从轴是否已同步（跟随误差在窗口内） |
| `fault` | digital | 跟随误差超限等故障 |
| `following_error` | analog | 当前跟随误差值 |
| `master_pos` | analog | 当前主轴位置 |
| `slave_cmd` | analog | 当前从轴指令位置 |

```plc
wait: cam_xy.engage == true        # engage 端口状态为 on
wait: cam_xy.in_sync == true
wait: cam_xy.following_error < 1.0
if: cam_xy.engage == true goto step_running else: goto step_idle
```

### 4.4 设备库 TOML

```toml
# devices/cam_coupling.toml
[identity]
name = "电子凸轮耦合"
type = "cam_coupling"

[semantics]
description = "软件定义的主从轴位置同步，替代机械凸轮。"
role = "controller"
typical_applications = ["飞剪", "印刷套色", "旋转灌装", "包装封切"]

[[interfaces.ports]]
name = "engage"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "input"
description = "凸轮耦合使能"

[[interfaces.ports]]
name = "in_sync"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "同步状态，从轴跟随误差在窗口内时置位"

[[interfaces.ports]]
name = "fault"
port_type = "digital"
states = ["on", "off"]
default_state = "off"
direction = "output"
description = "故障输出，跟随误差超限时置位"

[device_constraints]

[[device_constraints.safety]]
left = "fault.on"
right = "engage.on"
relation = "conflicts_with"
reason = "凸轮故障时必须解除耦合（IEC 61800-5-2）"

[usage_guidance]
when_to_use = "需要主从轴位置同步的场景：飞剪、套色、旋转灌装等。"
when_not_to_use = "简单定位用 servo_drive；调速用 vfd。"
```

**影响范围**：`plc.pest`、`parser/mod.rs`、`ast/mod.rs`、`ir/mod.rs`、`semantic/mod.rs`、`verification/safety.rs`、`topology_semantic_gate.rs`、`device_subtype.rs`、`devices/cam_coupling.toml`（9 个文件）

---

## 5. 阶段 3：凸轮运行时集成

### 5.1 Tick 循环中的凸轮求值

在 `runtime-core` 的 tick 循环中，凸轮求值插入在 PID 之后、状态机之前：

```
tick() {
    1. update_pid_loops(now, io)       // 现有
    2. update_cam_couplings(now, io)   // 新增
    3. state_machine_evaluation()      // 现有
}
```

**`crates/runtime-core/src/lib.rs` 改动**：

```rust
pub const MAX_CAM_COUPLINGS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamCouplingConfig {
    pub master_input: AnalogInputId,     // 主轴编码器 AI
    pub slave_output: AnalogOutputId,    // 从轴位置指令 AO
    pub table_index: u16,               // 凸轮表索引（初始表）
    pub gear_ratio: f32,
    pub initial_phase_offset: f32,      // 初始相位偏移（运行时相位存于 CamState）
    // cam_period 已删除：周期性由 CamTableData.periodic 决定，
    // 周期值从 master[last] - master[0] 自动推导
    pub following_error_limit: f32,
    pub slave_feedback: AnalogInputId,   // 从轴实际位置 AI
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CamState {
    pub engaged: bool,
    pub master_pos: f32,
    pub slave_cmd: f32,
    pub slave_actual: f32,
    pub following_error: f32,
    pub in_sync: bool,
    pub fault: bool,
    pub active_table: u16,
    pub phase_offset: f32,        // 运行时可变相位偏移（cam_phase 写入此处）
    pub switch_offset: f32,       // 凸轮切换时的偏移修正
    pub switch_decay_ticks: u16,  // 偏移衰减剩余 tick 数
}

// Program 增加凸轮配置
pub struct Program<'a> {
    pub tasks: &'a [Task<'a>],
    pub pid_loops: &'a [PidConfig],
    pub var_init: &'a [f32],
    pub cam_configs: &'a [CamCouplingConfig],  // 新增
    pub cam_tables: &'a [CamTableData],        // 新增
}

// Runtime 增加凸轮状态
pub struct Runtime<'a> {
    program: &'a Program<'a>,
    loc: Location,
    step_entered_at: Option<Tick>,
    pid_states: [PidState; MAX_PID_LOOPS],
    variables: [f32; MAX_VARIABLES],
    cam_states: [CamState; MAX_CAM_COUPLINGS],  // 新增
}
```

**边界保护**：`Runtime::new` 中增加凸轮数量校验，与 PID 循环校验对齐：

```rust
// Runtime::new 中新增
if program.cam_configs.len() > MAX_CAM_COUPLINGS {
    return Err(RuntimeError::TooManyCamCouplings {
        configured: program.cam_configs.len(),
        max: MAX_CAM_COUPLINGS,
    });
}
// 校验每个 cam_config 引用的 table_index 在范围内
for (i, cfg) in program.cam_configs.iter().enumerate() {
    if cfg.table_index as usize >= program.cam_tables.len() {
        return Err(RuntimeError::InvalidCamTableIndex {
            cam_index: i,
            table_index: cfg.table_index,
        });
    }
}
```

`RuntimeError` 新增变体：

```rust
pub enum RuntimeError {
    // 现有...
    TooManyCamCouplings { configured: usize, max: usize },
    InvalidCamTableIndex { cam_index: usize, table_index: u16 },
    InvalidCamIndex { cam_index: u16 },
}
```

### 5.2 凸轮求值主循环

```rust
fn update_cam_couplings<IO: Io>(&mut self, _now: Tick, io: &mut IO) {
    for i in 0..self.program.cam_configs.len() {
        let cfg = &self.program.cam_configs[i];
        let state = &mut self.cam_states[i];

        if !state.engaged { continue; }

        // 1. 读主轴位置
        state.master_pos = io.read_analog_input(cfg.master_input);

        // 2. 齿轮比 + 相位偏移（phase_offset 在 CamState 中，可被 cam_phase 动态修改）
        let adjusted = state.master_pos * cfg.gear_ratio + state.phase_offset;

        // 3. 归一化主轴位置（复用 normalize_master，处理周期 wrap 和 oneshot clamp）
        let table = &self.program.cam_tables[state.active_table as usize];
        let lookup_pos = normalize_master(table, adjusted);

        // 4. 查表插值
        state.slave_cmd = cubic_interpolate(table, lookup_pos);

        // 5. 凸轮切换偏移衰减
        if state.switch_decay_ticks > 0 {
            state.slave_cmd += state.switch_offset;
            state.switch_offset *= 0.95;  // 指数衰减
            state.switch_decay_ticks -= 1;
        }

        // 6. 写从轴指令
        io.write_analog_output(cfg.slave_output, state.slave_cmd);

        // 7. 读从轴反馈 + 跟随误差
        state.slave_actual = io.read_analog_input(cfg.slave_feedback);
        state.following_error = abs(state.slave_cmd - state.slave_actual);

        // 8. 同步判定
        state.in_sync = state.following_error < cfg.following_error_limit;

        // 9. 故障检测
        if state.following_error > cfg.following_error_limit * 3.0 {
            state.fault = true;
            state.engaged = false;  // 自动脱开
        }
    }
}
```

### 5.3 凸轮动作执行

在 `Instr` 枚举中新增凸轮指令变体：

```rust
pub enum Instr<'a> {
    // 现有...
    CamEngage { cam_index: u16, next: StepId },
    CamDisengage { cam_index: u16, next: StepId },
    CamSwitch { cam_index: u16, table_index: u16, next: StepId },
    CamPhase { cam_index: u16, offset_ops: &'a [ExprOp], next: StepId },
}
```

状态机执行分支：

```rust
Instr::CamEngage { cam_index, next } => {
    if cam_index as usize >= self.program.cam_configs.len() {
        return Err(RuntimeError::InvalidCamIndex { cam_index });
    }
    let cfg = &self.program.cam_configs[cam_index as usize];
    let state = &mut self.cam_states[cam_index as usize];
    state.engaged = true;
    state.fault = false;
    state.active_table = cfg.table_index;           // 从配置初始化当前表
    state.phase_offset = cfg.initial_phase_offset;  // 从配置初始化相位
    state.switch_offset = 0.0;
    state.switch_decay_ticks = 0;
    self.transition(now, next, TransitionReason::Action, &mut on_event)?;
    continue;
}
Instr::CamDisengage { cam_index, next } => {
    if cam_index as usize >= self.program.cam_configs.len() {
        return Err(RuntimeError::InvalidCamIndex { cam_index });
    }
    self.cam_states[cam_index as usize].engaged = false;
    self.transition(now, next, TransitionReason::Action, &mut on_event)?;
    continue;
}
Instr::CamSwitch { cam_index, table_index, next } => {
    if cam_index as usize >= self.program.cam_configs.len() {
        return Err(RuntimeError::InvalidCamIndex { cam_index });
    }
    if table_index as usize >= self.program.cam_tables.len() {
        return Err(RuntimeError::InvalidCamTableIndex {
            cam_index: cam_index as usize, table_index,
        });
    }
    let cfg = &self.program.cam_configs[cam_index as usize];
    let state = &mut self.cam_states[cam_index as usize];
    let old_cmd = state.slave_cmd;
    state.active_table = table_index;
    // 复用与主循环相同的坐标变换：齿轮比 + 相位偏移 + 归一化
    let adjusted = state.master_pos * cfg.gear_ratio + state.phase_offset;
    let new_table = &self.program.cam_tables[table_index as usize];
    let lookup_pos = normalize_master(new_table, adjusted);
    let new_cmd = cubic_interpolate(new_table, lookup_pos);
    // 偏移修正 = 旧指令 - 新指令，逐步衰减到 0
    state.switch_offset = old_cmd - new_cmd;
    state.switch_decay_ticks = 100;  // 100 tick 衰减
    self.transition(now, next, TransitionReason::Action, &mut on_event)?;
    continue;
}
Instr::CamPhase { cam_index, offset_ops, next } => {
    if cam_index as usize >= self.program.cam_configs.len() {
        return Err(RuntimeError::InvalidCamIndex { cam_index });
    }
    let offset = eval_expr(offset_ops, &self.variables, io);
    self.cam_states[cam_index as usize].phase_offset = offset;
    self.transition(now, next, TransitionReason::Action, &mut on_event)?;
    continue;
}
```

### 5.4 实时性预算

RP2040（125MHz Cortex-M0+）上的凸轮求值耗时估算：

| 操作 | 耗时 | 说明 |
|---|---|---|
| 二分查找（256 点表） | ~3 us | 8 次比较 |
| 三次样条求值（Horner） | ~1 us | 4 次乘法 + 3 次加法 |
| 跟随误差 + 同步判定 | ~1 us | 减法 + abs + 比较 |
| 单个凸轮耦合总计 | ~5 us | |
| 8 个凸轮耦合 | ~40 us | 占 1ms tick 的 4% |

**结论**：在 1ms tick 周期下，8 路凸轮耦合的计算开销约 4%，留有充足余量。

**影响范围**：`runtime-core/src/lib.rs`、`runtime_bridge.rs`、`sim/`（3 个文件/crate）

---

## 6. 阶段 4：凸轮验证

### 6.1 安全约束

凸轮系统引入新的安全约束模式：

```plc
[constraints]
# 故障互锁（现有 safety 语法已支持）
safety: cam_xy.fault.on conflicts_with cam_xy.engage.on
    reason: "凸轮故障时必须解除耦合"

# 从轴使能前置（现有 safety 语法已支持）
safety: servo_x.enable.off conflicts_with cam_xy.engage.on
    reason: "从轴伺服未使能时不得启动凸轮耦合"

# 跟随误差超限保护（现有 safety 语法已支持 analog_condition）
safety: cam_xy.following_error > 5.0 conflicts_with cam_xy.engage.on
    reason: "跟随误差超限必须停机（IEC 61800-5-2 §4.3）"
```

**注意**：`cam_xy.following_error > 5.0` 使用的是现有 `analog_condition` 语法（`plc.pest:123`），不需要阶段 0 的表达式引擎。现有 `safety_operand` 规则已支持 `analog_condition | state_reference`，其中 `analog_condition = { identifier ~ comparison_operator ~ (measured_value | number) }`。但需要确认 `cam_xy.following_error` 能被解析为 `identifier`（含点号的 `state_reference`），可能需要将 `safety_operand` 扩展为也接受 `state_reference ~ comparison_operator ~ number`。

**`src/parser/plc.pest` 改动**：

```pest
# 改前
analog_condition = { identifier ~ comparison_operator ~ (measured_value | number) }

# 改后：左侧也接受 state_reference（如 cam_xy.following_error）
analog_condition = { (state_reference | identifier) ~ comparison_operator ~ (measured_value | number) }
```

**`src/verification/safety.rs` 改动**：

`safety_expr_states_with_reason` 需要处理 `cam_coupling` 端口的模拟量阈值约束。当 `SafetyExpr::Threshold` 的 `device` 是 `cam_xy.following_error` 时，需要解析为设备 `cam_xy` 的模拟端口 `following_error`。

### 6.2 时序验证

```plc
[constraints]
timing: task.cam_control must_complete_within 1ms
    reason: "凸轮求值必须在一个 tick 内完成"

timing: task.cam_control.engage must_complete_within 5000ms
    reason: "凸轮同步必须在 5 秒内建立"
```

**验证引擎改动**（`verification/timing.rs`）：

凸轮动作（`cam_engage`/`cam_disengage`/`cam_switch`/`cam_phase`）的执行时间为 0ms（瞬时指令），但 `wait: cam_xy.in_sync == true` 的等待时间取决于从轴响应特性，需要结合 `response_time` 属性估算。

### 6.3 因果验证

```plc
[constraints]
causality: encoder_main -> cam_xy -> servo_x
    reason: "主轴编码器信号必须经凸轮耦合传递到从轴伺服"
```

**验证引擎改动**（`verification/causality.rs`）：

`cam_coupling` 设备在拓扑图中建立 `master → cam_coupling → slave` 的因果链。因果 BFS 遍历需要识别 `cam_coupling` 的 `master` 和 `slave` 属性作为上下游连接。

### 6.4 凸轮表静态分析

编译期对凸轮表做以下检查（在 `semantic/mod.rs` 中实现）：

| 检查项 | 规则 | 错误类型 |
|---|---|---|
| 主轴单调递增 | `master[i] < master[i+1]` | `PlcError::semantic` |
| 周期表首尾一致 | `periodic && slave[0] != slave[last]` → 错误 | `PlcError::semantic` |
| 点数范围 | `2 <= n <= MAX_CAM_POINTS(256)` | `PlcError::semantic` |
| 导数上界 | `max(|S'(x)|) < drive_max_speed` | `PlcError::warning`（需要设备参数） |
| 加速度上界 | `max(|S''(x)|) < drive_max_accel` | `PlcError::warning`（需要设备参数） |
| 引用完整性 | `cam_coupling.table` 引用的表名必须存在 | `PlcError::undefined_reference` |
| 主从设备存在 | `cam_coupling.master` / `slave` 引用的设备必须存在 | `PlcError::undefined_reference` |

**影响范围**：`verification/safety.rs`、`verification/timing.rs`、`verification/causality.rs`、`semantic/mod.rs`（4 个文件）

---

## 7. 完整示例

### 7.1 飞剪（Flying Shear）

```plc
[topology]
device plc_main: plc {}
device encoder_conv: sensor { detects: conveyor, response_time: 1ms }
device servo_knife: servo_drive {
    enable: true,
    max_speed: 3000,
    positioning_window: 0.5
}
device product_sensor: sensor { detects: conveyor, debounce: 5ms }

variable target_length: float = 500.0
variable cut_count: int = 0

# 飞剪凸轮曲线：驻停 → 加速 → 等速切割 → 减速 → 返回
cam_table shear_cam: periodic [
    (0.0, 0.0),
    (60.0, 2.0),
    (120.0, 30.0),
    (150.0, 80.0),
    (180.0, 180.0),
    (210.0, 280.0),
    (240.0, 330.0),
    (300.0, 358.0),
    (360.0, 360.0)
]

device cam_shear: cam_coupling {
    master: encoder_conv,
    slave: servo_knife,
    table: shear_cam,
    interpolation: cubic_spline,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 2.0
}

relation { from: encoder_conv, to: plc_main, via: reports_to }
relation { from: plc_main, to: servo_knife.enable, via: driven_by }

[constraints]
safety: cam_shear.fault.on conflicts_with cam_shear.engage.on
    reason: "飞剪凸轮故障时必须脱开"
safety: servo_knife.enable.off conflicts_with cam_shear.engage.on
    reason: "伺服未使能时不得启动凸轮"

timing: task.shear_control.sync must_complete_within 3000ms
    reason: "飞剪同步必须在 3 秒内建立"

causality: encoder_conv -> cam_shear -> servo_knife
    reason: "输送带编码器驱动飞剪凸轮"

[tasks]
task shear_control:
  step init:
    action: set servo_knife.enable on
    wait: servo_knife.ready == true
    timeout: 5000ms -> goto fault
  step sync:
    action: cam_engage cam_shear
    wait: cam_shear.in_sync == true
    timeout: 3000ms -> goto fault
  step running:
    wait: product_sensor == true
    action: compute cut_count = cut_count + 1
    action: log "cut completed"
    allow_indefinite_wait: true
  step fault:
    action: cam_disengage cam_shear
    action: set servo_knife.enable off
    action: log "shear fault - cam disengaged"
    allow_indefinite_wait: true
  on_complete: goto shear_control.init
```

### 7.2 印刷套色（Print Registration）

```plc
[topology]
device plc_main: plc {}
device encoder_web: sensor { detects: web_roller, response_time: 1ms }
device servo_print: servo_drive {
    enable: true,
    max_speed: 2000,
    positioning_window: 0.1
}
device reg_sensor: sensor { detects: print_mark, debounce: 2ms }

variable phase_correction: float = 0.0
variable reg_error: float = 0.0

# 印刷凸轮：1:1 线性映射
cam_table print_cam: periodic [
    (0.0, 0.0),
    (360.0, 360.0)
]

device cam_print: cam_coupling {
    master: encoder_web,
    slave: servo_print,
    table: print_cam,
    interpolation: cubic_spline,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 1.0
}

relation { from: encoder_web, to: plc_main, via: reports_to }
relation { from: reg_sensor, to: plc_main, via: reports_to }
relation { from: plc_main, to: servo_print.enable, via: driven_by }

[constraints]
safety: cam_print.fault.on conflicts_with cam_print.engage.on
    reason: "套色凸轮故障时必须脱开"

causality: encoder_web -> cam_print -> servo_print
    reason: "料卷编码器驱动印刷辊凸轮"

[tasks]
task print_control:
  step init:
    action: set servo_print.enable on
    wait: servo_print.ready == true
    timeout: 5000ms -> goto fault
  step sync:
    action: cam_engage cam_print
    wait: cam_print.in_sync == true
    timeout: 3000ms -> goto fault
  step running:
    wait: reg_sensor == true
    action: compute phase_correction = phase_correction + reg_error * 0.3
    action: cam_phase cam_print phase_correction
    allow_indefinite_wait: true
  step fault:
    action: cam_disengage cam_print
    action: set servo_print.enable off
    action: log "print registration fault"
    allow_indefinite_wait: true
  on_complete: goto print_control.init
```

---

## 8. 实施顺序与依赖

```
阶段 0（表达式引擎基础，无外部依赖）
  ├── 0a: 变量声明（plc.pest + parser + ast + ir + semantic + runtime-core）
  ├── 0b: 算术表达式 + compute 动作（plc.pest + parser + ast + ir + runtime-core）
  ├── 0c: 数学函数库（runtime-core + libm 依赖）
  └── 0d: 表达式条件（plc.pest + parser + runtime-core）
  （0a → 0b → 0c 串行；0d 依赖 0b）

阶段 1（查找表与插值，依赖阶段 0）
  ├── 1a: 凸轮表声明语法（plc.pest + parser + ast + ir）
  ├── 1b: 线性插值运行时（runtime-core）
  └── 1c: 三次样条预计算 + 运行时求值（semantic + runtime-core）
  （1a → 1b → 1c 串行）

阶段 2（凸轮配置系统，依赖阶段 1）
  ├── 2a: cam_coupling 设备类型（16 处代码改动）
  ├── 2b: 凸轮动作语句（plc.pest + parser + ast + ir）
  ├── 2c: 凸轮状态查询（semantic + runtime_bridge）
  └── 2d: 设备库 TOML（devices/cam_coupling.toml）
  （2a → 2b/2c/2d 并行）

阶段 3（凸轮运行时，依赖阶段 2）
  ├── 3a: tick 循环凸轮求值（runtime-core）
  ├── 3b: 跟随误差监控（runtime-core）
  ├── 3c: 凸轮切换（runtime-core）
  └── 3d: SIL 仿真集成（sim crate）
  （3a → 3b/3c 并行；3d 依赖 3a）

阶段 4（凸轮验证，依赖阶段 2，与阶段 3 并行）
  ├── 4a: 安全约束（verification/safety.rs）
  ├── 4b: 时序验证（verification/timing.rs）
  ├── 4c: 因果验证（verification/causality.rs）
  └── 4d: 凸轮表静态分析（semantic/mod.rs）
  （4a/4b/4c/4d 相互独立，可并行）
```

---

## 9. 验收测试矩阵

### 阶段 0

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 变量声明解析 | `variable x: float = 0.0` 能解析为 AST | 新增 fixture |
| 变量类型校验 | `variable x: float = true` 报语义错误 | 新增 error fixture |
| 算术表达式解析 | `compute y = x * 2.0 + 1.0` 能解析 | 新增 fixture |
| 运算符优先级 | `a + b * c` 解析为 `a + (b * c)` | 单元测试 parser |
| 栈式求值正确性 | `eval_expr` 对各种表达式返回正确结果 | 单元测试 runtime-core |
| 数学函数 | `sin(3.14159)` ≈ 0.0 | 单元测试 runtime-core |
| 表达式条件 | `wait: abs(x - y) < 0.5` 能解析和执行 | 集成测试 |
| 现有测试不回归 | 所有现有 `.plc` 示例仍可编译 | `cargo test --test examples_integration` |

### 阶段 1

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 凸轮表解析 | `cam_table t: periodic [...]` 能解析 | 新增 fixture |
| 主轴单调性校验 | 非递增主轴报语义错误 | 新增 error fixture |
| 线性插值精度 | 已知表的中间点插值误差 < 1e-6 | 单元测试 runtime-core |
| 三次样条 C2 连续 | 相邻段边界处二阶导数连续 | 单元测试 semantic |
| 周期表取模 | `master_pos = 720.0` 正确映射到 `[0, 360)` | 单元测试 runtime-core |
| 二分查找边界 | 表首/表尾/精确命中点的查找正确 | 单元测试 runtime-core |

### 阶段 2

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| cam_coupling 解析 | `device x: cam_coupling { ... }` 能解析 | 新增 fixture |
| 凸轮动作解析 | `cam_engage` / `cam_disengage` / `cam_switch` / `cam_phase` | 新增 fixture |
| 设备库约束注入 | `fault.on conflicts_with engage.on` 自动注入 | 单元测试 |
| 引用完整性 | `table: nonexistent` 报 `undefined_reference` | 新增 error fixture |
| 飞剪示例可编译 | `examples/flying_shear.plc` 无错误 | CI |

### 阶段 3

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 凸轮求值正确性 | engage 后从轴跟随主轴 | SIL 仿真 + trace 对比 |
| 跟随误差检测 | 误差超限时 fault 置位 | SIL 仿真 |
| 凸轮切换连续性 | 切换瞬间从轴无跳变 | SIL 仿真 + trace 检查 |
| 凸轮切换连续性（变换参数） | `gear_ratio != 1` + 非零 `phase_offset` 下切表，从轴无跳变 | SIL 仿真 + trace 检查 |
| 相位调整 | `cam_phase` 后从轴偏移正确 | SIL 仿真 |
| 实时性 | 单路凸轮求值 < 10us（RP2040） | timing-report |
| cam_disengage 越界 | `cam_index >= cam_configs.len()` 时返回 `InvalidCamIndex` | 单元测试 runtime-core |
| cam_phase 越界 | `cam_index >= cam_configs.len()` 时返回 `InvalidCamIndex` | 单元测试 runtime-core |
| runtime_bridge 端口映射 | `CamState.engaged` 变化后下一 tick `engage` 端口同步更新 | 单元测试 runtime_bridge |

### 阶段 4

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 安全约束验证 | 故障互锁在 safety 引擎中通过 | `cargo test --test verification_capability` |
| 因果链验证 | `encoder → cam → servo` 因果链通过 | 集成测试 |
| 凸轮表静态分析 | 非单调表报错、周期表首尾不一致报错 | 新增 error fixture |

---

## 10. 与现有系统的兼容性

| 方面 | 影响 | 说明 |
|---|---|---|
| 现有 `.plc` 文件 | 无影响 | 所有新语法为增量添加，不修改现有规则 |
| 运行时内存 | 增加约 70KB | `MAX_VARIABLES(64) × 4B + MAX_CAM_POINTS(256) × 20B × 8 tables` |
| no_std 兼容 | 保持 | 固定大小数组，`libm` 纯 Rust 实现 |
| PID 循环 | 无影响 | 凸轮求值在 PID 之后独立执行 |
| trace 格式 | 扩展 | JSONL 每行增加 `cam_states` 字段 |
| 表达式引擎复用 | 正向 | 未来可用于 PID 动态设定值、模拟量计算等 |

---

## 11. 设计决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | 固定大小数组（`MAX_VARIABLES=64`、`MAX_CAM_POINTS=256`、`MAX_CAM_COUPLINGS=8`） | no_std 兼容，RP2040 内存有限（264KB SRAM），避免动态分配 |
| 2 | 三次样条系数编译期预计算 | Thomas 算法需要临时数组，不适合 no_std 运行时；编译期无此限制 |
| 3 | 栈式表达式求值器（后缀指令序列） | 无递归、栈深度有界（`MAX_EXPR_STACK=16`）、执行时间确定性 |
| 4 | Horner 法多项式求值 | 数值稳定性优于直接展开，且减少乘法次数 |
| 5 | 跟随误差每 tick 检查 | 安全关键：不能等到步骤转换才检查，必须实时监控 |
| 6 | 凸轮切换用偏移衰减（非混合） | 实现简单、行为可预测、易于形式化验证；混合方案需要两张表同时求值，开销翻倍 |
| 7 | `cam_coupling` 作为独立设备类型（非语法糖） | 凸轮耦合有独立的状态（engaged/in_sync/fault）和生命周期，不适合展开为普通步骤 |
| 8 | 表达式引擎与凸轮系统分阶段交付 | 表达式引擎是通用基础设施，独立于凸轮有价值（PID 动态设定值、模拟量计算等） |

---

## 12. 实施现状与下一步升级（2026-02-26）

### 12.1 当前实现现状（已落地）

- `cam_coupling` 在运行时使用**模拟量通道抽象**：
  - `master_input: AnalogInputId`
  - `slave_output: AnalogOutputId`
  - `slave_feedback: AnalogInputId`
- 每 tick 的凸轮计算路径是：读主轴 AI → 凸轮插值计算 → 写从轴 AO → 读反馈 AI 计算 following_error。
- `runtime_bridge` 会把拓扑中的 `master/slave/slave_feedback` 解析并映射到 AI/AO 物理通道；无法唯一解析时直接报错（避免错误隐式绑定）。
- 这也是当前测试中常见 `AI0/AO0` 夹具写法的原因：用于最小闭环验证凸轮算法与状态机行为。

### 12.2 为什么当前先采用 AI/AO 抽象

| 设计点 | 当前选择 | 原因 |
|---|---|---|
| 运行时接口 | AI/AO 通道 ID | 与现有 PID/模拟量路径共享基础设施，降低实现复杂度 |
| 实时性 | 通道级直接读写 | 避免多层对象分发，保持 tick 内执行确定性 |
| no_std 约束 | 固定数组 + 通道索引 | 减少动态结构，便于 RP2040 资源控制 |
| 验证闭环 | 先闭合数值链路 | 优先保障“可算、可测、可回归” |

### 12.3 已识别的语义不足

- 对用户来说，“主轴/从轴”是运动语义（encoder/servo），而当前底层是 AI/AO 映射，存在认知落差。
- 文档/示例中若直接出现 `AI0/AO0`，会弱化设备层语义表达，不利于工程可读性。
- 长期看，凸轮应支持更明确的“轴端点语义”而不仅是“模拟通道语义”。

### 12.4 下一步升级计划（v1.4 目标）

1. **轴端点语义层（Axis Endpoint）**
   - 在语义/桥接层引入标准端点语义：`encoder.position`、`servo.cmd_pos`、`servo.fb_pos`。
   - `cam_coupling.master/slave/slave_feedback` 优先绑定这些端点，而非直接暴露 AI/AO 名称。

2. **桥接策略升级（兼容优先）**
   - 新规则：优先端点映射，其次回退到当前 AI/AO 自动解析。
   - 保持现有项目可运行，不做破坏性迁移。

3. **诊断与报错升级**
   - 当绑定失败时，错误信息同时给出“设备语义路径”和“通道路径”，减少排障成本。

4. **示例与测试升级**
   - 新增以 `encoder_main`/`servo_x` 为主语义的凸轮示例与回归测试。
   - `AI0/AO0` 夹具保留在底层单测，用于最小数值路径验证。

5. **验收标准补充**
   - 增加“端点语义映射正确性”测试矩阵（正例 + 失败诊断）。
   - 保持现有 cam regression gate 全绿，并新增端点映射 gate。

---

## 13. Codex 审查问题修正记录

### v1.1 修正

| # | 问题 | 严重度 | 修正方案 |
|---|---|---|---|
| 1 | `cam_phase` 语法只接受字面量 `number`，但示例传变量 `phase_correction`；运行时修改只读 `Program` | 🔴 高 | `cam_phase` 改为接受 `expression`；`phase_offset` 从 `CamCouplingConfig` 移到 `CamState`；运行时用 `eval_expr` 求值后写入 `CamState` |
| 2 | `active_table` 未初始化，`CamEngage` 也没设置，默认总是 0 号表 | 🔴 高 | `CamEngage` 执行时从 `CamCouplingConfig.table_index` 初始化 `active_table`，同时初始化 `phase_offset`、`switch_offset`、`switch_decay_ticks` |
| 3 | 端口命名不一致：查询用 `engaged`，约束用 `engage.on` | 🔴 高 | 统一端口名为 `engage`，状态 `on/off`；查询写 `cam_xy.engage == true`（映射到 `engage.on`） |
| 4 | 插值归一化不安全：`fmod` 可能返回负值；`oneshot` 表未做边界钳制 | 🔴 高 | 周期表改用 floor-based wrap（`offset - floor(offset/period) * period`）；`oneshot` 表做 `[x0, xN]` clamp；抽取 `normalize_master()` 函数统一处理 |
| 5 | 示例用 `{}` 风格 task/step，与现有 DSL `:` 风格不一致 | 🟡 中 | 所有示例改回 `task name:` / `step name:` 冒号风格；不引入语法迁移 |
| 6 | 安全约束 `following_error > 5.0` 未描述 safety 语法/语义升级 | 🟡 中 | 补充说明：利用现有 `analog_condition` 语法，但需扩展 `analog_condition` 左侧接受 `state_reference`；补充 `plc.pest` 和 `safety.rs` 改动 |
| 7 | 周期表首尾一致性严重度前后不一致（一处"必须"，一处"warning"） | 🟢 低 | 统一为 `PlcError::semantic`（错误），偏安全策略 |

### v1.2 修正

| # | 问题 | 严重度 | 修正方案 |
|---|---|---|---|
| 8 | `cam_period` 配置了但运行时不生效（`normalize_master` 只看表本身） | 🔴 高 | 删除 `cam_period` 字段和 DSL 属性；周期性由 `CamTableData.periodic` 决定，周期值从 `master[last] - master[0]` 自动推导 |
| 9 | `CamSwitch` 连续性补偿用 `state.master_pos` 直接插值，未经齿轮比+相位偏移变换 | 🔴 高 | 切表时复用主循环相同的坐标变换：`adjusted = master_pos * gear_ratio + phase_offset`，再 `normalize_master` + `cubic_interpolate` |
| 10 | 固定数组边界保护不完整：循环按 `cam_configs.len()` 迭代可能越界 | 🟡 中 | `Runtime::new` 增加 `TooManyCamCouplings` 和 `InvalidCamTableIndex` 检查；`CamEngage` / `CamSwitch` 增加 `cam_index` 和 `table_index` 防御校验 |

### v1.3 修正

| # | 问题 | 严重度 | 修正方案 |
|---|---|---|---|
| 11 | `CamDisengage` 和 `CamPhase` 缺少 `cam_index` 边界检查，与 `CamEngage`/`CamSwitch` 不对齐 | 🔴 高 | 四个凸轮指令分支统一先校验 `cam_index < cam_configs.len()`，否则返回 `RuntimeError::InvalidCamIndex` |
| 12 | 开放问题决策写"写入端口状态"，但伪代码只更新 `CamState.engaged`，未体现端口映射路径 | 🟡 中 | 明确：`CamState.engaged` 是权威状态源，端口状态由 `runtime_bridge` 每 tick 从 `CamState` 映射（`bridge.map_cam_port`），`cam_engage`/`cam_disengage` 不直接操作端口 I/O |

### 开放问题决策

| 问题 | 决策 |
|---|---|
| DSL task 语法是否从 `:` 迁到 `{}` 风格？ | **不迁移**。保持现有 `:` 风格，本文档所有示例已修正。如未来需要迁移，应作为独立提案处理。 |
| `engage` 是"命令输入端口"还是只保留运行时状态？ | **两者并存**。`engage` 是设备库定义的 digital input 端口（`direction: input`），运行时 `CamState.engaged` 是权威状态源。端口状态由 `runtime_bridge` 在每个 tick 结束时从 `CamState` 映射：`bridge.map_cam_port("engage", cam_state.engaged)`。`cam_engage` / `cam_disengage` 动作只写 `CamState.engaged`，不直接操作端口 I/O。 |
| 文档伪代码是概念草图还是实现约束？ | **实现约束**。伪代码应可直接映射到最终 Rust 实现，因此边界保护、坐标变换一致性等必须在文档中体现。 |

---

**文档状态**：v1.3，已根据三轮 Codex 审查修正 12 个问题并回答 3 个开放问题。可进入实施拆解。
