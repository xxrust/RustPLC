# DSL 控制逻辑 + Rust 计算 的落地规划

本文档给出"DSL 仅保留控制逻辑，复杂计算由 Rust 执行"的具体落地方案与阶段计划。目标是：保持 DSL 可验证性，同时把数值/算法复杂度移出 DSL。

**文档状态**: 基于 POC 架构验证后的正式实施规划
**相关文档**:
- `hybrid_architecture_poc.md` - POC 实施指南（已完成）
- `dsl_verification_boundary.md` - 形式化验证边界论证
- `dsl_computation_analysis.md` - DSL 计算能力分析报告
**重要说明**: 本文包含“拟议扩展”。当前 DSL 尚未支持 `extern function` / `action: call` / `on_error` / 命名参数 / 多返回 / 数组类型等语法，相关内容在文中已标注为“拟议”或“可选扩展”。

---

## 1. 目标与非目标

### 1.1 核心目标
- **DSL 聚焦控制平面**: 状态机、互锁、时序、IO、简单算术与比较
- **复杂计算走 Rust**: 拟合、矩阵、优化、统计、PID 等数值算法
- **清晰的职责边界**: DSL 负责"调用与契约检查"，Rust 负责"计算实现与数值稳定性"
- **保障确定性**: 所有外部函数必须在 tick 内完成，有明确的时间上界
- **保持可验证性**: DSL 的形式化验证能力不受影响，外部函数通过契约验证

### 1.2 非目标
- **不在 DSL 中引入通用编程能力**: 避免引入动态循环、递归、动态内存分配
- **不要求 DSL 对数值算法做完整证明**: 数值正确性由 Rust 单元测试和数值分析保证
- **不支持任意外部调用**: 禁止阻塞 IO、系统调用、随机数生成等不确定性操作

---

## 2. 总体架构

### 2.1 架构图
```
┌─────────────────────────────────────────────────────────────┐
│                        DSL 层（控制平面）                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ 状态机逻辑    │  │ 互锁约束      │  │ 时序验证      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                          │                                   │
│                          │ extern function call             │
│                          ▼                                   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           ExternFunctionRegistry (契约层)            │   │
│  │  - 函数签名验证                                       │   │
│  │  - 参数类型检查                                       │   │
│  │  - 时间上界保证                                       │   │
│  │  - 输入范围验证                                       │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Rust 层（计算平面）                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ 数值算法      │  │ 线性代数      │  │ 优化求解      │      │
│  │ (拟合/统计)   │  │ (矩阵运算)    │  │ (PID/MPC)     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                              │
│  验证方式：单元测试 + 性能基准 + 数值稳定性分析                │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 边界原则
- **DSL 职责**: 控制逻辑与调度，不承担算法实现
- **外部函数契约要求**:
  - 纯函数或显式副作用声明
  - 有最大耗时上界（必须在 tick 内完成）
  - 有输入范围与失败条件
  - 禁止阻塞、IO、随机数等不确定性操作

### 2.3 验证边界
- **DSL 层验证**: 状态机安全性、互锁正确性、时序约束、因果关系
- **契约层验证**: 函数签名匹配、参数类型正确、调用时机合法
- **Rust 层验证**: 单元测试、性能基准、数值稳定性分析（可选）

---

## 3. DSL 层拟议扩展

### 3.1 外部函数声明语法

在 `[topology]` 段中声明外部函数（仅声明，不实现）。**该语法为拟议，当前 DSL 尚未支持**：

```plc
[topology]

# 简单函数示例
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

# 复杂函数示例（二次拟合）
extern function quadratic_fit(
    x0: float, x1: float, x2: float, x3: float, x4: float,
    y0: float, y1: float, y2: float, y3: float, y4: float
) -> (float, float, float) {
    rust_module: "math::fitting"
    pure: true
    time_bound_us: 50
    input_range: {
        x0..x4: [-1e3, 1e3],
        y0..y4: [-1e6, 1e6]
    }
    output_range: {
        a: [-1e6, 1e6],
        b: [-1e6, 1e6],
        c: [-1e6, 1e6]
    }
    error_condition: "det ~ 0 -> singular matrix"
}

# PID 控制器示例
extern function pid_update(
    error: float,
    kp: float,
    ki: float,
    kd: float,
    dt: float
) -> float {
    rust_module: "control::pid"
    pure: false  # 有内部状态（积分项、上次误差）
    time_bound_us: 20
    input_range: {
        error: [-1e6, 1e6],
        kp: [0.0, 1e3],
        ki: [0.0, 1e3],
        kd: [0.0, 1e3],
        dt: [0.001, 1.0]
    }
    output_range: {
        output: [-1e6, 1e6]
    }
}
```

### 3.2 外部函数调用语法

在 `[tasks]` 段的 step 中调用外部函数（**拟议语法**）：

```plc
[tasks]

task temperature_control:
    step measure:
        # 假设 temp_value 已由 IO 映射更新
        wait temp_value > 0.0
        delay: 10ms

    step compute_control:
        # 计算 PID 控制输出
        action: call pid_update(
            setpoint - temp_value,
            kp, ki, kd, dt
        ) -> control_output
        delay: 5ms

    step apply_control:
        # 应用控制输出
        action: set_analog heater control_output
        delay: 10ms
        goto measure

task curve_fitting:
    step collect_data:
        # 假设已经采集了数据到变量 x0..x4, y0..y4
        delay: 100ms

    step fit:
        # 调用拟合函数
        action: call quadratic_fit(
            x0, x1, x2, x3, x4,
            y0, y1, y2, y3, y4
        ) -> (coeff_a, coeff_b, coeff_c)
        delay: 10ms

    step check_result:
        # 检查拟合结果
        if coeff_a > 0.0:
            goto success
        else:
            goto retry
```

### 3.3 语法要点

1. **声明位置**: 外部函数声明放在 `[topology]` 段，与设备声明并列
2. **参数传递**:
   - 支持位置参数: `call func(x, y, z)`
   - 支持命名参数: `call func(a: x, b: y)`
   - 支持字面量: `call func(1.0, 2.0)`
3. **返回值绑定**:
   - 单返回值: `-> result`
   - 多返回值: `-> (a, b, c)`
4. **调用位置**: 只能在 step 的 action 中调用，不能在表达式中调用
5. **契约字段**:
   - `rust_module`: Rust 模块路径（必需）
   - `pure`: 是否纯函数（必需）
   - `time_bound_us`: 最大执行时间（必需）
   - `input_range`: 输入范围约束（可选）
   - `output_range`: 输出范围约束（可选）
   - `error_condition`: 错误条件描述（可选）

---

## 4. Rust 计算侧设计

### 4.1 外部函数注册表

```rust
// crates/runtime-core/src/extern_functions/mod.rs
// 注意：runtime-core 当前为 no_std。以下代码为概念示意，实际实现应放在
// std 侧（例如 runtime_bridge/CLI），或使用 alloc + hashbrown 等 no_std 方案。
// 执行时间度量也应由宿主提供（或基于 tick 预算），避免直接依赖 std::time::Instant。
// 当前运行时变量类型为 f32，示例中的 f64 需按实际实现统一为 f32。

use std::collections::HashMap;

/// 外部函数签名
pub type ExternFn = fn(&[f64]) -> Result<Vec<f64>, ExternError>;

