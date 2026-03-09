# Agent Notes - RustPLC 开发指南

## 编译流水线架构

### Parser → AST → Semantic → IR → Verification/Codegen

编译流水线严格分层，每层职责清晰：

- **Parser** (`src/parser/plc.pest` + `src/parser/mod.rs`): PEG 语法 → AST。长关键字必须在短前缀之前（如 `must_complete_within_worst_case` 在 `must_complete_within` 之前）。Wrapper 规则必须先解包再匹配具体规则。布尔字面量与 `identifier` 并存时，`boolean_value` 必须放在 `identifier` 之前，避免 `true/false` 被误解析成标识符。
- **AST** (`src/ast/mod.rs`): 源语法的忠实表示。不进行语义验证或展开。
- **Semantic** (`src/semantic/mod.rs`): 预处理（repeat/delay/operation-template 展开）+ IR 降级。所有语法糖在 IR 生成前展开。设备库约束在降级前注入 AST。
- **IR** (`src/ir/mod.rs`): 规范中间形式（TopologyGraph + StateMachine + ConstraintSet + TimingModel）。验证引擎和运行时桥接消费 IR，不消费 AST。
- **Verification** (`src/verification/*.rs`): 四个引擎（safety, liveness, timing, causality）并行运行在预处理后的 IR 上。
- **Codegen** (`src/codegen/st.rs`): 从 IR 生成 ST 代码（Phase 1：不支持 parallel/race）。
- **Runtime Bridge** (`src/runtime_bridge.rs`): IR → runtime-core Program。强制 tick 对齐，解析 I/O 映射，验证 action/guard 支持。

### TransitionAction 变体更新

引入新的 `TransitionAction` 变体时，必须同时更新**所有**依赖层：

1. **AST** (`src/ast/mod.rs`): 添加 `ActionStatement` 变体
2. **Parser** (`src/parser/plc.pest` + `src/parser/mod.rs`): 添加语法规则 + 降级逻辑
3. **Semantic** (`src/semantic/mod.rs`): 在 `lower_action_statement` 中添加 IR 降级
4. **IR** (`src/ir/mod.rs`): 添加 `TransitionAction` 变体
5. **Verification** (`src/verification/safety.rs`, `causality.rs`, `timing.rs`): 添加匹配器
6. **Runtime Bridge** (`src/runtime_bridge.rs`): 添加 action 翻译到 `runtime_core::Action`
7. **Codegen** (`src/codegen/st.rs`): 添加 ST 代码生成（或用 `StCodegenError` 拒绝）
8. **Tests**: 在 `tests/examples_integration.rs` 和各模块中添加集成测试和单元测试

### AxisFault 语义对齐（US-003 起）

- `AxisFaultKind` / `AxisFaultCategory` 需在 **IR**（`src/ir/mod.rs`）与 **runtime-core**（`crates/runtime-core/src/lib.rs`）保持同名同语义（`reject/motion/safety/vendor`）。
- 语义降级必须通过 `lower_axis_fault_branch` 从 `kind` 推导 `category/vendor_code`，避免手工写死导致不一致。
- 运行时轴回调统一返回 `AxisMotionResult::Fault(AxisFault)`；测试优先使用 `AxisMotionResult::reject/motion_fault/safety_fault` 构造器保持语义稳定。
- `axis_fault_contract` 策略矩阵通过 **runtime bridge**（`src/runtime_bridge.rs`）降级到 `runtime_core::Program.axis_fault_policies`；调整策略枚举时需同步 IR/runtime-core/bridge 三层。
- 运行时策略审计日志通过 `runtime_core::axis_fault_policy_log_message_id` + `AXIS_FAULT_POLICY_LOG_MESSAGE` 生成；修改编码规则时必须同步 `tests/runtime_bridge_us006.rs` 回归断言。
- 停机模式语义在 runtime-core 中固定迁移为 `Running -> {ControlledStopping|QuickStopping|ImmediateStopping} -> Stopped`，并通过 `axis_stop_transition_log_message_id` + `AXIS_STOP_TRANSITION_{ENTER,COMPLETED}_LOG_MESSAGE` 输出可观测日志；修改 stop 编码或阶段名时需同步 `tests/runtime_bridge_us006.rs` 与 `crates/runtime-core/src/lib.rs` 单测。

