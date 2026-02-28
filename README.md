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
cargo run --release -- examples/two_cylinder.plc --no-print-ir
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

> ⚠️ **强制审核规则（自 2026-02-24 起）**：每个 `device` 必须声明 `purpose`，缺失将直接导致 Semantic Gate 校验失败。
>
> 🔄 **兼容说明（~ 2026-06-30）**：旧版 `connected_to` 或端口作为设备的写法仍可运行，但会给出 `WARN` 级迁移提示。

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
你可以通过 **AI 对话生成**（推荐）：
> *"帮我写个 PLC 程序。我有两个气缸，不能同时伸出，先伸 A 再伸 B..."*

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
cargo run --release -- sim-plc examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out trace.jsonl

# 无板交付门禁：校验实时性能与时序表现
cargo run --release -- no-board-gate examples/assembly_station.plc \
  --scenario scenarios/normal.yaml \
  --out-dir out/gate \
  --max-p99-exec-us 500 \
  --max-overrun-count 0
```

<br>

### 3. 硬件部署与发布交付
```bash
# 一键编译 RP2040 固件并烧录
cargo run --release -- build-rp2040 examples/assembly_station.plc \
  --out out/rp2040 --io-map io_map.toml --emit-uf2 out/firmware.uf2

# 自动化发行包生成
cargo run --release -- release-bundle examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out-dir out/release
```

---

## 📚 开发文档与资源

欢迎访问我们的 **[GitHub Wiki](https://github.com/xxrust/RustPLC/wiki)** 获取详尽指南：

- 🚀 [Quick Start](https://github.com/xxrust/RustPLC/wiki/Quick-Start) - 5 分钟上手指南
- 📖 [DSL Language Reference](https://github.com/xxrust/RustPLC/wiki/DSL-Language-Reference) - 完整语法参考
- 🏗️ [Architecture](https://github.com/xxrust/RustPLC/wiki/Architecture) - 编译流水线与模块架构
- 🔬 [Verification Engines](https://github.com/xxrust/RustPLC/wiki/Verification-Engines) - 深入了解四大形式化引擎
- 🤖 [AI Assisted Generation](https://github.com/xxrust/RustPLC/wiki/AI-Assisted-Generation) - AI 辅助编码流程

> **📎 进阶开发者提示**：在本地 `docs/已实现/` 目录中，可查阅场景系统、在线变量控制、元件库以及 Subtype 规范等数十份详细的底层设计白皮书。

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
