<p align="center">
  <h1 align="center">RustPLC</h1>
  <p align="center">
    <strong>形式化验证的工业控制编译器 / Formally Verified Industrial Control Compiler</strong><br>
    <em>声明物理拓扑与安全约束，编译器数学证明其正确性。</em>
  </p>
  <p align="center">
    <a href="https://github.com/xxrust/RustPLC/actions"><img alt="Build Status" src="https://img.shields.io/github/actions/workflow/status/xxrust/RustPLC/ci.yml?branch=main&style=flat-square"></a>
    <a href="https://crates.io/crates/rust-plc"><img alt="Crates.io" src="https://img.shields.io/crates/v/rust-plc.svg?style=flat-square"></a>
    <a href="https://github.com/xxrust/RustPLC/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square"></a>
    <a href="https://rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/Rust-1.75%2B-orange.svg?logo=rust&style=flat-square"></a>
  </p>
  <p align="center">
    <a href="README_EN.md">English</a> | <strong>中文</strong>
  </p>
</p>

---

## 🌟 为什么选择 RustPLC？

**传统方式**：工程师手写梯形图 ➔ 人工审查安全性 ➔ 现场调试发现碰撞、死锁或超时风险。

**RustPLC 方式**：工程师描述工艺 ➔ AI 生成声明式 DSL ➔ 编译期引擎进行**数学证明安全性** ➔ 问题在编译期**全部暴露**！

| 维度 | 🏭 传统 PLC | 🦀 RustPLC |
|------|----------|---------|
| 🛡️ **安全校验** | 规则校验 + 人工审查 | 编译期**四引擎**形式化验证 |
| 🐛 **问题暴露** | 现场联调期（成本极高） | 编译 / 仿真阶段提前拦截 |
| 📝 **变更审计** | 图形 Diff，难以追溯 | DSL 纯文本 Diff + Release Bundle |
| 🔄 **仿真回归** | 严重依赖特定厂商工具链 | SIL/Virtual-board 可脚本化批量回归 |
| 🔌 **硬件绑定** | 与独家厂商生态强耦合 | 拓扑与 I/O 映射解耦，原生支持 RP2040 等 |

---

## 🚀 快速开始

### 1. 安装与运行

```bash
# 克隆仓库
git clone https://github.com/xxrust/RustPLC.git
cd RustPLC

# 编译项目
cargo build --release

# 运行基础示例
cargo run --release --bin rust_plc -- examples/two_cylinder.plc --no-print-ir
```

<details>
<summary><strong>✅ 查看验证通过的输出示例 (点击展开)</strong></summary>

```text
验证通过：
  - Safety:    完备证明（深度 4）— conflicts_with 全部满足
  - Liveness:  通过 — 无死锁风险
  - Timing:    通过
  - Causality: 通过 — 所有信号链路连通
```

</details>

### 2. 推荐示例入口
您可以从以下示例开始探索：
- 🟢 `examples/two_cylinder.plc` — 最小可运行示例，适合初学者。
- 🏭 `examples/assembly_station.plc` — 大型拓扑，展示复杂逻辑。
- 🚨 `examples/recovery_templates/estop_recovery.plc` — 紧急停止与恢复模板。
- 🛠️ `examples/force_override_demo.plc` — 在线强制信号与调试演示。
- 📦 `examples/project_scaffold_demo/` — 完整项目脚手架示例，展示 `system/plc/scenario/config/out` 的组织方式。

> ⚠️ **强制审核规则（自 2026-02-24 起）**：每个 `device` 必须声明 `purpose`，缺失将直接导致 Semantic Gate 校验失败。
>
> 🔄 **拓扑约束说明**：旧版 `connected_to` 与“端口直接充当设备”的写法不再作为推荐兼容路径；遇到旧工程时，应先迁移到当前拓扑/端口建模，再继续编译与生成。

---

## 📦 项目脚手架（推荐）

如果你不想把 `.system.md`、`.plc`、场景、I/O 映射和中间产物散落在各处，推荐直接生成完整项目骨架：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

