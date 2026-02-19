# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在本仓库中工作时提供指引。

## 项目简介

RustPLC 是一个面向工业控制系统的形式化验证编译器。它接收声明式 `.plc` DSL（描述物理拓扑、安全约束和控制逻辑），在编译期证明安全性、活性、时序和因果性。

**完整工作流**：编写/生成 .plc → 编译验证 → SIL 仿真 → 场景回归 → 无板门禁 / RP2040 部署 → release-bundle 交付。

## 常用命令

### 编译与验证
```bash
cargo build                                    # 调试构建
cargo build --release                          # 发布构建
cargo build --release --features z3-solver     # 启用 Z3 SMT 求解器
cargo run --release -- examples/two_cylinder.plc --no-print-ir  # 编译验证
```

### 测试
```bash
cargo test                                     # 全部测试（约 87 个）
cargo test --lib                               # 仅单元测试
cargo test --test examples_integration         # 集成测试
cargo test --test verification_capability      # 能力测试
cargo test <test_name>                         # 按名称运行单个测试
```

### SIL 仿真
```bash
cargo run --bin sim -- examples/two_cylinder.plc scenarios/two_cylinder.yaml
cargo run --bin sim -- examples/two_cylinder.plc scenarios/two_cylinder.yaml --wave wave.vcd
```

### 场景工程
```bash
cargo run --bin scenario-init -- examples/two_cylinder.plc --preset basic  # 生成场景模板
cargo run --bin scenario-validate -- scenarios/two_cylinder.yaml           # 验证场景语法
cargo run --bin scenario-expand -- scenarios/two_cylinder.yaml             # 展开 pulse/hold
cargo run --bin scenario-gen -- examples/two_cylinder.plc --seed 42 --count 10  # 批量生成
cargo run --bin sim-regress -- examples/two_cylinder.plc scenarios/batch/  # 批量回归
```

### RP2040 部署
```bash
# 交叉编译到 RP2040
cd crates/rp2040-runner
cargo build --release --target thumbv6m-none-eabi

# 生成 UF2 固件
elf2uf2-rs target/thumbv6m-none-eabi/release/rp2040-runner firmware.uf2

# trace 对比门禁
cargo run --bin trace-diff -- sil_trace.jsonl board_trace.jsonl --fail-on-mismatch
```

### 无板门禁
```bash
# 虚拟板级运行
cargo run --bin virtual-board -- examples/two_cylinder.plc scenarios/two_cylinder.yaml

# 实时性门禁
cargo run --bin no-board-gate -- \
  --sil-trace sil_trace.jsonl \
  --board-trace board_trace.jsonl \
  --tick-timing tick_timing.jsonl \
  --max-p99-exec-us 800 \
  --max-overrun-count 0

# 生成 release-bundle
cargo run --bin release-bundle -- \
  --plc examples/two_cylinder.plc \
  --verification-report verification_report.json \
  --sil-trace sil_trace.jsonl \
  --board-trace board_trace.jsonl \
  --timing-report timing_report.json \
  --output release-bundle/
```

机器可读的 IR JSON 输出到 stdout；人类可读的验证摘要输出到 stderr。

## 架构

完整架构见 README.md 的 ASCII 框架图。核心流程：

```
输入层: .plc DSL + scenario.yaml
  ↓
编译器核心: Parser (pest PEG) → AST → 语义分析 + 预处理 → IR (petgraph DiGraph)
  ↓
验证引擎 (并行): Safety (BMC + k-归纳) | Liveness (SCC) | Timing (关键路径) | Causality (BFS)
  ↓
运行时层: runtime-core (no_std) → SimIO / Virtual Board / RP2040 HAL
  ↓
分析门禁: trace-diff | timing-report | no-board-gate | release-bundle
  ↓
输出层: verification_report.json | trace.jsonl | firmware.uf2 | release-bundle/
```

### 目录结构