## 外部函数架构

外部函数跨越**四层**：AST 声明 → IR 签名 → 语义验证 → 运行时执行。

### 声明与签名收集

- 外部函数声明在 `[topology]` 段：`extern_function <name> { rust_module: "...", params: [...], return_type: "...", time_bound_us: N, pure: true/false }`
- 语义签名收集（`src/semantic/mod.rs`）强制合约检查：
  - 禁止重复名称
  - `rust_module` 非空
  - `time_bound_us` 为正数
  - Phase-1 标量签名类型（int, real, bool）
  - 返回绑定类型检查对标 `[topology] variable` 声明
- 错误在调用点验证前暴露；使用 `PlcError::extern_*` 构造器保持一致性。

### 调用点验证与 IR 降级

- `action: call <name>(<args>) -> <bindings>` 在 `src/parser/mod.rs` 中解析
- 语义降级（`src/semantic/mod.rs`）：
  - 按名称解析函数
  - 参数类型检查对标签签名
  - 返回绑定类型检查对标声明变量
  - 降级为 `TransitionAction::CallExtern { function_id, args, bindings }`
- IR 携带 `ExternFunctionDef`（签名）+ `ExternCallBinding`（返回映射）

### 运行时执行

- `ExternFunctionRegistry`（`src/extern_functions.rs`）集中合约强制：
  - 内置数学外部函数：`add`, `multiply`, `quadratic_fit`
  - 内置控制外部函数：`pid_update`（有状态）
  - 自定义外部函数通过 `register_extern_function!` 宏注册
  - 参数计数/范围/超时/运行时错误全部集中
- 运行时桥接（`src/runtime_bridge.rs`）翻译 IR `CallExtern` → `runtime_core::Action::CallExtern`
- `Runtime::tick_with_extern` 执行外部调用；违规作为 `RuntimeTickError::ExternCallFailed` 暴露，包含嵌套 `ExternRuntimeError` 详情
- 测试使用 `with_time_source` 进行确定性超时断言；`reset_builtin_pid_state()` 用于 PID 回归

### 外部函数错误处理与回退

- DSL 级回退/重试，使用 `Runtime::tick_with_extern_error_code` 配合声明的 `[topology] variable last_error: int`
- 通过 `extern_runtime_error_code` 映射 `ExternRuntimeError` 到变量驱动分支（无需新 DSL 关键字）
- 调度器级护栏按阶段分割：
  - 非纯跨分支并发检查：语义任务验证（`src/semantic/mod.rs`）
  - Tick 预算强制（`tick_ms` vs 求和 `time_bound_us`）：运行时桥接（`src/runtime_bridge.rs`）

### 因果性与外部函数建模

- 因果链节点可引用设备、`[topology] variable` 和外部函数名称
- 在 `src/verification/causality.rs` 中：
  - 添加调用点边：`arg_vars -> function`
  - 仅将 `pure: true` 外部函数视为确定性变换：`function -> binding` 边
  - 非纯外部函数破坏因果链（需显式 `connected_to` 或 `detects`）

### 外部函数文档

- 保持两层文档：
  - `docs/extern_function_mvp_spec.md`：冻结的语法合约（字段/默认/非目标边界）
  - `docs/extern_function_development_guide.md`：推出指导、实际示例、迁移说明
- 通过将规范视为不可变来避免规范漂移；所有指导放在开发指南中
## 操作合约（EXOP）系统

操作合约声明设备级安全合约，具有确定性展开和多阶段验证。

### 声明与预处理

- 在 `[topology]` 段声明：`operation_contract <name> { kind: "...", device: "...", fields: {...} }`
- 解析器语法（`src/parser/plc.pest`）+ 降级（`src/parser/mod.rs`）必须保持同步
- 语义验证（`src/semantic/mod.rs`）强制 EXOP 规则（见下文）
- 预处理（`preprocess_program`）展开 `action: op.<device>.<template>(...)` 为具体 `action+wait+timeout->fault` 语句
- 展开是确定性的：相同合约 + 输入总是产生相同 IR

