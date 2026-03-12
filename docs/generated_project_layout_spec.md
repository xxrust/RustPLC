# RustPLC 生成项目目录约定

日期：2026-03-12

## 1. 目标

本约定用于规范 `rust_plc new` 以及后续 AI / CLI 生成的 PLC 项目骨架，使一个项目具备清晰的输入、派生、交付边界，而不是把 `.system.md`、`.plc`、场景、构建产物和一次性调试输出分散到多个无约束位置。

本约定回答三件事：

- 什么属于项目源码资产
- 什么属于可版本化运行资产
- 什么属于可删除、可重建的派生产物

## 2. 设计原则

- `plc/main.system.md` 与 `plc/main.plc` 是同一控制项目的双语义入口，必须同目录、同 basename。
- 运行输入与部署配置必须独立于 DSL 源码，避免把场景、I/O 接线、保持区配置混入 `.plc`。
- 所有编译/验证/仿真/代码生成/板级构建输出统一收敛到 `out/`，不再散落在项目根目录。
- 版本控制默认提交 `plc/`、`config/`、`scenarios/`、`docs/`；默认忽略 `out/`。
- 输出目录按生命周期划分，而不是按命令名临时命名。

## 3. 标准目录结构

```text
my_plc_project/
├── README.md
├── .gitignore
├── rustplc.project.toml
├── plc/
│   ├── main.system.md
│   └── main.plc
├── scenarios/
│   ├── nominal/
│   │   └── normal.yaml
│   ├── faults/
│   └── generated/
├── config/
│   ├── io_map.toml
│   └── retain.toml
├── docs/
│   └── project-layout.md
├── out/
│   ├── ir/
│   ├── sim/
│   ├── gate/
│   ├── codegen/
│   ├── rp2040/
│   └── release/
├── .github/
│   └── workflows/
└── .vscode/
```

## 4. 各目录职责

### 4.1 `rustplc.project.toml`

项目根级清单，用于固定该工程的默认入口与常用路径。

建议至少声明：

- 项目名与 slug
- 主 system 文件
- 主 PLC 文件
- 默认 nominal scenario
- 默认 `io_map` / `retain` 配置
- `out/` 下各生命周期目录

规则：

- 这是项目级事实源，不是临时缓存。
- 即使当前 CLI 仍允许显式传参，脚手架也应生成该文件，避免项目路径约定只存在于 README 文本里。

### 4.2 `plc/`

项目唯一的控制语义源。

- `main.system.md`：需求语义锚点，给人和 AI 看的系统级描述。
- `main.plc`：可编译、可验证、可运行的 DSL 主入口。

规则：

- 一个项目默认只有一对主入口文件。
- 如果未来支持多 PLC 包，仍应按 `plc/<package>/main.system.md` 与 `plc/<package>/main.plc` 成对出现。
- `.system.md` 不承载中间产物，不替代 `.plc`。

### 4.3 `scenarios/`

版本化的运行刺激与回归输入。

- `nominal/`：正常流程基线。
- `faults/`：故障与异常注入场景。
- `generated/`：`scenario-gen` 生成且希望保留的覆盖集。

规则：

- 场景应按意图归类，不再把所有 YAML 平铺在一个目录。
- `scenario-init` 生成的第一份骨架默认放到 `scenarios/nominal/normal.yaml`。

### 4.4 `config/`

部署和运行时配置，不属于 DSL 本体。

- `io_map.toml`：物理 I/O 映射。
- `retain.toml`：保持变量或持久化策略。

可继续扩展：

- `online_var_bindings.toml`
- `analog_calibration.toml`
- 板级或工厂特定配置文件

### 4.5 `docs/`

项目内文档，而不是编译器文档。

- `project-layout.md`：说明本项目目录规范与常用命令。
- 也可放 commissioning、handover、现场运行说明。

### 4.6 `out/`

所有可重建的派生产物统一输出目录。

- `out/ir/`：IR、验证摘要、语义快照等编译中间物。
- `out/sim/`：SIL trace、波形、仿真报告。
- `out/gate/`：`no-board-gate`、trace 对比、诊断产物。
- `out/codegen/`：ST 或其他代码生成输出。
- `out/rp2040/`：板级构建目录与模板配置。
- `out/release/`：可交付包、manifest、校验和。

规则：

- CLI 的 `--out` / `--out-dir` 默认推荐落到这些子目录。
- `out/` 内文件默认不纳入版本控制，除非项目明确需要冻结某份基线证据。

## 5. 推荐命名约定

- 主 PLC 文件固定为 `plc/main.plc`。
- 对应系统描述固定为 `plc/main.system.md`。
- 标准正常场景固定为 `scenarios/nominal/normal.yaml`。
- 正常仿真 trace 推荐输出到 `out/sim/normal/trace.jsonl`。
- 无板门禁基线推荐输出到 `out/gate/no_board/normal/`。
- ST 代码推荐输出到 `out/codegen/st/main.st`。

## 6. Git 约定

默认应提交：

- `plc/`
- `scenarios/`
- `config/`
- `docs/`
- `rustplc.project.toml`
- `.github/`
- `.vscode/`

默认应忽略：

- `out/**`

若项目需要冻结交付证据，建议复制到独立的版本化目录，而不是直接取消 `out/` 忽略规则。

## 7. 与现有 CLI 的映射

推荐命令路径：

```bash
cargo run --release --bin rust_plc -- scenario-validate \
  plc/main.plc --scenario scenarios/nominal/normal.yaml --output human

cargo run --release --bin rust_plc -- sim-plc \
  plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl

cargo run --release --bin rust_plc -- no-board-gate \
  plc/main.plc --scenario scenarios/nominal/normal.yaml \
  --out-dir out/gate/no_board/normal --output human

cargo run --release --bin rust_plc -- gen-st \
  plc/main.plc --out out/codegen/st/main.st

cargo run --release --bin rust_plc -- build-rp2040 \
  plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

## 8. 为什么不采用“所有文件平铺根目录”

因为那会让三类边界消失：

- 人工维护输入和机器生成输出混在一起
- 语义源文件和现场部署配置混在一起
- 长期回归资产和一次性调试垃圾混在一起

结果就是项目越用越乱，用户难以判断什么该改、什么能删、什么必须提交。

## 9. `rust_plc new` 的最低生成要求

脚手架至少应生成：

- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`
- `config/io_map.toml`
- `config/retain.toml`
- `rustplc.project.toml`
- `docs/project-layout.md`
- `.gitignore`
- `.github/workflows/no_board_gate.yml`
- `.vscode/*`

这样生成出来的就不再是“单文件 demo”，而是一个最小但完整的 PLC 工程。