```
src/
├── main.rs              # CLI 入口
├── lib.rs               # 公共模块声明
├── parser/
│   ├── mod.rs           # pest 解析器 → AST
│   └── plc.pest         # PEG 语法定义
├── ast/mod.rs           # AST 类型（PlcProgram、DeviceDeclaration、StepStatement 等）
├── semantic/mod.rs      # 预处理器（repeat 展开）+ IR 降级（拓扑图、状态机、约束集、时序模型）
├── ir/mod.rs            # IR 类型（基于 petgraph 的 TopologyGraph、StateMachine、ConstraintSet、TimingModel）
├── error/mod.rs         # PlcError 枚举，结构化诊断（位置/原因/建议）
└── verification/
    ├── mod.rs           # 编排层 — 运行全部四个引擎
    ├── safety.rs        # BMC + k-归纳（可选 Z3）；检查 conflicts_with 和 requires
    ├── liveness.rs      # SCC 分析 + 可达性；检查死锁/活锁
    ├── timing.rs        # 关键路径分析；检查 must_complete_within 系列约束
    └── causality.rs     # 拓扑图 BFS；检查信号传播链

crates/
├── runtime-core/        # no_std 确定性状态机执行器
├── sim/                 # SIL 仿真运行时 (SimIO + Plant 模型 + 故障注入)
├── virtual-board/       # 虚拟板级 Runner (模拟真实板 + tick_timing 采样)
├── rp2040-runner/       # RP2040 HAL (GPIO/ADC/PWM/PIO 运动控制)
└── scenario-tools/      # 场景工程工具 (init/validate/expand/gen)

examples/                # .plc 示例文件（既是文档也是集成测试）
scenarios/               # 场景 YAML 文件
docs/                    # 详细设计文档 (playbook, PRD, 技术方案)
```

## 关键模式

**新增 StepStatement 变体**需要同步更新：解析器语法（`plc.pest`）、解析器到 AST 的降级（`parser/mod.rs`）、AST 类型（`ast/mod.rs`）、所有语义/验证模块的 `match` 分支，以及所有语法语句列表（`step_statement`、`parallel_branch_statement`、`race_branch_statement`）。

**语法糖**（repeat、delay）在语义预处理器中展开后再进行 IR 降级，验证引擎始终在展开后的程序上运行。

**解析器约定**：pest 有序选择规则中，长关键字必须排在短前缀之前（如 `must_complete_within_worst_case` 在 `must_complete_within` 之前）。匹配具体规则前需先解包 wrapper PEG 规则。

**拓扑方向**：`connected_to` 是上游链接（target → current），因果遍历沿物理信号流方向。因果验证会在拓扑图中补充 `detects.device → sensor` 逻辑边后再做可达性分析。

**parallel/race 上下文**：需标记语句来源，过滤跨分支的 action/wait 配对，避免因果性误报。

**时序**：两种变体用途不同 — `must_complete_within` 仅计算 action/delay 时间；`must_complete_within_worst_case` 包含 timeout 上界。需沿 `connected_to` 链累加上游 `response_time`。

**活性**：结合 AST 元数据（`allow_indefinite_wait`、`on_complete`）与 StateMachine 转换；仅靠 IR guard 无法重建所有 wait 豁免。

**错误处理**：使用 `PlcError` 构造器（`undefined_reference`、`type_mismatch`、`duplicate_definition`）保持诊断格式统一。所有语义构建器聚合错误，一次运行输出完整诊断。

**Serde**：DSL 枚举使用 `rename_all = "snake_case"` 和 tagged enum。TopologyGraph 序列化依赖 petgraph 的 `serde-1` feature。

**Z3**：置于 `z3-solver` cargo feature 之后，默认 `cargo test` 无需 cmake/libz3。

**no_std 兼容**：runtime-core 必须保持 no_std 兼容，以支持 RP2040 裸机部署。使用 `alloc` crate 而非 `std`。

**trace 格式**：所有运行时（SIL/Virtual Board/RP2040）输出统一的 JSONL trace 格式，每行一个 tick 的状态快照，用于 trace-diff 对比。

**场景驱动**：SIL 仿真和 Virtual Board 都使用 scenario.yaml 驱动输入，支持 `pulse` / `hold` 语法糖（在 scenario-expand 中展开）。

