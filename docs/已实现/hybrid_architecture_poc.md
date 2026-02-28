# 混合架构 POC 实施指南

## 概述

本文档提供混合架构概念验证（POC）的详细实施步骤，用于验证 DSL + Rust 混合架构的可行性。

## POC 目标

1. **验证技术可行性**：证明 DSL 可以安全地调用 Rust 函数
2. **测量性能开销**：量化函数调用的性能损耗
3. **评估开发体验**：评估开发者使用的便利性
4. **识别潜在问题**：发现实施过程中的技术障碍

## POC 范围

### 包含内容
- ✅ 最简单的 extern function 调用机制
- ✅ 基础的参数传递（标量类型）
- ✅ 简单的返回值处理
- ✅ 基本的错误处理
- ✅ 性能基准测试

### 不包含内容
- ❌ 数组支持（留待阶段2）
- ❌ 复杂类型（结构体、枚举）
- ❌ 完整的安全验证
- ❌ ST 代码生成

## 实施步骤

### 第1步：设计最小化语法（1天）

#### DSL 语法扩展

```plc
[topology]

# 声明外部函数
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
}

# 声明变量
variable x: float = 1.0
variable y: float = 2.0
variable result: float = 0.0

[tasks]

task main:
    step compute:
        # 调用外部函数
        action: call add(x, y) -> result
        delay: 10ms

    step check:
        if result > 2.5:
            goto success
        else:
            goto error
```

#### 语法要点

1. **extern function 声明**
   - 函数名
   - 参数列表（名称 + 类型）
   - 返回类型
   - Rust 模块路径

2. **call 语句**
   - 函数名
   - 参数（变量或字面量）
   - 返回值变量

### 第2步：扩展 AST（1天）

```rust
// src/ast/mod.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternFunctionDeclaration {
    pub line: usize,
    pub name: String,
    pub params: Vec<FunctionParam>,
    pub return_type: Option<FunctionType>,
    pub rust_module: String,
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
pub enum ActionStatement {
    // ... 现有变体 ...

    CallExtern {
        function_name: String,
        args: Vec<String>,  // 变量名或字面量
        return_var: Option<String>,
    },
}
```

### 第3步：扩展解析器（1天）

```rust
// src/parser/plc.pest

// 在 topology_entry 中添加
extern_function_declaration = {
    "extern" ~ "function" ~ identifier ~
    "(" ~ function_param_list? ~ ")" ~
    ("->" ~ function_type)? ~
    "{" ~ extern_function_attributes ~ "}"
}

function_param_list = {
    function_param ~ ("," ~ function_param)*
}

function_param = {
    identifier ~ ":" ~ function_type
}

function_type = { "float" | "int" | "bool" }

extern_function_attributes = {
    "rust_module" ~ ":" ~ string_literal
}

// 在 action_command 中添加
action_call_extern = {
    "call" ~ identifier ~
    "(" ~ call_arg_list? ~ ")" ~
    ("->" ~ identifier)?
}

call_arg_list = {
    call_arg ~ ("," ~ call_arg)*
}

call_arg = { identifier | number | boolean_value }
```

### 第4步：实现函数注册表（2天）