### EXOP 规则强制

规则跨语义和运行时阶段分割：

- **EXOP-001**（合约存在性）：合约名称必须存在；在语义验证中解析
- **EXOP-005/006**（路径唯一性）：沿 `driven_by` 追踪命令端点到恰好一个物理输出（Y*/AO*）；沿 `reports_to` 追踪反馈到恰好一个物理输入（X*/AI*）。在展开前在 `validate_operation_contract` 中强制。
- **EXOP-007**（超时对齐）：在语义中强制超时字段类型/正值检查；在运行时桥接中使用 `TopologyGraph.operation_contract_timeouts` 强制 `tick_ms` 对齐
- **EXOP-008**（安全状态兼容性）：在语义中验证合约类型-safe_state 兼容性；降级到 `TopologyGraph.operation_contract_safe_states`；让 stop/fault 任务降级合并这些动作作为默认值
- **EXOP-010/011**（桥接子集护栏）：EXOP-005/006 通过后，要求操作模板命令路径解析到数字输出（Y*），反馈解析到数字输入（X*），以便展开永不到达桥接时 `UnsupportedAction`/`UnsupportedGuardExpression`

### 模板展开

- `op.vacuum.on_and_confirm`：总是发出 `set+wait(true)+timeout_on_ms->fault`
- `op.vacuum.off`：发出 `set off`；仅当声明 `timeout_off_ms` 时追加 `wait(false)+timeout_off_ms->fault`
- `op.motor.move_to(contract, position)`：先解析 `motor_position` 合约 + `positions` 映射；发出 `set motor.run on + wait(sensor==true) + timeout_move_ms_default->fault + set motor.run off`
- 展开时字段/超时失败携带 EXOP 规则 ID（`EXOP-001`, `EXOP-007` 等）和行感知诊断

### 诊断与回归

- EXOP 诊断携带稳定负载：规则 id（`[EXOP-*]`）、合约名称、非零源行、非空修复建议
- 回归由 `tests/operation_contract_diagnostics_us012.rs` 和 `scripts/operation_contract_exop_gate.sh` 门禁

---

## 组件模型系统

组件拓扑和库提供声明式设备组合和约束注入。

### ComponentTopology 与 ComponentLibrary

- `ComponentTopology`（`src/component_topology.rs`）：组件实例 + 连接 + 标签规则的 JSON 模式
- `ComponentLibrary`（`src/component_library.rs`）：设备类型定义，包含接口（端口、状态）+ 设备约束（安全规则）
- 验证（`src/component_topology.rs::parse_component_topology_json`）：强制模式、端口兼容性、标签规则（danger_level, functional_group, location_group）
- 标签规则支持三种模式：`AllowAny`, `WithinOnly`, `CrossOnly` 用于功能/位置分组

### 设备库注入

- 设备库 TOML 文件（`devices/*.toml`）定义端口状态、默认状态和安全约束
- 语义预处理（`src/semantic/mod.rs::inject_device_constraints`）用库端口定义丰富 AST 设备
- 库约束在 IR 降级前展开为 AST 安全约束
- 仅当设备暴露引用端口时应用；库丰富，永不覆盖

### 验证与诊断

- 组件拓扑验证产生结构化问题，带代码（如 `CTOP-PARSE-001`, `CTOP-SCHEMA-*`）
- 设备库验证检查端口兼容性、约束语法、类型一致性
- 诊断包括路径（JSON 指针）、消息和代码用于机器可读处理

---

## 运行时桥接架构

运行时桥接（`src/runtime_bridge.rs`）翻译 IR → `runtime_core::Program`，严格验证。

### Tick 对齐与持续时间验证

- `tick_ms` 必须 > 0
- 所有 action/delay 持续时间必须对齐到 `tick_ms`（无余数）
- PID 循环 `period_ms` 必须对齐到 `tick_ms`
- 操作合约超时字段必须对齐到 `tick_ms`
- 违规作为 `BridgeError::*NotAligned` 暴露，带状态/持续时间/tick 上下文

### I/O 解析