**实时性采样**：Virtual Board 和 RP2040 都会采样每个 tick 的执行时间（exec_us / slack_us），输出到 tick_timing.jsonl，用于 timing-report 统计和 no-board-gate 门禁。

**恢复模板**：关键 wait 语句必须可恢复（有 timeout 或 allow_indefinite_wait），否则 liveness 引擎会报 warning。急停/掉电/传感器卡死场景需要恢复模板。

**PID 运行时**：PID 控制器在 runtime-core 中实现为确定性状态机，每个 tick 更新一次。KPI 回归检查超调量、稳定时间、稳态误差。

**运动控制**：步进电机使用 RP2040 的 PIO 生成高速脉冲（最高 100kHz），AB 编码器使用 PIO 状态机解码。虚拟通道将编码器位置映射到 DI/AI，供控制逻辑读取。碰撞防护使用 zone_code + 双向互锁。

## DSL 结构

`.plc` 文件包含三个段：

### [topology]
设备声明与连接：
- `device <name>: <type>` - 声明设备（Cylinder, Sensor, Motor, Valve 等）
- `connected_to: [<upstream_devices>]` - 声明上游连接（信号流方向）
- `response_time: <ms>` - 设备响应时间
- `detects: <device>` - 传感器检测目标

### [constraints]
安全、时序、因果约束：
- `conflicts_with: [<devices>]` - 互斥约束（不能同时激活）
- `requires: [<devices>]` - 前置条件约束
- `must_complete_within: <ms>` - 时序约束（仅计算 action/delay）
- `must_complete_within_worst_case: <ms>` - 时序约束（包含 timeout 上界）
- `connected_to: [<devices>]` - 因果链约束

### [tasks]
控制逻辑（状态机步骤）：
- `action <device> <state> <duration_ms>` - 执行动作
- `wait <condition>` - 等待条件（支持 AND/OR 组合）
- `delay <ms>` - 延时
- `timeout <ms> { ... }` - 超时保护
- `if <condition> { ... } else { ... }` - 条件分支
- `goto <task>` / `goto <task>.<step>` - 跳转
- `repeat <count> { ... }` - 重复（语法糖，会展开）
- `parallel { ... }` - 并行分支
- `race { ... }` - 竞争分支（首个完成者胜出）
- `allow_indefinite_wait` - 豁免活性检查
- `on_complete: goto <task>` - 完成后跳转

### PID 控制
```
[topology]
device pid_temp: PidController {
  kp: 2.0,
  ki: 0.5,
  kd: 0.1,
  setpoint: 25.0,
  output_min: 0.0,
  output_max: 100.0
}

[tasks]
task control_loop {
  step update_pid {
    action pid_temp update 100
    wait pid_temp.output > 0.0
  }
}
```

### 运动控制（RP2040）
```
[topology]
device stepper_x: StepperMotor {
  step_pin: 2,
  dir_pin: 3,
  steps_per_rev: 200,
  max_speed_hz: 1000
}

device encoder_x: AbEncoder {
  a_pin: 4,
  b_pin: 5,
  ppr: 600
}

[tasks]
task move_x {
  step move_forward {
    action stepper_x forward 1000  # 1000 steps
    wait encoder_x.position >= 1000
  }
}
```

## 测试

`examples/` 目录下的 `.plc` 文件既是文档也是集成测试输入。错误用例文件（`error_all_verifiers.plc`、`error_missing_device.plc`）用于验证诊断输出。

### 测试分类