```rust
// crates/runtime-core/src/extern_functions.rs

use std::collections::HashMap;

/// 外部函数签名
pub type ExternFn = fn(&[f32]) -> Result<Vec<f32>, ExternError>;

/// 外部函数错误
#[derive(Debug, Clone)]
pub enum ExternError {
    InvalidArgCount { expected: usize, got: usize },
    InvalidArgType { arg_index: usize, expected: &'static str },
    RuntimeError(String),
}

/// 外部函数元数据
#[derive(Debug, Clone)]
pub struct ExternFunctionInfo {
    pub name: String,
    pub param_types: Vec<ParamType>,
    pub return_type: Option<ParamType>,
    pub rust_module: String,
    pub function: ExternFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
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

        // 注册内置函数
        registry.register_builtin_functions();

        registry
    }

    fn register_builtin_functions(&mut self) {
        // 注册简单的加法函数
        self.register(ExternFunctionInfo {
            name: "add".to_string(),
            param_types: vec![ParamType::Float, ParamType::Float],
            return_type: Some(ParamType::Float),
            rust_module: "math::basic".to_string(),
            function: math_add,
        });

        // 注册二次拟合函数
        self.register(ExternFunctionInfo {
            name: "quadratic_fit".to_string(),
            param_types: vec![
                ParamType::Float, ParamType::Float, ParamType::Float,
                ParamType::Float, ParamType::Float, ParamType::Float,
                ParamType::Float, ParamType::Float, ParamType::Float,
                ParamType::Float,
            ],
            return_type: Some(ParamType::Float),
            rust_module: "math::fitting".to_string(),
            function: math_quadratic_fit,
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
        args: &[f32],
    ) -> Result<Vec<f32>, ExternError> {
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

        // 调用函数
        (info.function)(args)
    }
}

// 内置函数实现

fn math_add(args: &[f32]) -> Result<Vec<f32>, ExternError> {
    if args.len() != 2 {
        return Err(ExternError::InvalidArgCount {
            expected: 2,
            got: args.len(),
        });
    }
    Ok(vec![args[0] + args[1]])
}

fn math_quadratic_fit(args: &[f32]) -> Result<Vec<f32>, ExternError> {
    // 参数：x0-x4 (5个), y0-y4 (5个)
    if args.len() != 10 {
        return Err(ExternError::InvalidArgCount {
            expected: 10,
            got: args.len(),
        });
    }

    let x = &args[0..5];
    let y = &args[5..10];

    // 调用实际的拟合算法
    match quadratic_fit_impl(x, y) {
        Ok((a, b, c)) => Ok(vec![a, b, c]),
        Err(e) => Err(ExternError::RuntimeError(e)),
    }
}

fn quadratic_fit_impl(x: &[f32], y: &[f32]) -> Result<(f32, f32, f32), String> {
    let n = x.len() as f32;

    // 累加
    let (sum_x, sum_x2, sum_x3, sum_x4) = x.iter()
        .fold((0.0, 0.0, 0.0, 0.0), |(sx, sx2, sx3, sx4), &xi| {
            let xi2 = xi * xi;
            (sx + xi, sx2 + xi2, sx3 + xi2 * xi, sx4 + xi2 * xi2)
        });

    let sum_y = y.iter().sum::<f32>();
    let sum_xy = x.iter().zip(y).map(|(xi, yi)| xi * yi).sum::<f32>();
    let sum_x2y = x.iter().zip(y).map(|(xi, yi)| xi * xi * yi).sum::<f32>();

    // 克拉默法则
    let det = n * sum_x2 * sum_x4 + sum_x * sum_x3 * sum_x2
            + sum_x2 * sum_x * sum_x3
            - sum_x2 * sum_x2 * sum_x2 - sum_x * sum_x * sum_x4
            - n * sum_x3 * sum_x3;

    if det.abs() < 1e-10 {
        return Err("Determinant too small".to_string());
    }

    let det_a = sum_y * sum_x2 * sum_x4 + sum_xy * sum_x3 * sum_x2
              + sum_x2y * sum_x * sum_x3
              - sum_x2y * sum_x2 * sum_x2 - sum_xy * sum_x * sum_x4
              - sum_y * sum_x3 * sum_x3;

    let det_b = n * sum_xy * sum_x4 + sum_x * sum_x2y * sum_x2
              + sum_x2 * sum_y * sum_x3
              - sum_x2 * sum_xy * sum_x2 - sum_x * sum_y * sum_x4
              - n * sum_x2y * sum_x3;

    let det_c = n * sum_x2 * sum_x2y + sum_x * sum_x3 * sum_y
              + sum_x2 * sum_x * sum_xy
              - sum_x2 * sum_x2 * sum_y - sum_x * sum_x * sum_x2y
              - n * sum_x3 * sum_xy;

    let a = det_a / det;
    let b = det_b / det;
    let c = det_c / det;

    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return Err("Result not finite".to_string());
    }

    Ok((a, b, c))
}
```

### 第5步：集成到运行时（2天）

```rust
// crates/runtime-core/src/lib.rs

pub struct Runtime<'a> {
    // ... 现有字段 ...
    extern_registry: &'a ExternFunctionRegistry,
}

impl<'a> Runtime<'a> {
    pub fn new(
        program: &'a Program,
        extern_registry: &'a ExternFunctionRegistry,
    ) -> Self {
        Self {
            // ... 现有初始化 ...
            extern_registry,
        }
    }

    fn execute_action<IO: Io>(
        &mut self,
        action: &TransitionAction,
        io: &mut IO,
    ) -> Result<(), RuntimeError> {
        match action {
            // ... 现有 action 处理 ...

            TransitionAction::CallExtern {
                function_name,
                arg_indices,
                return_var_index,
            } => {
                // 收集参数值
                let args: Vec<f32> = arg_indices
                    .iter()
                    .map(|&idx| self.variables[idx as usize])
                    .collect();

                // 调用外部函数
                let result = self.extern_registry
                    .call(function_name, &args)
                    .map_err(|e| RuntimeError::ExternFunctionError(
                        format!("{:?}", e)
                    ))?;

                // 存储返回值
                if let Some(return_idx) = return_var_index {
                    if let Some(&value) = result.first() {
                        self.variables[*return_idx as usize] = value;
                    }
                }

                Ok(())
            }
        }
    }
}
```

### 第6步：编写测试（1天）

