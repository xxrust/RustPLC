# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在本仓库中工作时提供指引。

## 项目简介

RustPLC 是一个面向工业控制系统的形式化验证编译器。它接收声明式 `.plc` DSL（描述物理拓扑、安全约束和控制逻辑），在编译期证明安全性、活性、时序和因果性。

## 常用命令

```bash
cargo build                                    # 调试构建
cargo build --release                          # 发布构建
cargo build --release --features z3-solver     # 启用 Z3 SMT 求解器
cargo test                                     # 全部测试（约 87 个）
cargo test --lib                               # 仅单元测试
cargo test --test examples_integration         # 集成测试
cargo test --test verification_capability      # 能力测试
cargo test <test_name>                         # 按名称运行单个测试
cargo run --release -- examples/two_cylinder.plc  # 编译一个 .plc 文件
```

机器可读的 IR JSON 输出到 stdout；人类可读的验证摘要输出到 stderr。

## 架构

编译流水线：`.plc` 源文件 → Parser (pest PEG) → AST → 预处理器 (repeat/delay 展开) → 语义分析 → IR → 四大验证引擎 → JSON IR 输出。

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

## DSL 结构

`.plc` 文件包含三个段：`[topology]`（设备与连接）、`[constraints]`（安全/时序/因果约束）、`[tasks]`（控制逻辑，以状态机步骤表达）。控制流语句包括：`action`、`wait`（支持 AND/OR）、`delay`、`timeout`、`if/else`、`goto`（task 或 task.step）、`repeat`、`parallel`、`race`、`allow_indefinite_wait`。

## 测试

`examples/` 目录下的 `.plc` 文件既是文档也是集成测试输入。错误用例文件（`error_all_verifiers.plc`、`error_missing_device.plc`）用于验证诊断输出。