生成后的项目结构：

```text
my_plc_project/
├── README.md
├── .gitignore
├── rustplc.project.toml
├── plc/
│   ├── main.system.md
│   └── main.plc
├── scenarios/
│   ├── nominal/normal.yaml
│   ├── faults/
│   └── generated/
├── config/
│   ├── io_map.toml
│   └── retain.toml
├── docs/project-layout.md
├── out/
│   ├── ir/
│   ├── sim/
│   ├── gate/
│   ├── codegen/
│   ├── rp2040/
│   └── release/
├── .github/workflows/no_board_gate.yml
└── .vscode/
```

关键点：

- `plc/main.system.md` 与 `plc/main.plc` 同目录、同 basename，分别承载系统语义和 DSL 源码。
- `scenarios/`、`config/` 与 `plc/` 分层，避免把运行输入和部署配置混进 DSL。
- 所有可重建产物统一进入 `out/`，不再把 trace、codegen、build 结果平铺在项目根目录。
- 根级 `rustplc.project.toml` 固定项目名、主入口和默认输出路径。

推荐第一天命令：

```bash
cargo run --release --bin rust_plc -- scenario-validate plc/main.plc \
  --scenario scenarios/nominal/normal.yaml --output human

cargo run --release --bin rust_plc -- no-board-gate plc/main.plc \
  --scenario scenarios/nominal/normal.yaml \
  --out-dir out/gate/no_board/normal --output human

cargo run --release --bin rust_plc -- gen-st plc/main.plc \
  --out out/codegen/st/main.st
```

### CLI Help

- `cargo run --release --bin rust_plc -- --help`：查看顶层命令列表。
- `cargo run --release --bin rust_plc -- help <command>`：查看某个子命令的完整帮助页。
- `cargo run --release --bin rust_plc -- <command> --help`：与上一条等价，适合直接在命令后补 `--help`。
- 默认编译模式也支持帮助页：`cargo run --release --bin rust_plc -- help compile`
- 示例：

```bash
cargo run --release --bin rust_plc -- help sim-plc
cargo run --release --bin rust_plc -- scenario-validate --help
```

进一步说明：

- 目录约定文档：`docs/已实现/generated_project_layout_spec.md`
- 脚手架说明：`docs/已实现/developer_bootstrap_pack.md`
- 仓库内完整示例：`examples/project_scaffold_demo/`

正式项目的需求入口固定为 `plc/main.system.md`。
`examples/*.system.md` 只用于示例与回归夹具。
`docs/patent_collected/**` 与 `docs/web_collected/**` 属于研究资料，不应当作项目正式入口。

---

## 🏗️ 系统架构