```rust
// tests/extern_function_poc.rs

use rust_plc::*;
use runtime_core::*;

#[test]
fn test_simple_extern_function_call() {
    let plc_code = r#"
[topology]

extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
}

variable x: float = 1.0
variable y: float = 2.0
variable result: float = 0.0

device plc_main: plc {
    purpose: "test",
    ports: [X0:digital:consumer]
}

[constraints]

[tasks]

task main:
    step compute:
        action: call add(x, y) -> result
        delay: 10ms

    on_complete: goto done

task done:
    step wait:
        allow_indefinite_wait: true
"#;

    // 编译
    let program = compile_plc(plc_code).expect("should compile");

    // 创建函数注册表
    let registry = ExternFunctionRegistry::new();

    // 创建运行时
    let mut runtime = Runtime::new(&program, &registry);
    let mut io = TestIo::new();

    // 执行一个 tick
    runtime.tick(&mut io);

    // 验证结果
    let result_idx = program.find_variable("result").unwrap();
    assert_eq!(runtime.variables()[result_idx], 3.0);
}

#[test]
fn test_quadratic_fit_extern_function() {
    let plc_code = r#"
[topology]

extern function quadratic_fit(
    x0: float, x1: float, x2: float, x3: float, x4: float,
    y0: float, y1: float, y2: float, y3: float, y4: float
) -> (float, float, float) {
    rust_module: "math::fitting"
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
    let program = compile_plc(plc_code).expect("should compile");

    // 创建函数注册表
    let registry = ExternFunctionRegistry::new();

    // 创建运行时
    let mut runtime = Runtime::new(&program, &registry);
    let mut io = TestIo::new();

    // 执行一个 tick
    runtime.tick(&mut io);

    // 验证结果
    let a_idx = program.find_variable("a").unwrap();
    let b_idx = program.find_variable("b").unwrap();
    let c_idx = program.find_variable("c").unwrap();

    assert!((runtime.variables()[a_idx] - 0.5).abs() < 0.01);
    assert!((runtime.variables()[b_idx] - 2.0).abs() < 0.01);
    assert!((runtime.variables()[c_idx] - 1.0).abs() < 0.01);
}
```

### 第7步：性能基准测试（1天）

```rust
// benches/extern_function_benchmark.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_plc::*;
use runtime_core::*;

fn benchmark_extern_function_call(c: &mut Criterion) {
    let program = /* ... 编译好的程序 ... */;
    let registry = ExternFunctionRegistry::new();
    let mut runtime = Runtime::new(&program, &registry);
    let mut io = TestIo::new();

    c.bench_function("extern_function_call_add", |b| {
        b.iter(|| {
            runtime.tick(black_box(&mut io));
        });
    });
}

fn benchmark_inline_computation(c: &mut Criterion) {
    let program = /* ... 使用 compute 的程序 ... */;
    let mut runtime = Runtime::new(&program, &ExternFunctionRegistry::new());
    let mut io = TestIo::new();

    c.bench_function("inline_computation_add", |b| {
        b.iter(|| {
            runtime.tick(black_box(&mut io));
        });
    });
}

criterion_group!(benches, benchmark_extern_function_call, benchmark_inline_computation);
criterion_main!(benches);
```

## 成功标准

POC 被认为成功，如果：

1. ✅ **功能正确性**
   - 简单函数调用正常工作
   - 参数正确传递
   - 返回值正确接收
   - 错误处理正常

2. ✅ **性能可接受**
   - 函数调用开销 < 10µs
   - 相比内联计算，性能损失 < 20%

3. ✅ **开发体验良好**
   - 语法直观易懂
   - 错误信息清晰
   - 调试方便

4. ✅ **可扩展性**
   - 容易添加新函数
   - 容易支持新类型
   - 架构清晰

## 预期时间表

| 步骤 | 工作量 | 累计 |
|------|--------|------|
| 1. 设计语法 | 1天 | 1天 |
| 2. 扩展 AST | 1天 | 2天 |
| 3. 扩展解析器 | 1天 | 3天 |
| 4. 函数注册表 | 2天 | 5天 |
| 5. 运行时集成 | 2天 | 7天 |
| 6. 编写测试 | 1天 | 8天 |
| 7. 性能测试 | 1天 | 9天 |
| **总计** | **9天** | **~2周** |

## 风险和缓解

### 风险1：性能开销过大
**缓解**：
- 使用内联优化
- 减少参数复制
- 使用零成本抽象

### 风险2：类型系统不匹配
**缓解**：
- 从简单类型开始（float, int, bool）
- 逐步扩展到复杂类型
- 使用 trait 抽象类型转换

### 风险3：错误处理复杂
**缓解**：
- 使用 Result 类型
- 提供清晰的错误信息
- 添加调试日志

## 下一步

POC 成功后：

1. **评审和反馈**（1周）
   - 团队评审
   - 收集反馈
   - 调整设计

2. **完善设计文档**（1周）
   - 详细的技术规范
   - API 文档
   - 使用指南

3. **开始阶段1实施**（2-3周）
   - 完整的 extern function 支持
   - 完善的错误处理
   - 完整的测试覆盖

## 参考资料

- [Rust FFI 指南](https://doc.rust-lang.org/nomicon/ffi.html)
- [IEC 61131-3 外部函数](https://www.plcopen.org/)
- [Beckhoff TwinCAT C++ 集成](https://infosys.beckhoff.com/)