/// 外部函数错误
#[derive(Debug, Clone)]
pub enum ExternError {
    InvalidArgCount { expected: usize, got: usize },
    InvalidArgType { arg_index: usize, expected: &'static str },
    InputOutOfRange { arg_index: usize, value: f64, range: (f64, f64) },
    OutputOutOfRange { result_index: usize, value: f64, range: (f64, f64) },
    RuntimeError(String),
    TimeoutExceeded { elapsed_us: u64, limit_us: u64 },
}

/// 外部函数元数据
#[derive(Debug, Clone)]
pub struct ExternFunctionInfo {
    pub name: String,
    pub param_types: Vec<ParamType>,
    pub return_type: ReturnType,
    pub rust_module: String,
    pub is_pure: bool,
    pub time_bound_us: u64,
    pub input_ranges: Option<Vec<(f64, f64)>>,
    pub output_ranges: Option<Vec<(f64, f64)>>,
    pub function: ExternFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnType {
    Void,
    Single(ParamType),
    Tuple(Vec<ParamType>),
}

/// 外部函数注册表
pub struct ExternFunctionRegistry {
    functions: HashMap<String, ExternFunctionInfo>,
}

impl ExternFunctionRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            functions: HashMap::new(),
        };
        registry.register_builtin_functions();
        registry
    }

    fn register_builtin_functions(&mut self) {
        // 注册基础数学函数
        self.register(ExternFunctionInfo {
            name: "add".to_string(),
            param_types: vec![ParamType::Float, ParamType::Float],
            return_type: ReturnType::Single(ParamType::Float),
            rust_module: "math::basic".to_string(),
            is_pure: true,
            time_bound_us: 10,
            input_ranges: None,
            output_ranges: None,
            function: math::basic::add,
        });

        // 注册二次拟合函数
        self.register(ExternFunctionInfo {
            name: "quadratic_fit".to_string(),
            param_types: vec![ParamType::Float; 10],
            return_type: ReturnType::Tuple(vec![ParamType::Float; 3]),
            rust_module: "math::fitting".to_string(),
            is_pure: true,
            time_bound_us: 50,
            input_ranges: Some(vec![
                (-1e3, 1e3), (-1e3, 1e3), (-1e3, 1e3), (-1e3, 1e3), (-1e3, 1e3),
                (-1e6, 1e6), (-1e6, 1e6), (-1e6, 1e6), (-1e6, 1e6), (-1e6, 1e6),
            ]),
            output_ranges: Some(vec![(-1e6, 1e6), (-1e6, 1e6), (-1e6, 1e6)]),
            function: math::fitting::quadratic_fit,
        });

        // 注册 PID 控制器
        self.register(ExternFunctionInfo {
            name: "pid_update".to_string(),
            param_types: vec![ParamType::Float; 5],
            return_type: ReturnType::Single(ParamType::Float),
            rust_module: "control::pid".to_string(),
            is_pure: false,  // 有内部状态
            time_bound_us: 20,
            input_ranges: Some(vec![
                (-1e6, 1e6),  // error
                (0.0, 1e3),   // kp
                (0.0, 1e3),   // ki
                (0.0, 1e3),   // kd
                (0.001, 1.0), // dt
            ]),
            output_ranges: Some(vec![(-1e6, 1e6)]),
            function: control::pid::update,
        });
    }

    pub fn register(&mut self, info: ExternFunctionInfo) {
        self.functions.insert(info.name.clone(), info);
    }

    pub fn get(&self, name: &str) -> Option<&ExternFunctionInfo> {
        self.functions.get(name)
    }

    pub fn call(
        &self,
        name: &str,
        args: &[f64],
    ) -> Result<Vec<f64>, ExternError> {
        let info = self.functions.get(name)
            .ok_or_else(|| ExternError::RuntimeError(
                format!("Function '{}' not found", name)
            ))?;

        // 验证参数数量
        if args.len() != info.param_types.len() {
            return Err(ExternError::InvalidArgCount {
                expected: info.param_types.len(),
                got: args.len(),
            });
        }

        // 验证输入范围
        if let Some(ranges) = &info.input_ranges {
            for (i, (&arg, &(min, max))) in args.iter().zip(ranges.iter()).enumerate() {
                if arg < min || arg > max {
                    return Err(ExternError::InputOutOfRange {
                        arg_index: i,
                        value: arg,
                        range: (min, max),
                    });
                }
            }
        }

        // 执行时间由宿主计时（或基于 tick 预算估算）
        let (result, elapsed_us) = timed_call(info.function, args)?;

        // 检查时间上界
        if elapsed_us > info.time_bound_us {
            return Err(ExternError::TimeoutExceeded {
                elapsed_us,
                limit_us: info.time_bound_us,
            });
        }

        // 验证输出范围
        if let Some(ranges) = &info.output_ranges {
            for (i, (&value, &(min, max))) in result.iter().zip(ranges.iter()).enumerate() {
                if value < min || value > max {
                    return Err(ExternError::OutputOutOfRange {
                        result_index: i,
                        value,
                        range: (min, max),
                    });
                }
            }
        }

        Ok(result)
    }
}
```

### 4.2 内置函数实现

```rust
// crates/runtime-core/src/extern_functions/math/basic.rs

pub fn add(args: &[f64]) -> Result<Vec<f64>, ExternError> {
    if args.len() != 2 {
        return Err(ExternError::InvalidArgCount {
            expected: 2,
            got: args.len(),
        });
    }
    Ok(vec![args[0] + args[1]])
}

pub fn multiply(args: &[f64]) -> Result<Vec<f64>, ExternError> {
    if args.len() != 2 {
        return Err(ExternError::InvalidArgCount {
            expected: 2,
            got: args.len(),
        });
    }
    Ok(vec![args[0] * args[1]])
}

// crates/runtime-core/src/extern_functions/math/fitting.rs

pub fn quadratic_fit(args: &[f64]) -> Result<Vec<f64>, ExternError> {
    // 参数：x0-x4 (5个), y0-y4 (5个)
    if args.len() != 10 {
        return Err(ExternError::InvalidArgCount {
            expected: 10,
            got: args.len(),
        });
    }

    let x = &args[0..5];
    let y = &args[5..10];

    // 使用最小二乘法拟合 y = a + b*x + c*x^2
    match quadratic_fit_impl(x, y) {
        Ok((a, b, c)) => Ok(vec![a, b, c]),
        Err(e) => Err(ExternError::RuntimeError(e)),
    }
}

fn quadratic_fit_impl(x: &[f64], y: &[f64]) -> Result<(f64, f64, f64), String> {
    let n = x.len() as f64;

    // 构建正规方程的系数矩阵
    let (sum_x, sum_x2, sum_x3, sum_x4) = x.iter()
        .fold((0.0, 0.0, 0.0, 0.0), |(sx, sx2, sx3, sx4), &xi| {
            let xi2 = xi * xi;
            (sx + xi, sx2 + xi2, sx3 + xi2 * xi, sx4 + xi2 * xi2)
        });

    let sum_y = y.iter().sum::<f64>();
    let sum_xy = x.iter().zip(y).map(|(xi, yi)| xi * yi).sum::<f64>();
    let sum_x2y = x.iter().zip(y).map(|(xi, yi)| xi * xi * yi).sum::<f64>();

    // 使用克拉默法则求解 3x3 线性方程组
    // | n      sum_x   sum_x2 | | a |   | sum_y   |
    // | sum_x  sum_x2  sum_x3 | | b | = | sum_xy  |
    // | sum_x2 sum_x3  sum_x4 | | c |   | sum_x2y |

    let det = n * sum_x2 * sum_x4
            + sum_x * sum_x3 * sum_x2
            + sum_x2 * sum_x * sum_x3
            - sum_x2 * sum_x2 * sum_x2
            - sum_x * sum_x * sum_x4
            - n * sum_x3 * sum_x3;

    if det.abs() < 1e-10 {
        return Err("Singular matrix: determinant too small".to_string());
    }

    let det_a = sum_y * sum_x2 * sum_x4
              + sum_xy * sum_x3 * sum_x2
              + sum_x2y * sum_x * sum_x3
              - sum_x2y * sum_x2 * sum_x2
              - sum_xy * sum_x * sum_x4
              - sum_y * sum_x3 * sum_x3;

    let det_b = n * sum_xy * sum_x4
              + sum_x * sum_x2y * sum_x2
              + sum_x2 * sum_y * sum_x3
              - sum_x2 * sum_xy * sum_x2
              - sum_x * sum_y * sum_x4
              - n * sum_x2y * sum_x3;

    let det_c = n * sum_x2 * sum_x2y
              + sum_x * sum_x3 * sum_y
              + sum_x2 * sum_x * sum_xy
              - sum_x2 * sum_x2 * sum_y
              - sum_x * sum_x * sum_x2y
              - n * sum_x3 * sum_xy;

    let a = det_a / det;
    let b = det_b / det;
    let c = det_c / det;

    // 检查结果是否有效
    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return Err("Result not finite (NaN or Inf)".to_string());
    }

    Ok((a, b, c))
}