- 数字/模拟输入/输出从拓扑通过 `PlcPortKind` 解析解析
- 无法解析的 I/O 作为 `BridgeError::Unresolvable{Digital,Analog}{Input,Output}` 暴露
- 模拟等待守卫需要状态机中的区域表；缺失表作为 `BridgeError::MissingAnalogRegions` 暴露

### Action 与守卫翻译

- `TransitionAction` 变体翻译为 `runtime_core::Action` 枚举
- 不支持的 action（Phase 1 中的 parallel, race）作为 `BridgeError::UnsupportedAction` 暴露
- 守卫表达式必须可翻译为 `runtime_core::ExprProgram`；不支持的守卫作为 `BridgeError::UnsupportedGuardExpression` 暴露

### 状态机约束

- 初始状态必须存在于状态列表中
- 所有转换目标必须解析到已知状态
- 转换形状必须被支持（无跨分支 action/wait 配对）
- 每 tick 最多 64 个转换在运行时强制

---

## 语义分析与预处理

语义分析（`src/semantic/mod.rs`）是 AST 和 IR 之间的桥梁。

### 预处理阶段

1. **PLC 控制器展开**（`expand_plc_controller_devices`）：展开 `device plc { ports: [...] }` 为单个数字/模拟 I/O 设备
2. **Repeat 展开**（`expand_repeat_blocks`）：展开 `repeat N { ... }` 为 N 份块副本
3. **操作模板展开**（`expand_operation_contract_actions`）：展开 `action: op.*` 为具体 action/wait/timeout 序列
4. **设备库注入**（`inject_device_constraints`）：注入库端口定义和安全约束

### IR 降级

- 拓扑降级：构建 `TopologyGraph`，包含设备、连接、外部函数、操作合约
- 任务降级：构建 `StateMachine`，包含状态、转换、守卫、动作
- `parallel` 合成状态命名约定固定为：`__parallel_<idx>_fork` → `__parallel_<idx>_branch_<n>_active` → `__parallel_<idx>_branch_<n>_done` → `__parallel_<idx>_join`（验证器/回归测试依赖该命名模式）
- 约束降级：构建 `ConstraintSet`，包含安全/时序/因果规则
- 所有降级对语义规则验证；错误聚合并一起暴露

### 验证门禁

- `topology_semantic_gate.rs`：SEM-101~108 检查（设备目的、端口一致性、I/O 映射）
- `sequence_lint.rs`：序列级检查（parallel/race 结构、action/wait 配对）
- 设备库验证：端口状态兼容性、约束语法

---

## 验证引擎

四个引擎在预处理后的 IR 上并行运行：

### 安全性（`src/verification/safety.rs`）

- BMC + k-归纳（可选 Z3 SMT 求解器）
- 检查 `conflicts_with` 和 `requires` 约束
- 有界模型检查到可配置深度
- Z3 特性门禁；默认测试仅使用 BMC

### 活性（`src/verification/liveness.rs`）

- SCC 分析 + 可达性
- 检测死锁/活锁
- 结合 AST 元数据（`allow_indefinite_wait`, `on_complete`）与 StateMachine 转换
- 需要 AST 上下文；IR 守卫单独不足以处理所有等待豁免

### 时序（`src/verification/timing.rs`）

- 关键路径分析
- 检查 `must_complete_within`（仅 action/delay）和 `must_complete_within_worst_case`（包含超时界）
- 沿 `connected_to` 链累积上游 `response_time`
- 两个变体用途不同；在约束中正确使用

### 因果性（`src/verification/causality.rs`）

- 沿物理信号流的拓扑 BFS
- 检查 `connected_to` 链和 `detects` 关系
- 在可达性前用 `detects.device → sensor` 逻辑边补充拓扑
- 外部函数建模为：`arg_vars -> function`（调用点）+ `function -> binding`（仅纯函数）

---

## 代码生成（ST）

ST 代码生成（`src/codegen/st.rs`）从 IR 生成 IEC 61131-3 ST 代码。

### Phase 1 限制

- 不支持 parallel/race（作为 `StCodegenError::ParallelNotSupported` / `RaceNotSupported` 暴露）
- 状态机展平为单个 `_state` 变量，数字 ID（步长 10）
- 计时器编码为 `_timer_*` 变量
- 规范化后的变量名冲突作为 `StCodegenError::VariableNameConflict` 暴露

