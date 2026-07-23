# RustPLC 系统架构

> **最后更新**: 2026-07-06
> **版本**: v2.0
> **状态**: 稳定

---

## 目录

- [核心理念](#核心理念)
- [系统全景图](#系统全景图)
- [编译流水线](#编译流水线)
- [验证引擎](#验证引擎)
- [运行时系统](#运行时系统)
- [设备与组件模型](#设备与组件模型)
- [诊断与错误报告](#诊断与错误报告)
- [项目结构规范](#项目结构规范)

---

## 核心理念

RustPLC 不是传统的 PLC 编程工具，而是一个**形式化验证编译器**：

```
传统 PLC 工具链：编写 → 模拟 → 调试 → 现场测试 → 发现问题 → 返工
RustPLC 工具链：  编写 → 编译期证明 → 直接部署
```

### 三大支柱

1. **声明式 DSL**：用意图描述控制逻辑，而非手动编排状态机
2. **形式化验证**：数学证明安全性、活性、时序、因果性
3. **多目标生成**：从同一 IR 生成 ST 代码、嵌入式固件、仿真模型

---

## 系统全景图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              📝 输入层                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│  .plc DSL 文件              │  设备库 (TOML)      │  场景定义 (YAML)        │
│  - topology (拓扑)          │  - 气缸/电机/传感器  │  - 输入初始值           │
│  - constraints (约束)       │  - 安全约束         │  - 故障注入             │
│  - tasks (控制逻辑)         │  - 时序参数         │  - 预期输出             │
└────────────┬────────────────┴────────────────────┴─────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ⚙️ 编译器核心 (src/)                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Parser (PEG)  ──▶  AST  ──▶  Semantic Analysis  ──▶  IR                   │
│  plc.pest         忠实表示    - 预处理展开           规范中间表示            │
│  153K lines       语法结构    - repeat/delay         - TopologyGraph         │
│                               - 名称解析             - StateMachine          │
│                               - 设备库注入           - ConstraintSet         │
│                               367K lines             - TimingModel           │
│                                                      18K lines               │
└────────────┬────────────────────────────────────────────────────────────────┘
             │
             ├─────────────────┬─────────────────┬─────────────────┐
             ▼                 ▼                 ▼                 ▼
┌─────────────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│  🔬 Verification    │ │  🏃 Runtime  │ │  📦 Codegen  │ │  📊 Diagnostics│
│                     │ │   Bridge     │ │              │ │              │
│  4 并行引擎:        │ │              │ │  目标:       │ │  结构化报告:  │
│  • Safety (88K)    │ │  IR → runtime│ │  • ST 代码   │ │  • SEM-xxx   │
│  • Liveness (28K)  │ │  -core       │ │  • RP2040    │ │  • VER-xxx   │
│  • Timing (33K)    │ │  Program     │ │  • Renode    │ │  • GEN-xxx   │
│  • Causality (46K) │ │              │ │              │ │  • 修复建议   │
│                     │ │  强制检查:   │ │  49K lines   │ │              │
│  输出:              │ │  • 端口匹配  │ │              │ │              │
│  verification_      │ │  • 状态合法  │ │              │ │              │
│  report.json        │ │  • 动作序列  │ │              │ │              │
└─────────────────────┘ └──────────────┘ └──────────────┘ └──────────────┘
             │                 │                 │
             │                 ▼                 ▼
             │        ┌──────────────────────────────────┐
             │        │  🎯 运行时层 (crates/)          │
             │        ├──────────────────────────────────┤
             │        │  • runtime-core (no_std)        │
             │        │    确定性状态机执行器            │
             │        │  • sim (SIL 仿真)               │
             │        │  • board-rp2040 (嵌入式固件)    │
             │        │  • web-server (Web UI)          │
             │        └──────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          ✅ 输出层                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│  验证报告           │  可执行代码         │  部署包              │  仿真      │
│  - JSON 格式        │  - IEC 61131-3 ST  │  - 固件二进制         │  - 场景测试│
│  - 分级警告         │  - 嵌入式 Rust     │  - 配置文件           │  - KPI 回归│
│  - 修复建议         │  - Renode 脚本     │  - 文档               │  - 可视化  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 编译流水线

### 阶段分解

```rust
// 伪代码表示编译流程
fn compile(source: &str) -> Result<CompiledProgram, Diagnostics> {
    // 1. 词法 + 语法分析
    let ast = Parser::parse(source)?;

    // 2. 语义分析与预处理
    let preprocessed = SemanticAnalyzer::new()
        .resolve_names(&ast)?
        .expand_repeat_blocks(&ast)?
        .expand_delay_chains(&ast)?
        .inject_device_constraints(&ast, &device_library)?
        .validate_topology(&ast)?;

    // 3. IR 降级
    let ir = IRBuilder::lower(preprocessed)?;

    // 4. 并行验证（关键路径）
    let verification_results = tokio::join!(
        verify_safety(&ir),
        verify_liveness(&ir),
        verify_timing(&ir),
        verify_causality(&ir),
    );

    // 5. 运行时桥接
    let runtime_program = RuntimeBridge::translate(&ir)?;

    // 6. 代码生成
    let st_code = STCodegen::generate(&ir)?;

    Ok(CompiledProgram {
        ir,
        verification: verification_results,
        runtime_program,
        st_code,
    })
}
```

### 关键设计决策

| 层级 | 输入 | 输出 | 职责边界 |
|------|------|------|----------|
| **Parser** | `.plc` 文本 | AST | 只做语法解析，不验证语义 |
| **Semantic** | AST | 预处理后的 AST | 展开语法糖，名称解析，类型检查 |
| **IR Builder** | 预处理 AST | IR | 将 AST 降级为规范中间表示 |
| **Verification** | IR | 验证报告 | 数学证明，不修改 IR |
| **Runtime Bridge** | IR | runtime-core Program | 强制检查执行可行性 |
| **Codegen** | IR | 目标代码 | 只消费已验证的 IR，不补语义 |

---

## 验证引擎

### 四引擎架构

```
┌────────────────────────────────────────────────────────────────┐
│                    Verification Orchestrator                   │
│                    (并行调度 4 个引擎)                          │
└─────┬──────────────┬──────────────┬──────────────┬─────────────┘
      │              │              │              │
      ▼              ▼              ▼              ▼
┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
│  Safety  │  │ Liveness │  │  Timing  │  │Causality │
│  Engine  │  │  Engine  │  │  Engine  │  │  Engine  │
└──────────┘  └──────────┘  └──────────┘  └──────────┘
│ 88K lines│  │ 28K lines│  │ 33K lines│  │ 46K lines│
│          │  │          │  │          │  │          │
│ BMC +    │  │ SCC +    │  │ 关键路径 │  │ 拓扑 BFS │
│ k-归纳   │  │ 可达性   │  │ 分析     │  │          │
│          │  │ 分析     │  │          │  │          │
│ 证明:    │  │ 检测:    │  │ 验证:    │  │ 验证:    │
│ • 无碰撞 │  │ • 无死锁 │  │ • 响应时间│ │ • 信号链 │
│ • 互斥   │  │ • 无活锁 │  │ • 预算上界│ │ • 因果连接│
│ • 安全距离│ │ • 进度保证│ │ • 周期约束│ │ • 检测链路│
└──────────┘  └──────────┘  └──────────┘  └──────────┘
```

### Safety Engine

**技术**: 有界模型检测 (BMC) + k-归纳

**验证内容**:
- `conflicts_with`: 两个动作不能同时执行
- `requires`: 前置条件必须满足
- 安全距离约束
- 紧急停止路径可达性

**算法流程**:
```
1. 将约束转换为 SMT 公式
2. 使用 Z3 求解器检查可满足性
3. 如果找到反例，生成最短路径
4. 使用 k-归纳证明更大深度
```

### Liveness Engine

**技术**: 强连通分量 (SCC) 分解 + 可达性分析

**验证内容**:
- 所有任务可以完成（无死锁）
- 并行块可以汇合
- `repeat` 循环有出口
- 外部信号最终到达

**关键检查**:
- 每个状态至少有一个后继
- 终止状态可达
- 无自锁 SCC

### Timing Engine

**技术**: 关键路径分析 + 最坏情况执行时间 (WCET)

**验证内容**:
- `must_complete_within`: 任务在时限内完成
- `response_time`: 响应时间上界
- 周期约束满足
- `must_complete_within_worst_case`: 最坏情况分析

**计算模型**:
```
WCET(task) = max(path ∈ task.paths) { Σ step.duration + Σ wait.timeout }
```

### Causality Engine

**技术**: 拓扑排序 + BFS 路径搜索

**验证内容**:
- `connected_to`: 信号源到目标的路径存在
- `detects`: 传感器到控制器的链路完整
- 反馈环标记
- 断链检测

---

## 运行时系统

### Runtime-Core 架构

```rust
// runtime-core 的核心数据结构（简化）
pub struct Program {
    pub tasks: Vec<Task>,
    pub state: StateSnapshot,
}

pub struct Task {
    pub id: TaskId,
    pub steps: Vec<Step>,
    pub current_step: usize,
    pub status: TaskStatus,
}

pub struct Step {
    pub actions: Vec<Action>,      // 要执行的动作
    pub wait_condition: Condition, // 等待条件
    pub timeout_ms: Option<u32>,   // 超时
}

pub enum Action {
    SetOutput { port: u16, value: bool },
    SetAnalog { port: u16, value: f32 },
    Log { message: &'static str },
}

pub enum Condition {
    Always,
    InputHigh(u16),
    InputLow(u16),
    AnalogGreater { port: u16, threshold: f32 },
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
}
```

### 执行模型

```
每个扫描周期 (tick):
1. 读取所有输入
2. 对每个活动任务:
   a. 检查当前步骤的 wait_condition
   b. 如果满足，执行 actions 并进入下一步
   c. 如果超时，执行 on_timeout 分支
3. 写入所有输出
4. 更新时间戳
```

### 确定性保证

- **No Dynamic Allocation**: 所有内存在编译期分配
- **No Floating Point Non-determinism**: 固定精度整数运算
- **No Concurrent Mutation**: 单线程执行模型
- **Bounded Execution Time**: 每个周期的 WCET 可计算

---

## 设备与组件模型

### 三层架构

```
┌─────────────────────────────────────────────┐
│  Component Topology (JSON)                  │
│  设备实例 + 连接关系 + 资源边界              │
└─────────────┬───────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────┐
│  Device Library (TOML)                      │
│  设备类型定义 + 端口定义 + 约束模板          │
└─────────────┬───────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────┐
│  Device Semantics (Rust)                    │
│  动作语义 + 状态转换 + 验证规则              │
└─────────────────────────────────────────────┘
```

### 示例：气缸设备定义

```toml
# devices/cylinder.toml
[device]
name = "cylinder"
type = "actuator"
category = "pneumatic"

[ports]
extend = { type = "digital_output", direction = "out" }
retract = { type = "digital_output", direction = "out" }
extended_sensor = { type = "digital_input", direction = "in" }
retracted_sensor = { type = "digital_input", direction = "in" }

[constraints]
mutual_exclusion = ["extend", "retract"]  # 不能同时伸出和缩回
must_have_feedback = true                 # 必须有传感器反馈

[timing]
extend_max_ms = 2000
retract_max_ms = 2000

[safety]
requires_air_pressure = true
emergency_retract = true
```

### 约束注入流程

```
1. 编译器读取设备库定义
2. 在 Semantic Analysis 阶段注入约束:
   - mutual_exclusion → conflicts_with
   - must_have_feedback → connected_to
   - timing → must_complete_within
3. 验证引擎检查注入后的约束
```

---

## 诊断与错误报告

### 错误代码体系

| 前缀 | 类别 | 示例 |
|------|------|------|
| `SYN-` | 语法错误 | `SYN-001`: 未闭合的括号 |
| `SEM-` | 语义错误 | `SEM-101`: 未定义的设备 |
| `VER-` | 验证失败 | `VER-201`: Safety 冲突检测 |
| `GEN-` | 代码生成错误 | `GEN-301`: 不支持的目标平台 |
| `RUN-` | 运行时错误 | `RUN-401`: 端口映射失败 |

### 诊断消息格式

```json
{
  "code": "VER-201",
  "severity": "error",
  "message": "Safety conflict detected between motor_x and motor_y",
  "location": {
    "file": "examples/dual_axis.plc",
    "line": 45,
    "column": 5
  },
  "suggestion": "Add 'conflicts_with: motor_y' constraint to motor_x task",
  "related": [
    {
      "location": { "line": 38 },
      "message": "motor_y defined here"
    }
  ]
}
```

---

## 项目结构规范

### 标准项目分层

```
my_plc_project/
├── plc/
│   └── main.system.md              # 系统级意图描述
├── 00_topology/
│   ├── devices.plc                 # 设备定义
│   ├── connections.plc             # 连接关系
│   ├── workpieces.plc              # 工件位置
│   └── controller.plc              # 控制器配置
├── 01_init/
│   └── defaults.plc                # 初始化逻辑
├── 02_process/
│   ├── 10_feed_prep.plc            # 上料准备
│   ├── 20_orient_stage.plc         # 定位
│   └── 30_transfer_to_measure.plc  # 传送到检测
├── 03_constraints/
│   └── safety_timing.plc           # 安全与时序约束
├── 04_faults/
│   └── fault_handlers.plc          # 故障处理
├── process_model/
│   └── process_operation_model.toml # 工艺操作模型
├── config/
│   └── state_proof.toml            # 状态证明配置
├── scenarios/
│   └── nominal/
│       └── normal.yaml             # 测试场景
└── rustplc.bundle.toml             # 项目配置
```

### 分层设计原则

1. **00_topology**: 物理世界的结构化描述，不包含控制逻辑
2. **01_init**: 系统启动时的安全基线
3. **02_process**: 自动化生产流程
4. **03_constraints**: 跨任务的全局约束
5. **04_faults**: 异常路径与恢复策略
6. **process_model**: 拓扑与任务之间的调度意图
7. **config**: 机器可读的例外声明

---

## 关键指标

| 指标 | 数值 |
|------|------|
| 总代码行数 | 60K+ |
| 核心验证代码 | 195K lines (4 engines) |
| 测试用例 | 868 passing |
| 示例程序 | 30+ |
| 设备类型 | 15+ (气缸/电机/传感器/PID等) |
| 支持目标 | ST / RP2040 / Renode / SIL |
| 编译速度 | < 1s (典型项目) |
| 验证深度 | k=4~8 (BMC) |

---

## 扩展阅读

- [AGENTS.md](../AGENTS.md): 开发者快速上手指南
- [CODEX.md](../CODEX.md): 编译器核心设计文档
- [docs/wiki/](../docs/wiki/): 功能特性详细文档
- [examples/](../examples/): 参考示例程序

---

**最后更新**: 2026-07-06
**维护者**: RustPLC 核心团队