// crates/runtime-core/src/extern_functions/control/pid.rs

use std::cell::RefCell;

/// PID 控制器状态（非纯函数，有内部状态）
thread_local! {
    static PID_STATE: RefCell<PidState> = RefCell::new(PidState::default());
}

#[derive(Debug, Clone, Default)]
struct PidState {
    integral: f64,
    last_error: f64,
}

pub fn update(args: &[f64]) -> Result<Vec<f64>, ExternError> {
    if args.len() != 5 {
        return Err(ExternError::InvalidArgCount {
            expected: 5,
            got: args.len(),
        });
    }

    let error = args[0];
    let kp = args[1];
    let ki = args[2];
    let kd = args[3];
    let dt = args[4];

    PID_STATE.with(|state| {
        let mut state = state.borrow_mut();

        // 比例项
        let p_term = kp * error;

        // 积分项（梯形积分）
        state.integral += (error + state.last_error) * 0.5 * dt;
        let i_term = ki * state.integral;

        // 微分项
        let d_term = if dt > 0.0 {
            kd * (error - state.last_error) / dt
        } else {
            0.0
        };

        // 更新状态
        state.last_error = error;

        // 计算输出
        let output = p_term + i_term + d_term;

        Ok(vec![output])
    })
}

pub fn reset() {
    PID_STATE.with(|state| {
        *state.borrow_mut() = PidState::default();
    });
}
```

### 4.3 运行时集成

```rust
// crates/runtime-core/src/runtime.rs

use crate::extern_functions::{ExternFunctionRegistry, ExternError};

pub struct Runtime<'a, IO: Io> {
    program: &'a Program,
    state: State,
    variables: Vec<f64>,
    io: &'a mut IO,
    extern_registry: &'a ExternFunctionRegistry,
}

impl<'a, IO: Io> Runtime<'a, IO> {
    pub fn new(
        program: &'a Program,
        io: &'a mut IO,
        extern_registry: &'a ExternFunctionRegistry,
    ) -> Self {
        Self {
            program,
            state: State::default(),
            variables: vec![0.0; program.variable_count()],
            io,
            extern_registry,
        }
    }

    pub fn tick(&mut self) -> Result<(), RuntimeError> {
        // 执行当前状态的转换
        let transition = self.program.get_transition(&self.state)?;

        for action in &transition.actions {
            self.execute_action(action)?;
        }

        // 更新状态
        self.state = transition.next_state.clone();

        Ok(())
    }