### 配置

- `StCodegenConfig`：程序名、源文件、验证摘要包含
- 验证摘要包含安全/活性/时序/因果结果作为注释

### 错误处理

- 未解析的 goto 目标：`StCodegenError::UnresolvedGoto`
- 类型冲突：`StCodegenError::TypeConflict`
- 不支持的表达式：`StCodegenError::ExpressionNotSupported`

---

## 诊断系统

诊断（`src/diagnostics.rs`）提供结构化、可操作的错误消息。

### 错误类别

- **解析器错误**：语法违规，带行/列上下文
- **语义错误**：类型不匹配、未定义引用、约束违规
- **验证错误**：安全/活性/时序/因果失败，带反例
- **桥接错误**：I/O 解析、tick 对齐、action 支持问题
- **组件错误**：拓扑验证、库约束违规

### 诊断负载

- 错误代码（如 `SEM-101`, `EXOP-005`, `CTOP-PARSE-001`）
- 源位置（文件、行、列）
- 消息（人类可读）
- 修复建议（可操作指导）
- 上下文（相关代码片段、约束详情）

### 回归测试

- `tests/diagnostics_backend_doc_contract.rs`：验证诊断模式和负载结构
- `tests/io_snapshot_diagnostics.rs`：诊断输出快照测试
- 诊断代码必须稳定；破坏性变更需要迁移指南

---

## 测试策略

### 测试组织

- **单元测试**（`cargo test --lib`）：`src/**/*.rs` 中的模块级测试
- **集成测试**（`tests/*.rs`）：端到端测试，完整流水线
- **示例测试**（`tests/examples_integration.rs`）：编译所有 `examples/*.plc` 文件
- **回归测试**：诊断、trace diff、时序报告的快照测试

### 测试夹具

- 示例 PLC 文件（`examples/*.plc`）：30+ 文件覆盖所有 DSL 特性
- 场景 YAML 文件（`scenarios/*.yaml`）：SIL 仿真输入
- 设备库 TOML 文件（`devices/*.toml`）：设备定义
- 组件拓扑 JSON 文件：组件模型夹具

### 性能门禁

- `tests/extern_perf_gate_script.rs`：基准生产者 + 阈值配置
- `tests/extern_perf_bench_cli.rs`：CLI 基准运行器
- 基线快照 JSON + 脚本合约测试用于夹具注入

---

## 强制语义门禁

所有 CLI/集成夹具必须强制语义门禁：

- **SEM-101**：每个设备必须声明 `purpose` 元数据
- **SEM-102~108**：端口一致性、I/O 映射、设备库验证
- 遗留直接 `X0`/`Y0` 声明失败，错误 `SEM-107`/`SEM-108`
- PLC I/O 通过 `device ...: plc { ports: [...] }` 声明
- 违规在验证前暴露；无解决方案

---

## 文档分层

- **CLAUDE.md**：项目概览、常用命令、架构、DSL 结构、开发工作流
- **README.md**：快速开始、特性对比、系统架构、示例
- **Wiki**（`docs/wiki/`）：详细技术文档（14+ 页）
- **规范文档**（`docs/*_spec.md`）：冻结的语法合约（extern, operation-contract）
- **开发指南**（`docs/*_development_guide.md`）：推出指导、实际示例、迁移说明
- **AGENTS.md**（本文件）：面向 Agent 的实现模式和架构决策

---

## 关键代码库统计

- **源文件**：39 个 Rust 文件，~26K 行代码
- **测试**：66 个测试文件，319 个测试函数
- **示例**：30+ 个 `.plc` 示例文件
- **Crates**：6 个子项目（runtime-core, sim, codegen, io-traits, board-rp2040, web-server）
- **模块**：parser, ast, semantic, ir, verification (4 engines), codegen, diagnostics, component_*, device_*

---

## 常见开发模式

### 添加新 DSL 特性

