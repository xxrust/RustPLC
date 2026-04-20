# RustPLC

Formally verified industrial control compiler and runtime pipeline.

[English](README_EN.md) | **中文**

RustPLC 的主链固定为：

`Parser -> AST -> Semantic -> IR -> Verification / Runtime Bridge / Codegen`

项目目标不是“再写一门 PLC 语法”，而是让工业控制意图能被：

- 明确建模
- 形式化验证
- 运行时执行
- 代码生成
- 追踪和诊断

## 快速开始

```bash
git clone https://github.com/xxrust/RustPLC.git
cd RustPLC
cargo build --release

# 最小可运行示例：编译并验证
cargo run --release --bin rust_plc -- examples/project_scaffold_demo/plc/main.plc --no-print-ir

# 校验场景
cargo run --release --bin rust_plc -- scenario-validate \
  examples/project_scaffold_demo/plc/main.plc \
  --scenario examples/project_scaffold_demo/scenarios/nominal/normal.yaml \
  --output human

# 无板门禁
cargo run --release --bin rust_plc -- no-board-gate \
  examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/normal.yaml \
  --out-dir out/gate/rp2040_motion_minimal \
  --output human

# 生成 IEC 61131-3 ST
cargo run --release --bin rust_plc -- gen-st \
  examples/dual_axis_platform.plc \
  --out out/codegen/dual_axis_platform.st
```

## 当前推荐示例

- `examples/project_scaffold_demo/plc/main.plc`
  适合看最小项目结构、编译、场景初始化和项目脚手架。
- `examples/rp2040_motion_minimal.plc`
  适合看 `scenario-validate`、`no-board-gate`、`geometry-export`、`trace-doctor`。
- `examples/dual_axis_platform.plc`
  适合看双轴运动语义、并发步骤和 ST 代码生成。
- `examples/three_station_assembly.plc`
  适合看较大规模拓扑与装配流程。
- `examples/recovery_templates/estop_recovery.plc`
  适合看恢复模板和异常分流。
- `examples/force_override_demo.plc`
  适合看在线强制与调试相关语义。

仓库中的失效示例、旧 trace 资产和围绕它们建立的过期阶段文档已经移除，不再作为有效入口。

## 项目脚手架

推荐直接生成结构化项目，而不是把 `.system.md`、`.plc`、场景和输出散落在仓库各处：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

典型目录结构：

```text
my_plc_project/
├── rustplc.project.toml
├── plc/
│   ├── main.system.md
│   └── main.plc
├── scenarios/
│   ├── nominal/
│   └── faults/
├── config/
├── docs/
└── out/
```

## 常用命令

```bash
# 生成场景骨架
cargo run --release --bin rust_plc -- scenario-init \
  examples/project_scaffold_demo/plc/main.plc \
  --out out/project_scaffold_demo.scenario.yaml \
  --preset normal

# 展开 pulse/hold 语法糖
cargo run --release --bin rust_plc -- scenario-expand \
  examples/project_scaffold_demo/plc/main.plc \
  --scenario examples/scenarios/pulse_hold.yaml \
  --out out/scenario.expanded.yaml

# 批量生成场景
cargo run --release --bin rust_plc -- scenario-gen \
  --plc examples/rp2040_motion_minimal.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir out/scenario_gen

# 生成 RP2040 交付产物
cargo run --release --bin rust_plc -- build-rp2040 \
  examples/rp2040_motion_minimal.plc \
  --out out/rp2040 \
  --io-map examples/rp2040_motion_minimal.io_map.toml

# 查看命令帮助
cargo run --release --bin rust_plc -- --help
cargo run --release --bin rust_plc -- help scenario-validate
```

## 文档入口

- `AGENTS.md`：项目总纲、分层原则、源码导航
- `docs/architecture/signal-direction.md`：并发 task / blocking step 的长期语义源
- `examples/project_scaffold_demo/README.md`：结构化项目示例
- `docs/wiki/PLC-Optimization-Pipeline.md`：优化管线概览
- `docs/wiki/Scenario-Assetization-Coverage-Feedback.md`：场景资产化与覆盖反馈

## 工程原则

- 语义必须先于实现
- IR 是唯一语义汇合点
- verification 是主路径，不是插件
- runtime 和 codegen 只能消费已经闭合的 IR 语义
- 文档、示例、测试、skills 必须与当前编译器契约同步

## 许可

本项目采用 [MIT License](LICENSE)。