    fn execute_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        match action {
            Action::SetDevice { device_id, value } => {
                self.io.write_output(*device_id, *value)?;
            }

            Action::Delay { duration_ms } => {
                // 延时在 tick 级别处理
            }

            Action::CallExtern {
                function_name,
                arg_indices,
                return_var_indices,
            } => {
                // 收集参数值
                let args: Vec<f64> = arg_indices
                    .iter()
                    .map(|&idx| self.variables[idx])
                    .collect();

                // 调用外部函数
                let result = self.extern_registry
                    .call(function_name, &args)
                    .map_err(|e| RuntimeError::ExternFunctionError {
                        function: function_name.clone(),
                        error: format!("{:?}", e),
                    })?;

                // 存储返回值
                for (i, &var_idx) in return_var_indices.iter().enumerate() {
                    if let Some(&value) = result.get(i) {
                        self.variables[var_idx] = value;
                    }
                }
            }

            Action::Compute { var_idx, expr } => {
                // 现有的计算逻辑
                let value = self.evaluate_expression(expr)?;
                self.variables[*var_idx] = value;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeError {
    InvalidState,
    IoError(String),
    ExternFunctionError {
        function: String,
        error: String,
    },
    ComputeError(String),
}
```

---

## 5. 编译器改造

### 5.1 AST 扩展

```rust
// src/ast/mod.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlcProgram {
    pub topology: TopologySection,
    pub constraints: ConstraintsSection,
    pub tasks: TasksSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySection {
    pub devices: Vec<DeviceDeclaration>,
    pub variables: Vec<VariableDeclaration>,
    pub extern_functions: Vec<ExternFunctionDeclaration>,  // 新增
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternFunctionDeclaration {
    pub line: usize,
    pub name: String,
    pub params: Vec<FunctionParam>,
    pub return_type: FunctionReturnType,
    pub rust_module: String,
    pub is_pure: bool,
    pub time_bound_us: u64,
    pub input_ranges: Option<Vec<RangeConstraint>>,
    pub output_ranges: Option<Vec<RangeConstraint>>,
    pub error_condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionParam {
    pub name: String,
    pub param_type: FunctionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FunctionType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FunctionReturnType {
    Void,
    Single(FunctionType),
    Tuple(Vec<FunctionType>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeConstraint {
    pub param_name: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepStatement {
    // 现有变体...
    Action(ActionStatement),
    Wait(WaitCondition),
    Delay { duration_ms: u32 },
    // ...
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionStatement {
    // 现有变体...
    SetDevice { device: String, state: DeviceState },

    // 新增：调用外部函数
    CallExtern {
        function_name: String,
        args: Vec<CallArg>,
        return_vars: Vec<String>,
    },

    // 新增：计算表达式
    Compute {
        var_name: String,
        expr: Expression,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallArg {
    Variable(String),
    Literal(f64),
    Named { name: String, value: Box<CallArg> },
}
```

### 5.2 Parser 扩展

```rust
// src/parser/plc.pest

// 在 topology_entry 中添加
topology_entry = {
    device_declaration
  | variable_declaration
  | extern_function_declaration  // 新增
}

extern_function_declaration = {
    "extern" ~ "function" ~ identifier ~
    "(" ~ function_param_list? ~ ")" ~
    ("->" ~ function_return_type)? ~
    "{" ~ extern_function_body ~ "}"
}

function_param_list = {
    function_param ~ ("," ~ function_param)*
}

function_param = {
    identifier ~ ":" ~ function_type
}

function_type = { "float" | "int" | "bool" }

function_return_type = {
    function_type
  | "(" ~ function_type ~ ("," ~ function_type)* ~ ")"
}

extern_function_body = {
    extern_function_attribute ~ ("," ~ extern_function_attribute)*
}

extern_function_attribute = {
    rust_module_attr
  | pure_attr
  | time_bound_attr
  | input_range_attr
  | output_range_attr
  | error_condition_attr
}

rust_module_attr = { "rust_module" ~ ":" ~ string_literal }
pure_attr = { "pure" ~ ":" ~ boolean_value }
time_bound_attr = { "time_bound_us" ~ ":" ~ number }
input_range_attr = { "input_range" ~ ":" ~ "{" ~ range_constraint_list ~ "}" }
output_range_attr = { "output_range" ~ ":" ~ "{" ~ range_constraint_list ~ "}" }
error_condition_attr = { "error_condition" ~ ":" ~ string_literal }

range_constraint_list = {
    range_constraint ~ ("," ~ range_constraint)*
}

range_constraint = {
    identifier ~ ("." ~ "." ~ identifier)? ~ ":" ~ "[" ~ number ~ "," ~ number ~ "]"
}

// 在 action_command 中添加
action_command = {
    action_set_device
  | action_call_extern  // 新增
  | action_compute      // 新增
}

action_call_extern = {
    "call" ~ identifier ~
    "(" ~ call_arg_list? ~ ")" ~
    ("->" ~ call_return_binding)?
}

call_arg_list = {
    call_arg ~ ("," ~ call_arg)*
}

call_arg = {
    named_arg
  | positional_arg
}

named_arg = { identifier ~ ":" ~ positional_arg }
positional_arg = { identifier | number | boolean_value }

call_return_binding = {
    identifier
  | "(" ~ identifier ~ ("," ~ identifier)* ~ ")"
}

action_compute = {
    "compute" ~ identifier ~ "=" ~ expression
}

expression = {
    term ~ (binary_op ~ term)*
}

term = {
    identifier
  | number
  | "(" ~ expression ~ ")"
}

binary_op = { "+" | "-" | "*" | "/" }
```

### 5.3 语义分析扩展

```rust
// src/semantic/mod.rs

pub struct SemanticAnalyzer<'a> {
    program: &'a PlcProgram,
    errors: Vec<PlcError>,
    extern_functions: HashMap<String, &'a ExternFunctionDeclaration>,
    variables: HashMap<String, VariableInfo>,
}

impl<'a> SemanticAnalyzer<'a> {
    pub fn analyze(&mut self) -> Result<(), Vec<PlcError>> {
        // 1. 收集外部函数声明
        self.collect_extern_functions();

        // 2. 收集变量声明
        self.collect_variables();

        // 3. 检查外部函数调用
        self.check_extern_function_calls();

        // 4. 检查类型匹配
        self.check_type_consistency();

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    fn collect_extern_functions(&mut self) {
        for func in &self.program.topology.extern_functions {
            if self.extern_functions.contains_key(&func.name) {
                self.errors.push(PlcError::duplicate_definition(
                    func.line,
                    &format!("extern function '{}'", func.name),
                ));
            } else {
                self.extern_functions.insert(func.name.clone(), func);
            }
        }
    }

    fn check_extern_function_calls(&mut self) {
        for task in &self.program.tasks.tasks {
            for step in &task.steps {
                for stmt in &step.statements {
                    if let StepStatement::Action(ActionStatement::CallExtern {
                        function_name,
                        args,
                        return_vars,
                    }) = stmt {
                        self.check_extern_call(function_name, args, return_vars, step.line);
                    }
                }
            }
        }
    }

    fn check_extern_call(
        &mut self,
        function_name: &str,
        args: &[CallArg],
        return_vars: &[String],
        line: usize,
    ) {
        // 检查函数是否存在
        let func = match self.extern_functions.get(function_name) {
            Some(f) => f,
            None => {
                self.errors.push(PlcError::undefined_reference(
                    line,
                    &format!("extern function '{}'", function_name),
                ));
                return;
            }
        };

        // 检查参数数量
        if args.len() != func.params.len() {
            self.errors.push(PlcError::type_mismatch(
                line,
                &format!(
                    "function '{}' expects {} arguments, got {}",
                    function_name,
                    func.params.len(),
                    args.len()
                ),
            ));
        }

        // 检查返回值数量
        let expected_return_count = match &func.return_type {
            FunctionReturnType::Void => 0,
            FunctionReturnType::Single(_) => 1,
            FunctionReturnType::Tuple(types) => types.len(),
        };

        if return_vars.len() != expected_return_count {
            self.errors.push(PlcError::type_mismatch(
                line,
                &format!(
                    "function '{}' returns {} values, got {} bindings",
                    function_name,
                    expected_return_count,
                    return_vars.len()
                ),
            ));
        }

        // 检查返回变量是否已声明
        for var_name in return_vars {
            if !self.variables.contains_key(var_name) {
                self.errors.push(PlcError::undefined_reference(
                    line,
                    &format!("variable '{}'", var_name),
                ));
            }
        }
    }
}
```

---

## 6. 类型系统与数据传递

### 6.1 当前阶段支持的类型

| 类型 | DSL 语法 | Rust 类型 | 说明 |
|------|---------|-----------|------|
| 浮点数 | `float` | `f64` | 主要用于传感器值、控制输出 |
| 整数 | `int` | `i64` | 用于计数、索引 |
| 布尔 | `bool` | `bool` | 用于条件判断 |

### 6.2 参数传递方式

1. **位置参数**（推荐用于参数少的函数）
```plc
action: call add(x, y) -> result
```

2. **命名参数**（推荐用于参数多的函数）
```plc
action: call pid_update(
    setpoint - current,
    kp, ki, kd, dt
) -> output
```

3. **混合方式**
```plc
action: call quadratic_fit(
    x0, x1, x2, x3, x4,  # 位置参数
    y0: data[0],         # 命名参数
    y1: data[1],
    y2: data[2],
    y3: data[3],
    y4: data[4]
) -> (a, b, c)
```

### 6.3 返回值绑定

1. **单返回值**
```plc
action: call sqrt(x) -> result
```

2. **多返回值（元组解构）**
```plc
action: call quadratic_fit(...) -> (a, b, c)
```

3. **忽略返回值**
```plc
action: call log_data(x, y, z)  # 无返回值
```

### 6.4 未来扩展：受限数组（Phase 3）

如果业务确实需要数组支持，可以引入受限数组：

```plc
# 声明固定长度数组
# 当前 DSL 仅支持标量变量，采用固定数量样本
variable sample0: float = 0.0
variable sample1: float = 0.0
variable sample2: float = 0.0
variable sample3: float = 0.0
variable sample4: float = 0.0
variable sample5: float = 0.0
variable sample6: float = 0.0
variable sample7: float = 0.0
variable sample8: float = 0.0
variable sample9: float = 0.0

# 外部函数接受固定数量标量（拟议语法）
extern function mean10(
    s0: float, s1: float, s2: float, s3: float, s4: float,
    s5: float, s6: float, s7: float, s8: float, s9: float
) -> float {
    rust_module: "math::stats"
    pure: true
    time_bound_us: 30
}

# 调用
action: call mean10(sample0, sample1, sample2, sample3, sample4, sample5, sample6, sample7, sample8, sample9) -> average
```

**约束条件**：
- 数组长度必须编译期固定
- 索引必须可静态证明在范围内
- 禁止动态分配和切片

---

## 7. 形式化验证边界

### 7.1 DSL 层验证（完整保留）

DSL 层的形式化验证能力**不受影响**，继续验证：

1. **安全性验证（Safety）**
   - 互锁约束：`conflicts_with` 检查
   - 前置条件：`requires` 检查
   - 状态不变式：全局约束检查
   - **外部函数调用安全性**：
     - 函数签名匹配
     - 参数类型正确
     - 返回值绑定合法
     - 调用时机符合状态机约束

2. **活性验证（Liveness）**
   - 死锁检测：SCC 分析
   - 活锁检测：循环路径分析
   - 进度保证：`on_complete` 路径检查
   - **外部函数不阻塞**：通过 `time_bound_us` 契约保证

3. **时序验证（Timing）**
   - 关键路径分析：`must_complete_within` 检查
   - 响应时间累加：包含外部函数的 `time_bound_us`
   - 超时保护：`timeout` 语句覆盖
   - **外部函数时间上界**：编译期检查 + 运行时测量

4. **因果性验证（Causality）**
   - 信号传播链：`connected_to` 拓扑分析
   - 传感器检测：`detects` 关系检查
   - 控制逻辑因果：action → wait 配对
   - **外部函数纯度**：通过 `pure` 契约声明

### 7.2 契约层验证（新增）

外部函数通过**契约**与 DSL 层交互，契约验证包括：

1. **编译期契约检查**
   - 函数签名匹配：参数数量、类型、返回值
   - 调用位置合法：只能在 action 中调用
   - 变量绑定正确：返回值变量已声明
   - 时间预算充足：tick 内所有外部函数的 `time_bound_us` 总和 < tick_ms

2. **运行时契约检查**
   - 输入范围验证：`input_range` 约束
   - 输出范围验证：`output_range` 约束
   - 时间上界测量：实际执行时间 ≤ `time_bound_us`
   - 错误条件处理：`error_condition` 触发时的错误传播

3. **契约违反处理**
   - 编译期违反：编译错误，拒绝生成代码
   - 运行时违反：
     - 输入超范围 → `RuntimeError::InputOutOfRange`
     - 输出超范围 → `RuntimeError::OutputOutOfRange`
     - 超时 → `RuntimeError::TimeoutExceeded`
     - 函数错误 → `RuntimeError::ExternFunctionError`

### 7.3 Rust 层验证（独立保证）

Rust 实现的外部函数通过以下方式保证正确性：

1. **单元测试**
   - 功能正确性：输入 → 输出映射正确
   - 边界条件：极值、零值、特殊值
   - 错误处理：异常输入的错误返回
   - 数值稳定性：浮点误差在可接受范围

2. **性能基准测试**
   - 平均执行时间 < `time_bound_us` 的 50%（留余量）
   - P99 执行时间 < `time_bound_us`
   - 最坏情况执行时间 < `time_bound_us`

3. **数值分析（可选）**
   - 条件数分析：矩阵运算的数值稳定性
   - 误差传播：浮点运算的累积误差
   - 收敛性证明：迭代算法的收敛保证

### 7.4 验证边界示例

以二次拟合为例，说明三层验证的职责划分：

```plc
[topology]
extern function quadratic_fit(
    x0: float, x1: float, x2: float, x3: float, x4: float,
    y0: float, y1: float, y2: float, y3: float, y4: float
) -> (float, float, float) {
    rust_module: "math::fitting"
    pure: true
    time_bound_us: 50
    input_range: {
        x0..x4: [-1e3, 1e3],
        y0..y4: [-1e6, 1e6]
    }
    output_range: {
        a: [-1e6, 1e6],
        b: [-1e6, 1e6],
        c: [-1e6, 1e6]
    }
    error_condition: "det ~ 0 -> singular matrix"
}

[tasks]
task curve_fitting:
    step collect:
        # DSL 验证：状态机路径、时序约束
        wait data_ready == true
        delay: 100ms

    step fit:
        # 契约验证：签名匹配、参数类型、时间预算
        action: call quadratic_fit(
            x0, x1, x2, x3, x4,
            y0, y1, y2, y3, y4
        ) -> (a, b, c)
        # 运行时验证：输入范围、输出范围、时间上界
        delay: 10ms

    step check:
        # DSL 验证：控制逻辑安全性
        if a > 0.0:
            goto success
        else:
            goto retry
```

**验证职责划分**：
- **DSL 层**：验证 `collect → fit → check` 的状态机路径合法、时序约束满足
- **契约层**：验证 `quadratic_fit` 调用签名正确、参数类型匹配、时间预算充足
- **Rust 层**：保证 `quadratic_fit` 实现正确、数值稳定、性能达标

### 7.5 验证能力对比

| 验证项 | 纯 DSL 方案 | 混合架构方案 | 说明 |
|--------|------------|-------------|------|
| 状态机安全性 | ✅ 完整 | ✅ 完整 | 不受影响 |
| 互锁约束 | ✅ 完整 | ✅ 完整 | 不受影响 |
| 时序约束 | ✅ 完整 | ✅ 完整 | 包含外部函数时间 |
| 因果关系 | ✅ 完整 | ✅ 完整 | 通过 pure 契约保证 |
| 数值算法正确性 | ❌ 不可判定 | ⚠️ 契约 + 测试 | 通过契约封装 |
| 浮点误差分析 | ❌ 不支持 | ⚠️ 可选分析 | Rust 层独立保证 |
| 复杂计算性能 | ❌ 不支持 | ✅ 基准测试 | 运行时测量 |

**结论**：混合架构在保持 DSL 可验证性的同时，通过契约机制安全地引入复杂计算能力。

---

## 8. 错误处理策略

### 8.1 错误分类

1. **编译期错误**（阻止代码生成）
   - 函数未声明
   - 参数数量/类型不匹配
   - 返回值绑定错误
   - 时间预算超限

2. **运行时错误**（需要恢复机制）
   - 输入超范围
   - 输出超范围
   - 执行超时
   - 函数内部错误（如奇异矩阵）

### 8.2 错误传播方式

#### 方式 1：错误变量（推荐）

```plc
[topology]
variable last_error: int = 0  # 0 = 无错误

[tasks]
task main:
    step compute:
        action: call quadratic_fit(...) -> (a, b, c)
        # 如果出错，last_error 会被设置为错误码
        delay: 10ms

    step check_error:
        if last_error != 0:
            goto error_handler
        else:
            goto success

task error_handler:
    step log:
        # 记录错误
        action: call log_error(last_error)
        delay: 10ms
        goto recovery
```

#### ?? 2?Result ????????
????? DSL ??????????????????
#### ?? 3?`on_error` ??????????
??????????? DSL ???????
#### DSL 编译器测试

```rust
// tests/extern_function_compilation.rs

#[test]
fn test_extern_function_declaration_parsing() {
    let code = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 10
}
"#;

    let program = parse_plc(code).expect("should parse");
    assert_eq!(program.topology.extern_functions.len(), 1);

    let func = &program.topology.extern_functions[0];
    assert_eq!(func.name, "add");
    assert_eq!(func.params.len(), 2);
    assert_eq!(func.time_bound_us, 10);
    assert!(func.is_pure);
}

#[test]
fn test_extern_function_call_type_checking() {
    let code = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 10
}

variable x: float = 1.0
variable y: float = 2.0
variable result: float = 0.0

[tasks]
task main:
    step compute:
        action: call add(x, y) -> result
        delay: 10ms
"#;

    let program = parse_plc(code).expect("should parse");
    let result = semantic_analyze(&program);
    assert!(result.is_ok());
}

#[test]
fn test_extern_function_call_wrong_arg_count() {
    let code = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 10
}

variable x: float = 1.0

[tasks]
task main:
    step compute:
        action: call add(x) -> result  # 错误：参数不足
        delay: 10ms
"#;

    let program = parse_plc(code).expect("should parse");
    let result = semantic_analyze(&program);
    assert!(result.is_err());
}
```

#### Rust 函数测试

```rust
// crates/runtime-core/tests/math_functions.rs

#[test]
fn test_quadratic_fit_perfect_fit() {
    // y = 1 + 2x + 0.5x^2
    let x = vec![0.0, 1.0, 2.0, 3.0, 4.0];
    let y = vec![1.0, 3.5, 8.0, 14.5, 23.0];
    let args: Vec<f64> = x.into_iter().chain(y.into_iter()).collect();

    let result = math::fitting::quadratic_fit(&args).expect("should succeed");

    assert!((result[0] - 1.0).abs() < 1e-10);  // a
    assert!((result[1] - 2.0).abs() < 1e-10);  // b
    assert!((result[2] - 0.5).abs() < 1e-10);  // c
}

#[test]
fn test_quadratic_fit_singular_matrix() {
    // 所有 x 相同，矩阵奇异
    let x = vec![1.0, 1.0, 1.0, 1.0, 1.0];
    let y = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let args: Vec<f64> = x.into_iter().chain(y.into_iter()).collect();

    let result = math::fitting::quadratic_fit(&args);
    assert!(result.is_err());
}

#[test]
fn test_pid_update() {
    control::pid::reset();

    let error = 10.0;
    let kp = 2.0;
    let ki = 0.5;
    let kd = 0.1;
    let dt = 0.01;

    let args = vec![error, kp, ki, kd, dt];
    let result = control::pid::update(&args).expect("should succeed");

    // P term = 2.0 * 10.0 = 20.0
    // I term = 0.5 * 10.0 * 0.01 * 0.5 = 0.025 (梯形积分)
    // D term = 0.1 * 10.0 / 0.01 = 100.0
    let expected = 20.0 + 0.025 + 100.0;

    assert!((result[0] - expected).abs() < 0.1);
}
```

### 10.2 集成测试

```rust
// tests/extern_function_integration.rs

#[test]
fn test_end_to_end_quadratic_fit() {
    let code = r#"
[topology]
extern function quadratic_fit(
    x0: float, x1: float, x2: float, x3: float, x4: float,
    y0: float, y1: float, y2: float, y3: float, y4: float
) -> (float, float, float) {
    rust_module: "math::fitting",
    pure: true,
    time_bound_us: 50
}

variable x0: float = 0.0
variable x1: float = 1.0
variable x2: float = 2.0
variable x3: float = 3.0
variable x4: float = 4.0

variable y0: float = 1.0
variable y1: float = 3.5
variable y2: float = 8.0
variable y3: float = 14.5
variable y4: float = 23.0

variable a: float = 0.0
variable b: float = 0.0
variable c: float = 0.0

device plc_main: plc {
    purpose: "test",
    ports: [X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step compute:
        action: call quadratic_fit(x0, x1, x2, x3, x4, y0, y1, y2, y3, y4) -> (a, b, c)
        delay: 10ms

    on_complete: goto done

task done:
    step wait:
        allow_indefinite_wait: true
"#;

    // 编译
    let program = compile_plc(code).expect("should compile");

    // 创建运行时
    let registry = ExternFunctionRegistry::new();
    let mut io = TestIo::new();
    let mut runtime = Runtime::new(&program, &mut io, &registry);

    // 执行一个 tick
    runtime.tick().expect("should execute");

    // 验证结果
    let a_idx = program.find_variable("a").unwrap();
    let b_idx = program.find_variable("b").unwrap();
    let c_idx = program.find_variable("c").unwrap();

    assert!((runtime.variables()[a_idx] - 1.0).abs() < 1e-10);
    assert!((runtime.variables()[b_idx] - 2.0).abs() < 1e-10);
    assert!((runtime.variables()[c_idx] - 0.5).abs() < 1e-10);
}
```

### 10.3 性能回归测试

```bash
# 运行基准测试
cargo bench --bench extern_functions

# 对比历史数据
cargo bench --bench extern_functions -- --save-baseline main
git checkout feature-branch
cargo bench --bench extern_functions -- --baseline main
```

---

## 11. 分阶段实施计划

### Phase 1: 最小闭环（2-3 周）

**目标**: 实现基本的外部函数调用机制，验证技术可行性

#### 1.1 语法设计（3 天）
- [ ] 确定 `extern function` 声明语法
- [ ] 确定 `call` 语句语法
- [ ] 编写语法规范文档
- [ ] 设计 10+ 个示例用例

#### 1.2 编译器扩展（5 天）
- [ ] 扩展 `plc.pest` 语法规则
- [ ] 扩展 AST 类型定义
- [ ] 实现 Parser 降级逻辑
- [ ] 实现语义分析检查
  - 函数声明收集
  - 调用签名验证
  - 类型匹配检查
- [ ] 编写编译器单元测试

#### 1.3 运行时实现（5 天）
- [ ] 实现 `ExternFunctionRegistry`
- [ ] 实现 2-3 个内置函数
  - `add(a, b) -> c`
  - `multiply(a, b) -> c`
  - `quadratic_fit(x0..x4, y0..y4) -> (a, b, c)`
- [ ] 集成到 `Runtime::execute_action`
- [ ] 实现基本错误处理
- [ ] 编写运行时单元测试

#### 1.4 集成测试（2 天）
- [ ] 编写端到端测试
- [ ] 验证编译 → 运行流程
- [ ] 验证错误处理路径
- [ ] 性能基准测试

#### 交付物
- ✅ 可编译运行的 POC 代码
- ✅ 10+ 个测试用例全部通过
- ✅ 性能基准报告
- ✅ 技术可行性报告

---

### Phase 2: 契约与验证（2-3 周）

**目标**: 完善契约机制，增强验证能力

#### 2.1 契约字段扩展（3 天）
- [ ] 实现 `pure` 字段解析与检查
- [ ] 实现 `time_bound_us` 字段解析与检查
- [ ] 实现 `input_range` 字段解析与验证
- [ ] 实现 `output_range` 字段解析与验证
- [ ] 实现 `error_condition` 字段解析

#### 2.2 编译期验证增强（4 天）
- [ ] 时间预算检查
  - 计算每个 tick 的外部函数时间总和
  - 与 `tick_ms` 对比，超限报错
- [ ] 纯度检查
  - 标记非纯函数
  - 检查并发调用冲突
- [ ] 因果性验证扩展
  - 将外部函数纳入因果链分析
  - 检查 `pure` 函数的因果传播

#### 2.3 运行时验证增强（4 天）
- [ ] 输入范围验证
- [ ] 输出范围验证
- [ ] 执行时间测量与超时检测
- [ ] 性能监控与统计
- [ ] 详细的错误报告

#### 2.4 错误处理完善（3 天）
- [ ] 实现错误变量机制
- [ ] 明确错误处理机制（错误变量 + 显式检查；如需语法糖再评估 `on_error`）
- [ ] 定义标准错误码
- [ ] 编写错误处理测试

#### 交付物
- ✅ 完整的契约验证机制
- ✅ 编译期 + 运行时双重保障
- ✅ 详细的错误诊断信息
- ✅ 性能监控报告

---

### Phase 3: 生态完善（3-4 周）

**目标**: 丰富函数库，完善工具链

#### 3.1 内置函数库扩展（1 周）
- [ ] 基础数学函数
  - 已内置（表达式函数）：`abs`, `min`, `max`, `sin`, `cos`, `sqrt`, `pow`, `fmod`, `clamp`
  - 待补充（如需）：`exp`, `log`, `tan`
- [ ] 统计函数
  - `mean`, `variance`, `std_dev`
  - `median`, `percentile`
- [ ] 拟合函数
  - `linear_fit`
  - `polynomial_fit`
  - `exponential_fit`
- [ ] 控制算法
  - `pid_update`
  - `moving_average`
  - `low_pass_filter`

#### 3.2 工具链完善（1 周）
- [ ] 函数注册宏
  ```rust
  register_extern_function! {
      name: "add",
      params: [float, float],
      returns: float,
      pure: true,
      time_bound_us: 10,
      impl: |args| Ok(vec![args[0] + args[1]])
  }
  ```
- [ ] 自动生成 DSL 声明
- [ ] 性能分析工具
- [ ] 调试追踪工具

#### 3.3 文档与示例（1 周）
- [ ] 外部函数开发指南
- [ ] 契约设计最佳实践
- [ ] 10+ 个实际应用示例
- [ ] API 参考文档

#### 3.4 可选扩展（1 周）
- [ ] 受限数组支持（如果需要）
- [ ] 结构体参数支持（如果需要）
- [ ] Result 类型支持（如果需要）

#### 交付物
- ✅ 20+ 个内置函数
- ✅ 完整的开发工具链
- ✅ 详细的文档与示例
- ✅ 生产就绪的实现

---

## 12. 风险与缓解措施

### 12.1 技术风险

#### 风险 1: 性能开销过大
**影响**: 外部函数调用开销导致 tick 超时
**概率**: 中
**缓解措施**:
- 使用内联优化（`#[inline]`）
- 减少参数复制（使用引用）
- 预分配内存，避免动态分配
- 性能基准测试门禁
- 为关键函数提供 SIMD 优化版本

#### 风险 2: 类型系统不匹配
**影响**: DSL 类型与 Rust 类型转换复杂
**概率**: 低
**缓解措施**:
- Phase 1 仅支持简单标量类型
- 使用 trait 抽象类型转换
- 编写详细的类型转换测试

#### 风险 3: 契约验证不完整
**影响**: 运行时错误未被捕获
**概率**: 中
**缓解措施**:
- 编译期尽可能多地检查
- 运行时全面验证输入输出
- 提供详细的错误诊断
- 编写边界条件测试

#### 风险 4: 时间确定性难以保证
**影响**: 外部函数执行时间不可预测
**概率**: 中
**缓解措施**:
- 强制要求 `time_bound_us` 声明
- 运行时测量并记录超时
- 性能基准测试覆盖最坏情况
- 禁止阻塞操作和动态分配

### 12.2 工程风险

#### 风险 5: 开发周期延长
**影响**: 实施时间超出预期
**概率**: 中
**缓解措施**:
- 分阶段实施，每阶段独立交付
- Phase 1 聚焦最小闭环
- 及时调整范围，砍掉非必需功能
- 复用现有代码和工具

#### 风险 6: 向后兼容性问题
**影响**: 新语法破坏现有代码
**概率**: 低
**缓解措施**:
- 新增语法，不修改现有语法
- 外部函数是可选特性
- 保持现有测试全部通过
- 提供迁移指南

#### 风险 7: 文档不足
**影响**: 用户难以使用新特性
**概率**: 中
**缓解措施**:
- 每个 Phase 同步更新文档
- 提供丰富的示例代码
- 编写最佳实践指南
- 提供交互式教程

---

## 13. 成功标准

### 13.1 功能正确性
- ✅ 所有单元测试通过（100+ 个）
- ✅ 所有集成测试通过（20+ 个）
- ✅ 错误处理路径全覆盖
- ✅ 边界条件测试通过

### 13.2 性能指标
- ✅ 外部函数调用开销 < 1µs（简单函数）
- ✅ 外部函数调用开销 < 10µs（复杂函数）
- ✅ 相比内联计算，性能损失 < 20%
- ✅ 所有函数 P99 执行时间 < `time_bound_us`

### 13.3 验证能力
- ✅ DSL 形式化验证全部保留
- ✅ 契约验证覆盖所有外部函数
- ✅ 编译期检查捕获 90%+ 错误
- ✅ 运行时验证捕获剩余错误

### 13.4 开发体验
- ✅ 语法直观易懂
- ✅ 错误信息清晰准确
- ✅ 调试方便（支持追踪）
- ✅ 文档完整详细

### 13.5 可扩展性
- ✅ 容易添加新函数（< 30 分钟）
- ✅ 容易支持新类型（如需要）
- ✅ 架构清晰，模块解耦
- ✅ 代码可维护性高

---

## 14. 开放问题与决策点

### 14.1 已解决的问题

#### Q1: 外部函数声明放在哪个段？
**决策**: 放在 `[topology]` 段
**理由**:
- 与设备声明并列，语义一致
- 便于编译器收集所有声明
- 避免引入新的顶层段

#### Q2: 调用语法是 action 还是表达式？
**决策**: 作为 action，不能在表达式中调用
**理由**:
- 保持 DSL 简单，避免副作用混入表达式
- 便于时序分析和验证
- 与现有 action 语义一致

#### Q3: 错误处理用哪种方式？
**决策**: Phase 1 使用错误变量 + 显式检查（不引入 `on_error` 语法）
**理由**:
- 简单直观，易于实现
- 与现有 DSL 语法兼容
- 足够覆盖大部分场景
- Phase 2 可选引入 Result 类型

### 14.2 待决策的问题

#### Q4: 是否支持函数重载？
**选项**:
- A: 不支持，函数名必须唯一
- B: 支持，根据参数类型重载

**建议**: 选择 A（不支持）
**理由**:
- 简化编译器实现
- 避免类型推断复杂度
- 工业控制场景不常用重载

#### Q5: 是否支持可变参数？
**选项**:
- A: 不支持，参数数量固定
- B: 支持，使用 `...` 语法

**建议**: 选择 A（不支持）
**理由**:
- 简化类型检查
- 便于契约验证
- 可用数组参数替代（Phase 3）

#### Q6: 是否支持泛型函数？
**选项**:
- A: 不支持，类型具体化
- B: 支持，使用类型参数

**建议**: 选择 A（不支持）
**理由**:
- 避免类型推断复杂度
- 工业控制场景类型明确
- 可通过多个具体函数实现

#### Q7: 是否支持闭包/回调？
**选项**:
- A: 不支持
- B: 支持，允许传递函数指针

**建议**: 选择 A（不支持）
**理由**:
- 避免引入高阶函数复杂度
- 难以验证和分析
- 工业控制场景不常用

#### Q8: 非纯函数的状态如何管理？
**选项**:
- A: 使用 thread_local 全局状态
- B: 显式传递状态参数
- C: 引入状态对象

**建议**: Phase 1 使用 A，Phase 2 评估 C
**理由**:
- A 最简单，适合 POC
- C 更清晰，但需要引入对象系统
- B 增加用户负担

---

## 15. 最佳实践建议

### 15.1 外部函数设计原则

#### 原则 1: 单一职责
每个外部函数只做一件事，避免复杂的多功能函数。

**好的例子**:
```plc
extern function quadratic_fit(...) -> (float, float, float)
extern function evaluate_quadratic(a: float, b: float, c: float, x: float) -> float
```

**不好的例子**:
```plc
extern function fit_and_evaluate(...) -> float  # 做了两件事
```

#### 原则 2: 明确的契约
所有约束都应该在契约中明确声明。

**好的例子**:
```plc
extern function sqrt(x: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 10,
    input_range: { x: [0.0, 1e6] },
    output_range: { result: [0.0, 1e3] },
    error_condition: "x < 0 -> negative input"
}
```

**不好的例子**:
```plc
extern function sqrt(x: float) -> float {
    rust_module: "math::basic"
    # 缺少契约字段
}
```

#### 原则 3: 保守的时间上界
`time_bound_us` 应该是最坏情况的 2 倍，留有余量。

**好的例子**:
```plc
# 实测 P99 = 25µs，最坏 = 40µs
time_bound_us: 80  # 2x 最坏情况
```

**不好的例子**:
```plc
# 实测 P99 = 25µs
time_bound_us: 30  # 太紧，容易超时
```

#### 原则 4: 合理的输入范围
输入范围应该覆盖实际应用场景，但不要过于宽松。

**好的例子**:
```plc
# 温度传感器范围 -40°C ~ 120°C
input_range: { temp: [-50.0, 150.0] }  # 留 10°C 余量
```

**不好的例子**:
```plc
input_range: { temp: [-1e6, 1e6] }  # 过于宽松，失去保护作用
```

### 15.2 调用模式建议

#### 模式 1: 周期性计算
适用于需要定期更新的计算（如 PID 控制）。

```plc
task control_loop:
    step measure:
        wait data_ready == true
        delay: 10ms

    step compute:
        action: call pid_update(
            setpoint - sensor_value,
            kp, ki, kd, dt
        ) -> control_output
        delay: 5ms

    step apply:
        action: set_analog actuator control_output
        delay: 10ms
        goto measure  # 循环
```

#### 模式 2: 事件触发计算
适用于条件满足时才执行的计算（如曲线拟合）。

```plc
task data_processing:
    step wait_data:
        wait data_ready == true
        delay: 10ms

    step fit:
        action: call quadratic_fit(...) -> (a, b, c)
        delay: 10ms

    step use_result:
        if a > 0.0:
            goto success
        else:
            goto retry
```

#### 模式 3: 批量计算
适用于需要处理多个数据点的场景。

```plc
task batch_processing:
    step process_batch:
        action: call mean10(sample0, sample1, sample2, sample3, sample4, sample5, sample6, sample7, sample8, sample9) -> avg
        action: call std_dev10(sample0, sample1, sample2, sample3, sample4, sample5, sample6, sample7, sample8, sample9) -> std
        action: call min_max10(sample0, sample1, sample2, sample3, sample4, sample5, sample6, sample7, sample8, sample9) -> (min_val, max_val)
        delay: 20ms

    step check_quality:
        if std < threshold:
            goto accept
        else:
            goto reject
```

### 15.3 错误处理模式

#### 模式 1: 重试机制
```plc
task with_retry:
    step compute:
        action: call risky_function(...) -> result
        delay: 10ms
        goto check_error

    step check_error:
        if last_error != 0:
            goto retry
        else:
            goto success

    step retry:
        delay: 100ms  # 等待后重试
        if retry_count < 3:
            goto compute
        else:
            goto fatal_error
```

#### 模式 2: 降级处理
```plc
task with_fallback:
    step compute:
        action: call complex_function(...) -> result
        delay: 10ms
        goto check_error

    step check_error:
        if last_error != 0:
            goto fallback
        else:
            goto use_result

    step fallback:
        action: call simple_function(...) -> result
        delay: 5ms
        goto use_result
```

#### 模式 3: 错误记录
```plc
task with_logging:
    step compute:
        action: call function(...) -> result
        delay: 10ms
        goto check_error

    step check_error:
        if last_error != 0:
            goto log_error
        else:
            goto success

    step log_error:
        action: call log_error(last_error)
        delay: 5ms
        goto recovery
```

---

## 16. 参考资料

### 16.1 相关文档
- `hybrid_architecture_poc.md` - POC 实施指南
- `dsl_verification_boundary.md` - 形式化验证边界论证
- `dsl_computation_analysis.md` - DSL 计算能力分析报告
- `examples/quadratic_fitting.plc` - 二次拟合示例

### 16.2 技术参考
- [IEC 61131-3 标准](https://www.plcopen.org/) - PLC 编程语言标准
- [Rust FFI 指南](https://doc.rust-lang.org/nomicon/ffi.html) - Rust 外部函数接口
- [Beckhoff TwinCAT C++](https://infosys.beckhoff.com/) - 工业控制系统 C++ 集成
- [MATLAB Simulink External Functions](https://www.mathworks.com/help/simulink/external-functions.html) - 外部函数集成参考

### 16.3 学术参考
- "Formal Verification of Hybrid Systems" - 混合系统形式化验证
- "Contract-Based Design for Embedded Systems" - 基于契约的嵌入式系统设计
- "Real-Time Systems: Design Principles for Distributed Embedded Applications" - 实时系统设计原则

---

## 17. 总结

### 17.1 核心价值

混合架构方案通过**契约机制**实现了 DSL 控制逻辑与 Rust 复杂计算的安全集成：

1. **保持可验证性**: DSL 的形式化验证能力完全保留
2. **扩展计算能力**: 通过 Rust 外部函数支持复杂数值算法
3. **清晰的边界**: 控制平面（DSL）与计算平面（Rust）职责明确
4. **确定性保证**: 通过契约约束保证实时性和可预测性
5. **渐进式演进**: 分阶段实施，风险可控

### 17.2 关键设计决策

1. **外部函数作为 action**: 保持 DSL 简单，避免副作用混入表达式
2. **契约驱动验证**: 编译期 + 运行时双重保障
3. **标量类型优先**: Phase 1 聚焦简单类型，降低复杂度
4. **错误变量机制**: 简单直观的错误处理方式
5. **时间上界强制**: 保证硬实时约束

### 17.3 实施路线

- **Phase 1 (2-3 周)**: 最小闭环，验证技术可行性
- **Phase 2 (2-3 周)**: 契约与验证，增强安全性
- **Phase 3 (3-4 周)**: 生态完善，生产就绪

总工期：**7-10 周**

### 17.4 预期成果

完成后，RustPLC 将具备：
- ✅ 完整的外部函数调用能力
- ✅ 20+ 个内置数值算法函数
- ✅ 编译期 + 运行时契约验证
- ✅ 详细的文档与示例
- ✅ 生产级的性能与可靠性

这将使 RustPLC 成为**既可验证又实用**的工业控制系统开发平台。

---

## 附录 A: 完整示例

### A.1 温度控制系统（PID）

```plc
[topology]

# 外部函数声明
extern function pid_update(
    error: float,
    kp: float,
    ki: float,
    kd: float,
    dt: float
) -> float {
    rust_module: "control::pid",
    pure: false,
    time_bound_us: 20,
    input_range: {
        error: [-100.0, 100.0],
        kp: [0.0, 10.0],
        ki: [0.0, 10.0],
        kd: [0.0, 10.0],
        dt: [0.001, 1.0]
    },
    output_range: {
        output: [0.0, 100.0]
    }
}

# 设备声明
device temp_sensor: analog_input {
    range: 0.0..100.0,
    response_time: 50ms
}

device heater: analog_output {
    range: 0.0..100.0,
    response_time: 100ms
}

# 变量声明
variable setpoint: float = 25.0
variable temp_value: float = 0.0
variable temp_ready: bool = false
variable current_temp: float = 0.0
variable control_output: float = 0.0
variable error: float = 0.0

# PID 参数
variable kp: float = 2.0
variable ki: float = 0.5
variable kd: float = 0.1
variable dt: float = 0.01

[constraints]

[tasks]

task temperature_control:
    step measure:
        # 假设 temp_value 已由 IO 映射更新
        wait temp_ready == true
        compute current_temp = temp_value
        delay: 10ms

    step compute_error:
        compute error = setpoint - current_temp
        delay: 5ms

    step pid_control:
        action: call pid_update(error, kp, ki, kd, dt) -> control_output
        delay: 5ms

    step apply_control:
        action: set_analog heater control_output
        delay: 10ms
        goto measure

task error_handler:
    step log:
        # 记录错误
        delay: 10ms

    step reset:
        action: set_analog heater 0.0
        delay: 10ms
        goto temperature_control.measure
```

### A.2 曲线拟合系统

```plc
[topology]

# 外部函数声明
extern function quadratic_fit(
    x0: float, x1: float, x2: float, x3: float, x4: float,
    y0: float, y1: float, y2: float, y3: float, y4: float
) -> (float, float, float) {
    rust_module: "math::fitting",
    pure: true,
    time_bound_us: 50,
    input_range: {
        x0..x4: [-1e3, 1e3],
        y0..y4: [-1e6, 1e6]
    },
    output_range: {
        a: [-1e6, 1e6],
        b: [-1e6, 1e6],
        c: [-1e6, 1e6]
    },
    error_condition: "det ~ 0 -> singular matrix"
}

extern function evaluate_quadratic(
    a: float,
    b: float,
    c: float,
    x: float
) -> float {
    rust_module: "math::fitting",
    pure: true,
    time_bound_us: 10,
    input_range: {
        a: [-1e6, 1e6],
        b: [-1e6, 1e6],
        c: [-1e6, 1e6],
        x: [-1e3, 1e3]
    },
    output_range: {
        y: [-1e6, 1e6]
    }
}

# 数据点
variable x0: float = 0.0
variable x1: float = 1.0
variable x2: float = 2.0
variable x3: float = 3.0
variable x4: float = 4.0

variable y0: float = 0.0
variable y1: float = 0.0
variable y2: float = 0.0
variable y3: float = 0.0
variable y4: float = 0.0

# 拟合系数
variable coeff_a: float = 0.0
variable coeff_b: float = 0.0
variable coeff_c: float = 0.0

# 预测值
variable x_predict: float = 5.0
variable y_predict: float = 0.0

variable data_ready: bool = false
variable fit_success: bool = false

[constraints]

[tasks]

task data_collection:
    step collect:
        # 假设从传感器采集数据
        wait data_ready == true
        delay: 100ms

    step fit:
        action: call quadratic_fit(
            x0, x1, x2, x3, x4,
            y0, y1, y2, y3, y4
        ) -> (coeff_a, coeff_b, coeff_c)
        delay: 10ms
        goto check_fit

    step check_fit:
        if last_error != 0:
            goto fit_error
        else:
            goto predict

    step predict:
        action: call evaluate_quadratic(
            coeff_a, coeff_b, coeff_c, x_predict
        ) -> y_predict
        delay: 5ms

    step check_result:
        if y_predict > 0.0:
            goto success
        else:
            goto retry

task fit_error:
    step log:
        # 记录拟合失败
        delay: 10ms

    step retry:
        delay: 100ms
        goto data_collection.collect

task success:
    step done:
        allow_indefinite_wait: true
```

---

**文档版本**: v1.0
**最后更新**: 2026-02-27
**作者**: RustPLC Team
**审阅状态**: 待审阅