- **单元测试**：`cargo test --lib` - 各模块的单元测试
- **集成测试**：`cargo test --test examples_integration` - 编译所有 examples/*.plc
- **能力测试**：`cargo test --test verification_capability` - 验证引擎能力测试
- **SIL 仿真测试**：`cargo test -p sim` - 仿真运行时测试
- **场景工具测试**：`cargo test -p scenario-tools` - 场景工程工具测试
- **RP2040 测试**：需要实际硬件或 QEMU 模拟器

### 回归测试流程

1. 编译验证所有示例：`cargo test --test examples_integration`
2. SIL 仿真回归：`cargo run --bin sim-regress -- examples/ scenarios/`
3. 无板门禁：`cargo run --bin no-board-gate -- <args>`
4. 生成 release-bundle：`cargo run --bin release-bundle -- <args>`

## 文档

- **README.md** - 项目概览、架构图、快速开始
- **Wiki** (RustPLC.wiki/) - 完整技术文档（14 个页面）
  - Quick-Start.md - 5 分钟上手指南
  - DSL-Language-Reference.md - 完整 DSL 语法参考
  - Architecture.md - 编译流水线与模块结构
  - Verification-Engines.md - 四大验证引擎原理
  - SIL-Simulation.md - 仿真闭环指南
  - Scenario-System.md - 场景工程化指南
  - PID-Control.md - PID 控制指南
  - Motion-Control.md - 运动控制指南
  - RP2040-Deployment.md - RP2040 部署指南
  - No-Board-Gate.md - 无板门禁指南
  - Recovery-Templates.md - 恢复模板指南
  - Examples-Gallery.md - 示例文件详解
  - AI-Assisted-Generation.md - AI 辅助生成指南
  - Contributing.md - 开发指南
- **docs/** - 详细设计文档
  - scenario_playbook.md - 场景系统 playbook
  - scenario_gen.md - 场景生成技术方案
  - stepper_ab_encoder.md - 运动控制技术方案
  - prd.md - 产品需求文档

## 开发工作流

### 添加新功能

1. **更新 DSL 语法**（如果需要）
   - 修改 `src/parser/plc.pest`
   - 更新 `src/ast/mod.rs` 的 AST 类型
   - 更新 `src/parser/mod.rs` 的解析逻辑

2. **更新语义分析**
   - 修改 `src/semantic/mod.rs` 的 IR 降级逻辑
   - 更新 `src/ir/mod.rs` 的 IR 类型

3. **更新验证引擎**（如果需要）
   - 修改 `src/verification/*.rs` 的验证逻辑

4. **更新运行时**（如果需要）
   - 修改 `crates/runtime-core/` 的执行器
   - 修改 `crates/sim/` 的仿真运行时
   - 修改 `crates/rp2040-runner/` 的硬件层

5. **添加测试**
   - 在 `examples/` 添加示例 `.plc` 文件
   - 在 `scenarios/` 添加场景 YAML 文件
   - 添加单元测试和集成测试

6. **更新文档**
   - 更新 Wiki 相关页面
   - 更新 README.md（如果是重大功能）
   - 更新 CLAUDE.md（如果影响开发工作流）

### 调试技巧

- **查看 IR**：`cargo run -- examples/two_cylinder.plc` (不加 --no-print-ir)
- **查看 AST**：在 `src/main.rs` 中添加 `dbg!(&program);`
- **查看验证详情**：检查 stderr 输出的验证报告
- **查看 trace**：`cargo run --bin sim -- <plc> <scenario>` 生成 trace.jsonl
- **对比 trace**：`cargo run --bin trace-diff -- sil.jsonl board.jsonl`
- **查看时序统计**：`cargo run --bin timing-report -- tick_timing.jsonl`

## 常见问题

### 编译错误

- **pest 解析失败**：检查 `plc.pest` 语法规则顺序（长关键字在前）
- **类型不匹配**：检查 AST → IR 降级逻辑
- **petgraph 错误**：确保启用了 `serde-1` feature

### 验证失败

- **Safety 失败**：检查 conflicts_with / requires 约束
- **Liveness 失败**：检查是否有死锁或缺少 allow_indefinite_wait
- **Timing 失败**：检查 must_complete_within 约束和 response_time
- **Causality 失败**：检查 connected_to 链路和 detects 关系

### 仿真问题

- **trace 不匹配**：检查 Plant 模型逻辑和故障注入配置
- **实时性超限**：检查 tick_ms 配置和控制逻辑复杂度
- **场景语法错误**：运行 `scenario-validate` 检查 YAML 格式

### RP2040 部署问题

- **交叉编译失败**：确保安装了 `thumbv6m-none-eabi` target
- **固件无法运行**：检查 I/O 映射配置（io_map.toml）
- **trace 采集失败**：检查 RTT 连接和 probe-rs 配置