RustPLC 提供从编译、验证到仿真的全链路支持。

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                                  📝 输入层                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│  .plc DSL 文件                         场景 YAML (scenario.yaml)                    │
│  - topology / constraints / tasks      - digital_inputs / analog_inputs             │
│  - extern fn 声明 + 调用               - tick_ms / fault injection                  │
└────────────────┬────────────────────────────────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              ⚙️ 编译器核心 (src/)                                   │
├─────────────────────────────────────────────────────────────────────────────────────┤
│  Parser (pest PEG) ──▶ AST ──▶ 语义分析 + 预处理 ──▶ IR                            │
│                                (repeat/delay 展开)   (TopologyGraph + StateMachine) │
│                                                                                      │
│  DSL 编译链：parser/plc.pest → ast/ → semantic/ → ir/                              │
│  设备模型：  device_library.rs (devices/*.toml)  device_subtype.rs                 │
│  语义门禁：  topology_semantic_gate.rs (SEM-101~107)  sequence_lint.rs             │
│  I/O 模型：  iec_address.rs  plc_port.rs                                           │
│  元件链路：  component_{library,topology,scenario,sim,faults,diagnostics}.rs       │
│  Extern 解析：extern_functions.rs — 签名校验 / 合约注入 / tick 预算检查            │
│  运行时支撑：diagnostics.rs  alarm_runtime.rs                                      │
└────────────────┬────────────────────────────────────────────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────────────┐  ┌──────────────────────────┐
│          🔬 验证引擎（并行执行）(src/verification/)   │  │  🦀 Rust 计算平面        │
├──────────────────────────────────────────────────────┤  │  (extern functions)      │
│  Safety    (BMC + k-归纳)   conflicts_with / requires │  ├──────────────────────────┤
│  Liveness  (SCC + 可达性)   死锁 / 活锁检测           │  │  ┌──────────────────┐   │
│  Timing    (关键路径)       response_time 上界        │◀─┤  │  数值算法         │   │
│  Causality (BFS)            connected_to 链路         │  │  │  (拟合 / 统计)    │   │
│  Extern    non-pure 冲突 / tick 预算超限检查          │  │  └──────────────────┘   │
│                    ▼                                  │  │  ┌──────────────────┐   │
│      verification_report.json                        │  │  │  线性代数         │   │
│      (结构化报告 + warnings 分级)                     │  │  │  (矩阵运算)       │   │
└────────────────┬─────────────────────────────────────┘  │  └──────────────────┘   │
                 │                                         │  ┌──────────────────┐   │
                 ▼                                         │  │  优化求解         │   │
┌──────────────────────────────────────────────────────┐  │  │  (PID / MPC)      │   │
│                  🏃 运行时层 (crates/)                │  │  └──────────────────┘   │
├──────────────────────────────────────────────────────┤  │                          │
│        runtime-core (no_std 确定性状态机执行器)       │  │  注册：                  │
│               │              │             │          │◀─┤  ExternFunctionRegistry  │
│        SimIO (sim)    Virtual Board   RP2040 HAL      │  │  合约：                  │
│        SIL 仿真 I/O   虚拟板级 Runner  GPIO/ADC/PWM   │  │  deterministic / pure    │
│        Plant/故障注入  tick_timing采样  PIO/RTT日志   │  │  range / timeout         │
│               │              │             │          │  │  验证：                  │
│        sil_trace.jsonl  board_trace.jsonl  firmware   │  │  单元测试 + perf bench   │
│        alarm_events     tick_timing.jsonl  board.log  │  │  + 数值稳定性分析        │
└────────────────┬─────────────────────────────────────┘  └──────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                          📊 分析与门禁 (src/)                                        │
├─────────────────────────────────────────────────────────────────────────────────────┤
│  trace-diff        timing-report        no-board-gate       release-bundle          │
│  trace-doctor      sequence-lint        commissioning-run   pil-run                 │
│  extern-perf-gate  component-topology-validate/diff         component-sim           │
│  component-scenario-validate            io-map-normalize                            │
└────────────────┬────────────────────────────────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                                📦 输出层                                             │
├─────────────────────────────────────────────────────────────────────────────────────┤
│  编译期：verification_report.json + SEM-10x 语义门禁报告                            │
│  仿真期：trace.jsonl + wave.vcd + sim_report.json + alarm_events.ndjson            │
│  诊断期：diagnosis_report.json + component_diagnosis.json + io_snapshot.json       │
│  部署期：firmware.uf2 + io_map.toml                                                 │
│  门禁期：diff_report.json + timing_report.json + gate_summary.json                 │
│  交付期：release-bundle/ (manifest.json + SHA256 清单 + git 元数据 + 所有工件)      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

---

## ⚙️ 核心能力概览

| 功能领域 | 核心能力说明 |
|------|------|
| **🛠 代码生成** | `gen-st` 将验证后的 IR 完美编译为 IEC 61131-3 ST 代码；内置 `iec2c` 在 CI 闭环验证语法。 |
| **🛡 数学证明** | Safety / Liveness / Timing / Causality 四大引擎，实现**编译期形式化严密证明**。 |
| **🚧 严格门禁** | SEM-101~107 涵盖端口/类型/角色/subtype 校验，且强制审核 `purpose`。 |
| **🤖 AI 与自动化** | 支持自然语言 ➔ AI 多轮对话生成 `.plc` ➔ 自动编译验证的现代工作流。 |
| **🧪 深度仿真** | 提供场景驱动的确定性 SIL 仿真，支持故障注入、VCD 波形导出、批量回归测试。 |
| **🧩 拓展生态** | ComponentLibrary 支持元件维度的拓扑、场景、仿真、诊断全链路。 |
| **🩺 智能诊断** | 内置五类诊断引擎与证据锚点排序，并通过 WebSocket 推送告警 (AlarmDispatcher)。 |
| **🚀 硬件部署** | 交叉编译直出 RP2040 `.uf2` 固件，支持 I/O 映射与物理 Trace 门禁比对。 |
| **🎛 高阶控制** | 支持 PID 闭环稳定回归、步进+AB编码器运动控制、PIO 高速脉冲及碰撞安全防护。 |

---

## 🔄 典型工作流

这里展示了一个标准自动化开发周期：

### 1. 编写或生成逻辑
你可以通过 **MCP 服务器 + AI 对话生成**（推荐，零配置）：

```bash
# 1. 安装 MCP 依赖
pip install mcp

# 2. 构建编译器
cargo build --release

# 3. 在 Claude Code 中直接对话（.mcp.json 已预配置）
# "帮我生成一个双缸顺序动作的 PLC 程序"
# Claude Code 会自动调用 MCP 服务器，执行四阶段生成流程，并验证结果
```

> 详见 [rustplc-mcp/QUICKSTART.md](rustplc-mcp/QUICKSTART.md)

或者 **纯手写 DSL**：
<details>
<summary><strong>点击查看 DSL 示例代码</strong></summary>

```plc
[topology]
device plc_main: plc {
    purpose: "控制器本体与工艺 I/O 端口映射",
    ports: [Y0:digital:producer, X0:digital:consumer]
}
device valve_A: solenoid_valve {
    purpose: "控制A缸主气路通断",
    response_time: 20ms,
    ports: [coil:digital:consumer, out:pneumatic:producer]
}
device cyl_A: cylinder {
    purpose: "A工位执行缸，负责伸缩动作",
    stroke_time: 300ms,
    ports: [cmd:pneumatic:consumer, extended:logical:producer]
}
device sensor_A_ext: sensor { purpose: "采集A缸伸出到位信号" }

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: plc_main.X0, via: reports_to }

[constraints]
safety: cyl_A.extended requires sensor_A_ext.on

[tasks]
task cycle:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto fault_handler
```
</details>

<br>

### 2. 仿真与验证
```bash
# 场景仿真：录制波形并生成回归 Trace
cargo run --release --bin rust_plc -- sim-plc examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out trace.jsonl

# 无板交付门禁：校验实时性能与时序表现
cargo run --release --bin rust_plc -- no-board-gate examples/assembly_station.plc \
  --scenario scenarios/normal.yaml \
  --out-dir out/gate \
  --max-p99-exec-us 500 \
  --max-overrun-count 0
```

### 2.5 PLC Optimization (library API)

当前优化能力以库 API 形式提供，不是独立 CLI 子命令。它直接复用现有 semantic、timing 和 verification 流水线，不额外发明一套 legality 规则。
详细说明见 `docs/wiki/PLC-Optimization-Pipeline.md`。

```rust
use rust_plc::optimization::optimize_plc_source;

let source = std::fs::read_to_string("examples/two_cylinder.plc")?;
let candidates = optimize_plc_source(&source)?;

for candidate in candidates.iter().take(3) {
    println!(
        "{} legal={} nominal_ms={} rewrite={}",
        candidate.id,
        candidate.legality.is_legal,
        candidate.timing.global_nominal_ms,
        candidate.rewrite.summary
    );
}
```

<br>

### 3. 硬件部署与发布交付
```bash
# 一键编译 RP2040 固件并烧录
cargo run --release --bin rust_plc -- build-rp2040 examples/assembly_station.plc \
  --out out/rp2040 --io-map io_map.toml --emit-uf2 out/firmware.uf2

# 自动化发行包生成
cargo run --release --bin rust_plc -- release-bundle examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out-dir out/release
```

---

## 📚 开发文档与资源

当前以仓库内文档为准：

- `AGENTS.md` - 项目总纲、源码导航、跨层改动联动路径
- `docs/architecture/signal-direction.md` - 并发 task / blocking step 的长期语义源
- `docs/已实现/generated_project_layout_spec.md` - `rust_plc new` 项目目录约定
- `docs/已实现/developer_bootstrap_pack.md` - Day-1 项目初始化与脚手架说明
- `docs/已实现/extern_function_mvp_spec.md` - extern function 冻结合同
- `docs/已实现/extern_function_development_guide.md` - extern function 落地指南
- `docs/已实现/semantic_resource_interlock_spec.md` - 资源互锁规范
- `docs/已实现/semantic_resource_interlock_development_guide.md` - 资源互锁开发指南
- `docs/已实现/workpiece_to_st_codegen_policy.md` - 工件语义进入 ST 的边界
- `docs/已实现/轴配置/concurrent_runtime_migration_guide.md` - 并发 runtime 迁移说明
- `docs/已实现/轴配置/concurrent_runtime_e2e_acceptance_baseline.md` - 并发 runtime 验收基线

`docs/wiki/` 中仍保留少量 repo-local wiki 草稿用于离线阅读，但它们不是项目权威规范。

---

## 🌍 AI for AI 方向

RustPLC 的下一阶段目标，不只是“让 AI 帮人写 PLC”，而是把整个系统推进为一个 `AI for AI` 软件平台：

- AI 负责生成控制意图、拓扑、约束、场景和回归资产
- 编译器负责把这些内容收敛为统一 IR，并做形式化验证
- runtime / simulation / codegen 负责把结果转成可执行、可审计、可交付的工件
- 人类工程师从“手写细节”转向“定义边界、审核证据、批准发布”

这条路线要求系统满足四件事：

- AI 产物必须能进入统一语义模型，而不是停留在 prompt 文本
- AI 生成结果必须能被 verification、simulation、traceability 严格约束
- 代码生成不能静默丢语义，必须明确哪些语义保留、哪些语义擦除
- release bundle 必须能让另一个 AI 或另一位工程师复现整个决策链

如果目标是做一个真正能惊艳全球的 `AI for AI` 软件，RustPLC 的差异化不在于“又一个生成器”，而在于：

- 让 AI 生成的工业控制系统具备可验证性
- 让 AI 生成结果具备可执行性
- 让 AI 生成工件具备可追责性
- 让 AI 协作流程具备工程闭环

---

## 🗺️ 路线图 (Roadmap)

### ✅ 已完成
- [x] 基于 Rust 的 DSL 编译器与数学引擎验证
- [x] SIL / Virtual-board / RP2040 全环境统运行时
- [x] PLC 拓扑语义严格门禁体系 (SEM-101~107)
- [x] 智能故障诊断、告警、及在线信号强制推流
- [x] **ST 代码生成引擎** 与针对性的 matiec 闭环编译验证

### 🚧 进行中 / 计划内
- [ ] 🔌 **硬件抽象层扩展**：接入 EtherCAT / Modbus 与工业级 GPIO 扩展板。
- [ ] 🕸️ **多控制器协同**：支持分布式的拓扑定义与时间同步验证。
- [ ] 💻 **LSP 编辑器支持**：提供基于 VSCode/Neovim 的全自动语法高亮与智能提示插件。

---

## 📜 许可协议

本项目采用 [MIT License](LICENSE) 许可，你可以自由地在闭源商业系统中使用。

<br>

<p align="center">
  <sub><strong>Written in Rust 🦀, so it won't panic.</strong><br><em>Well, at least not on your production line.</em></sub>
</p>