1. 更新 `src/parser/plc.pest` 语法规则
2. 在 `src/parser/mod.rs` 中添加解析逻辑
3. 在 `src/ast/mod.rs` 中添加 AST 类型
4. 在 `src/semantic/mod.rs` 中添加 IR 降级
5. 在 `src/ir/mod.rs` 中添加 IR 类型
6. 在 `src/verification/*.rs` 中更新验证逻辑
7. 在 `src/runtime_bridge.rs` 中添加运行时翻译
8. 在 `src/codegen/st.rs` 中添加代码生成（或拒绝）
9. 添加集成测试和单元测试

### 添加新验证规则

1. 在 `src/verification/mod.rs` 中定义规则
2. 在相应引擎（safety/liveness/timing/causality）中实现检查
3. 添加诊断代码和修复建议
4. 在 `tests/` 中添加回归测试
5. 更新 AGENTS.md 和相关文档

### 修改轴参数层级资源

1. 轴资源链按 5 层维护：`axis_motor_classes` → `axis_families` → `axis_models` → `axis_configs` → `axis_motion_param_sets`
2. `axis_models/*.toml` 必须声明 `family_id`，`axis_configs/*.toml` 必须声明 `model_id`，`axis_motion_param_sets/*.toml` 必须声明 `config_id`
3. 设备声明只允许通过 `model_ref/config_ref/motion_param_set` 引用，禁止恢复旧的内联轴参数（会触发 `AXP-006`）
4. 层级 ID 一致性在 `src/axis_profile.rs` 集中校验（`AXP-007~AXP-010`）；新增字段时同步更新该模块与其单元测试
5. `axis.move_relative/absolute` 支持 `params` 参数集引用 + 动作内 `speed/acc/dec` 覆盖；解析规则在 `src/parser/plc.pest`，降级逻辑在 `src/parser/mod.rs`
6. 语义阶段会按优先级 `inline overrides > params 引用 > device.motion_param_set` 解析最终参数；缺失 `speed/acc/dec` 报 `AXIS-007`，`acc/dec` 超过 profile 上限报 `AXIS-009`

### 修改 I/O 映射

1. 在 `src/plc_port.rs` 中更新端口解析
2. 在 `src/runtime_bridge.rs` 中更新 I/O 解析逻辑
3. 在 `src/topology_semantic_gate.rs` 中更新验证
4. 添加集成测试验证映射

---

## 性能考虑

- **验证并行化**：四个验证引擎可并行运行；使用 `rayon` 或 `tokio` 进行并行化
- **IR 缓存**：预处理后的 IR 可缓存以加速迭代验证
- **增量验证**：仅重新验证受影响的约束（未实现，但可考虑）
- **Z3 超时**：配置 Z3 求解器超时以防止长时间验证

---

## 调试技巧

- **查看 IR**：`cargo run -- examples/two_cylinder.plc`（不加 `--no-print-ir`）
- **查看 AST**：在 `src/main.rs` 中添加 `dbg!(&program);`
- **查看验证详情**：检查 stderr 输出的验证报告
- **查看 ST 代码**：`cargo run --bin extern_perf_bench -- --output st.st`
- **查看诊断**：运行编译器并检查诊断输出格式

---

## 关键架构决策

### 为什么分层编译流水线？

- **清晰职责**：每层只做一件事，便于测试和维护
- **可组合性**：可独立测试每层，也可组合测试
- **可扩展性**：添加新特性时，只需在相应层添加逻辑
- **错误诊断**：错误在最早可能的层暴露，便于定位

### 为什么预处理而不是在 IR 中展开？

- **验证一致性**：所有验证引擎在展开后的程序上运行，避免重复处理
- **简化 IR**：IR 不需要处理语法糖，保持简洁
- **确定性**：展开是确定性的，便于调试和测试

### 为什么分离语义验证和运行时桥接？

- **关注点分离**：语义验证关注 DSL 正确性，桥接关注运行时可行性
- **独立测试**：可独立测试语义验证和桥接逻辑
- **多目标支持**：可为不同运行时（SIL、Virtual Board、RP2040）实现不同桥接

### 为什么使用组件模型？

- **设备库复用**：设备定义可跨项目复用
- **约束注入**：库可自动注入安全约束，减少手工编写
- **拓扑验证**：组件模型提供结构化验证，便于检查连接正确性

---
