<p align="center">
  <img src="docs/assets/rustplc-promo.png" alt="RustPLC 宣传页：从工业拓扑到形式化验证再到代码生成" width="900">
</p>

<p align="center">
  <strong>让 AI Agent 设计工控程序，编译器数学证明正确性</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-e8630a?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT License">
  <img src="https://img.shields.io/badge/tests-868_passing-2ea44f?style=flat-square" alt="Tests">
  <img src="https://img.shields.io/badge/code-60K%2B_lines-8250df?style=flat-square" alt="Lines">
  <img src="https://img.shields.io/badge/verification-4_engines-cf222e?style=flat-square" alt="Engines">
</p>

<p align="center">
  <a href="README_EN.md">English</a> | <strong>中文</strong>
</p>

<p align="center">
  <a href="#30-秒理解-rustplc">30 秒理解</a> •
  <a href="#为什么需要-rustplc">为什么需要</a> •
  <a href="#系统架构">系统架构</a> •
  <a href="#标准项目分层">项目分层</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#交付工作台">交付工作台</a> •
  <a href="#核心能力">核心能力</a> •
  <a href="#安全与架构审计">审计报告</a> •
  <a href="#文档">文档</a>
</p>

---

## 30 秒理解 RustPLC

```mermaid
flowchart LR
    A["👷 工程师描述意图"] --> B["🤖 AI Agent 生成 .plc"]
    B --> C["⚙️ RustPLC 编译器"]
    C --> D{"四引擎验证"}
    D -- "✅ 通过" --> E["🚀 部署到硬件"]
    D -- "❌ 不通过" --> F["📋 修复建议"]
    F --> B
    style A fill:#f5f0ff,stroke:#8250df,stroke-width:2px
    style B fill:#f5f0ff,stroke:#8250df,stroke-width:2px
    style C fill:#e8f4fd,stroke:#0969da,stroke-width:2px
    style D fill:#fce4ec,stroke:#cf222e,stroke-width:2px
    style E fill:#e6ffed,stroke:#2ea44f,stroke-width:2px
    style F fill:#fff3e0,stroke:#e8630a,stroke-width:2px
```

传统方式：工程师手写梯形图 → 人工安全审查 → 碰撞/死锁/超时在调试现场才发现

RustPLC：工程师描述工艺 → AI 生成声明式 DSL → 编译器数学证明安全性 → 所有问题在编译期捕获

**一句话：RustPLC 是工控领域的 "Rust 编译器" — 如果它编译通过，它就是安全的。**

---

## 安全与架构审计

2026-07-23 完成的代码库审计覆盖 Web 控制面、semantic 前门、runtime 所有权、并发 verification/runtime 一致性及 Rust/npm 供应链。当前验证状态：

- `cargo check --workspace --all-targets` 通过
- `cargo test --workspace --all-targets` 通过
- RustSec 未发现已知漏洞、unsound 或 yanked 依赖；升级 defmt 链后嵌入式工具链保留 4 个停止维护警告，均来自上游 HAL/PIO 链
- npm 官方审计结果为 0 个已知漏洞，前端生产构建通过
- Web server 与 CLI utilities 已完成第一轮职责拆分，前端 `npm run lint` 为 0 errors / 0 warnings，Monaco 编辑器 chunk 从约 4.46 MB 降到约 2.66 MB

详细结论、机制链、源码证据和残余架构风险：

- **[HTML 审计报告](docs/audits/rustplc_security_architecture_audit_2026-07-23.html)**
- **[Markdown 审计总结](docs/audits/rustplc_security_architecture_audit_2026-07-23.md)**

---

## 交付工作台

Autonomous PLC Delivery Workbench 是 RustPLC 的桌面式工程界面。它以交付项目为观察单元，在同一个 Cursor/VS Code 风格工作区中展示 Agent 运行记录、PLC 源码、编译器阶段、四类验证、接线表、点检证据、HIL/trace 证据和人工放行门禁。

编译器进展通过真实交付项目体现。Problems、Tests、Search、状态栏和项目证据视图聚合各项目实际触发的阶段、诊断与 blocker；项目内的编译器证据、物理观察和人工责任始终保持独立状态。

当前正式 registry 固定包含三个互相独立的 canonical 项目：

- `module.axis_move_blocking_baseline`：单轴 blocking 动作与 runtime/project-check 基线。
- `station.dual_slot_shuttle_press_cell`：双槽移载、压装与工艺操作 refinement。
- `line.three_station_assembly`：三工站装配线、跨设备流程与 scenario runtime 能力边界。

这三个项目共同构成编译器进展接口。项目交付状态当前为 `0 pass / 1 blocked / 2 fail`，工作台按真实 artifact 展示这些结论，不对项目做优劣排名。运行证据已经证明三项目 fixture 可被 Agent 确定性物化和重复执行；源码由 Agent 在无人干预条件下自主创作这一点仍为 `source_authoring_verdict=not_proven`，因此总体 `unattended_verdict=not_proven`。

```powershell
cd web-ui
npm install
npm run build
cd ..
cargo run -p web-server
```

打开 `http://127.0.0.1:8080`。Loopback 开发模式提供 `engineer`、`electrical`、`commissioning`、`safety`、`release` 和 `admin` 演示身份，默认密码为 `password`；非 loopback 部署必须显式配置认证与允许来源。

工作台需求、证据模型与验收条件见 [Autonomous PLC Delivery Workbench PRD](tasks/prd-autonomous-plc-delivery-workbench.md)，本次阶段性验收见 [HTML 报告](docs/reports/autonomous_plc_delivery_workbench_selftest.html)。完整 WCAG、源码自主创作归因和最终提交后的 clean-checkout 证据仍按报告中的开放项处理。

三项目全流程自测可由一个命令启动；第二轮通过 `-RepeatOf` 对输入 digest、步骤集合和 verdict 做机械比较：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run_delivery_project_corpus.ps1
```

本次验收 corpus 为 `20260724-210000` 和 `20260724-211000`。两轮 harness 均通过，第二轮 repeatability differences 为 0。完整门禁、Subagent 完整度和异常修正记录见 [自测报告](docs/reports/autonomous_plc_delivery_workbench_selftest.md)。

可复制的异常、修正和执行链同时记录在 [Markdown 报告](docs/reports/autonomous_plc_delivery_workbench_selftest.md)。

---

## 系统架构

完整的系统架构图和技术细节请查看 **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**。

### 编译流水线

```
.plc DSL → Parser → AST → Semantic Analysis → IR
                                                 ↓
                    ┌────────────────────────────┼────────────────────────────┐
                    ↓                            ↓                            ↓
            Verification (4 Engines)     Runtime Bridge              ST Codegen
            - Safety (BMC + k-induction) - IR → runtime-core        - IEC 61131-3
            - Liveness (SCC + reachability) - Port mapping          - RP2040 firmware
            - Timing (Critical path)     - State validation         - Renode scripts
            - Causality (Topology BFS)   - Action sequencing
```

### 核心模块

| 模块 | 代码量 | 职责 |
|------|--------|------|
| Parser | 153K lines | PEG 语法 → AST |
| Semantic | 367K lines | 预处理 + 名称解析 + IR 降级 |
| IR | 18K lines | 规范中间表示 |
| Verification | 195K lines | 四引擎并行验证 |
| Codegen | 49K lines | ST 代码生成 |
| Runtime Bridge | 8K lines | IR → runtime-core 翻译 |

---

## 为什么需要 RustPLC

工业控制软件有一个根本矛盾：

> 控制逻辑越复杂，人工审查越不可靠；但越是复杂系统，安全性要求越高。

现有工具链（梯形图、ST、FBD）本质上是 **"写完再查"** — 先实现逻辑，再靠人工 review 和现场调试发现问题。这在简单场景够用，但面对并发控制、多轴联动、安全联锁时，人脑已经跟不上状态空间的爆炸。

RustPLC 的回答是 **"写时即证"**：

<p align="center">
  <img src="docs/assets/comparison.svg" alt="传统 PLC 开发 vs RustPLC" width="750">
</p>

最后一行是关键：当 AI Agent 能生成工控程序时，**谁来保证 AI 生成的代码是安全的？** RustPLC 就是这个保证。

---

## 标准项目分层

RustPLC 的标准项目按语义层组织控制意图，让拓扑、工艺调度、PLC 程序流、故障、人机入口各自有明确边界：

```text
plc/main.system.md
    |
    v
00_topology/
    设备、连接、工件位置、容量、资源边界
    |
    v
process_model/process_operation_model.toml
    可调度工艺操作：source available / destination capacity / shared resource / predecessor
    |
    v
01_init/
    初始化基线、残料检测、清理/回收/人工确认
    |
    v
02_process/
    自动生产任务：PLC 怎样执行 process_model 中允许的候选操作
    |
    v
03_constraints/
    安全、互斥、节拍、因果约束
    |
    v
04_faults/
    故障路径、报警映射、恢复与收敛
    |
    v
05_supervision/  06_manual/  07_hmi/
    运行入口层：supervisor / 手动维护 / HMI 展示与交互
    |
    v
config/state_proof.toml
    机器可读的 no-feedback 与 trusted initial state 例外
    |
    v
rustplc.bundle.toml -> IR -> verification / runtime bridge / codegen
```

`supervisor` 属于运行入口和模式管理层，负责 operator front-door、自动循环锁存、启动/停止、模式仲裁和安全回退。`05_supervision/`、`06_manual/`、`07_hmi/` 是可按交付面启用的运行入口层，当前项目可以先聚焦自动主流程。

这层设计的核心原因是：拓扑负责物理连接和资源边界，`process_model` 负责拓扑和 task/step 之间的调度意图，`process-model-check` 再验证 task/step 是否 refine 这份源侧模型；`state-proof-check` 再审查物理状态是否由传感器、操作者输入、工件 token、闭环动作或显式例外证明，而不是被变量初值和内部 flag 预设为成立。

---

## 快速开始

```bash
git clone https://github.com/xxrust/RustPLC.git
cd RustPLC
cargo build --release
```

### 编译并验证

```bash
cargo run --release -- examples/dual_axis_platform.plc --no-print-ir
```

```
Verification passed:
  - Safety: Complete proof (depth 4) — conflicts_with satisfied
  - Liveness: Passed — no deadlock risk
  - Timing: Passed — task.cycle within 8000ms budget
  - Causality: Passed — all signal chains connected
```

四个验证引擎并行运行，数学证明你的程序没有碰撞、死锁、超时和断链。

### 项目级门禁

```bash
cargo run --release --bin rust_plc -- project-check my_plc_project/rustplc.bundle.toml --output human
cargo run --release --bin rust_plc -- state-proof-check my_plc_project/rustplc.bundle.toml \
  --config my_plc_project/config/state_proof.toml --output json
```

`project-check` 是复杂项目的默认交付门禁。对 `.bundle.toml`、包含 `variable`、或包含 workpiece flow 的项目，它会在 `sequence-lint` 之后、`process-model-check` 之前自动运行 `state_proof_check`。明显错误会失败，并在 `out/project-check/state_proof_check/report.json` 保留机器可读报告。

`state-proof-check` 专门拦截两类高风险问题：一是把必须由现场输入证明的状态用 `bool = true`、`*_ready`、`*_done`、`*_available` 等内部 flag 预设为成立；二是设备停机/急停恢复后可能残留工件，但 `01_init` 或 startup task 没有检测、清理、回收、人工确认或显式阻断。项目级例外写在 `config/state_proof.toml`，支持 `[[no_feedback_steps]]` 与 `[[trusted_initial_state]]`，每条都必须包含 `reason` 和 `proof_basis`。

### 生成 IEC 61131-3 ST 代码

```bash
cargo run --release -- gen-st examples/dual_axis_platform.plc --out out/dual_axis.st
```

验证通过的 IR 直接生成标准 ST 代码，可导入 OpenPLC / CODESYS。

### 场景仿真

```bash
# 生成场景骨架
cargo run --release -- scenario-init examples/project_scaffold_demo/plc/main.plc \
  --out scenarios/normal.yaml --preset normal

# SIL 仿真
cargo run --release -- sim-plc examples/project_scaffold_demo/plc/main.plc \
  --scenario scenarios/normal.yaml --out trace.jsonl
```

### 部署到 RP2040

```bash
cargo run --release -- build-rp2040 examples/rp2040_motion_minimal.plc \
  --out out/rp2040 --io-map examples/rp2040_motion_minimal.io_map.toml --emit-uf2 out/firmware.uf2
```

### 创建新项目

```bash
cargo run --release -- new my_plc_project --layout structured-fragments
```

```
my_plc_project/
├── rustplc.project.toml
├── plc/main.system.md
├── 00_topology/
├── process_model/
│   └── process_operation_model.toml
├── config/
│   └── state_proof.toml
├── 01_init/
├── 02_process/
├── 03_constraints/
├── 04_faults/
├── 05_supervision/
├── 06_manual/
├── 07_hmi/
├── rustplc.bundle.toml
├── scenarios/
│   ├── nominal/
│   └── faults/
└── out/
```

---

## DSL 一览

RustPLC 的 DSL 是工控意图的声明式描述。工程师（或 AI）声明 **"有什么设备、什么约束、要做什么"**，编译器负责证明这些声明是否自洽。

```plc
[topology]
device plc_main: plc {
    purpose: "控制器本体",
    model_ref: openplc_softplc
}
device valve_A: solenoid_valve {
    purpose: "气缸 A 驱动电磁阀",
    response_time: 20ms
}
device cyl_A: cylinder {
    purpose: "工位 A 气缸",
    stroke_time: 300ms
}
device sensor_A_ext: sensor { purpose: "气缸 A 伸出到位反馈" }
device sensor_A_ret: sensor { purpose: "气缸 A 缩回到位反馈" }

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }
relation { from: sensor_A_ret.out, to: plc_main.X1, via: reports_to }

[constraints]
timing: task.cycle must_complete_within 2000ms
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ret

[tasks]
task cycle:
    step extend:
        action: extend cyl_A
            timeout: 500ms -> goto fault.timeout
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto ready

task fault:
    step timeout:
        action: log "气缸动作超时"
    step motion_fault:
        action: log "气缸反馈与目标动作不一致"
    step safety_fault:
        action: log "气缸反馈出现矛盾状态"
```

三个段落，三件事：

- **topology** — 有什么设备，怎么连接
- **constraints** — 安全边界、互斥条件和必须满足的约束
- **tasks** — 按什么顺序做什么

编译器读取这三段，构建统一 IR，然后用四个引擎证明约束在所有可能的执行路径上都成立。

---

## 核心能力

### 四引擎形式化验证

编译器内置四个并行验证引擎，用数学证明覆盖安全性、活性、时序和因果性：

| 引擎 | 方法 | 证明什么 |
|------|------|---------|
| Safety | BMC + k-归纳 | `conflicts_with` / `requires` 在所有状态下成立 |
| Liveness | SCC + 可达性分析 | 无死锁、无活锁、所有路径可终止 |
| Timing | 关键路径分析 | `must_complete_within` 时间预算满足 |
| Causality | 拓扑 BFS | 信号链完整，无断链、无孤立设备 |

### 编译流水线

<p align="center">
  <img src="docs/assets/pipeline.svg" alt="RustPLC 编译流水线" width="850">
</p>

### 多目标部署

| 目标 | 命令 | 输出 |
|------|------|------|
| 形式化验证 | `compile` | verification_report.json |
| ST 代码生成 | `gen-st` | IEC 61131-3 ST (OpenPLC/CODESYS) |
| RP2040 固件 | `build-rp2040` | UF2 固件 + io_map |
| STM32 仿真 | `build-renode-stm32` | ELF + Renode trace |
| SIL 仿真 | `sim-plc` | trace.jsonl + sim_report |
| 无板交付 | `no-board-gate` | diff_report + timing_report |
| 发布包 | `release-bundle` | SHA256 manifest + 全部证据 |

### 场景工程

```bash
scenario-init     # 生成场景骨架
scenario-validate # 校验场景合法性
scenario-expand   # 展开 pulse/hold 语法糖
scenario-gen      # 批量生成场景
sim-regress       # 批量回归仿真
```

故障注入、波形导出、KPI 回归（超调量 / 稳态误差 / 调节时间）、失败最小化 — 完整的仿真工程链。

### 实时门禁

```bash
# SIL vs 虚拟板对比 + 实时阈值检查
cargo run --release -- no-board-gate examples/project_scaffold_demo/plc/main.plc \
  --scenario scenarios/normal.yaml \
  --max-p99-exec-us 500 --max-overrun-count 0
```

tick 级时序采样，p50/p95/p99 统计，超限自动拦截。发布包包含 SHA256 manifest + git 元数据，可审计可追溯。

---

## AI for AI 方向

传统 PLC 编辑器的默认用户是人：界面、梯形图、变量表和调试体验都围绕“让工程师更好地手写代码”设计。RustPLC 的默认用户是 agent：人输入需求、专利或设计意图，agent 负责生成文档、规划工程、串行或并行写代码、调用编译器验证、根据诊断自主推理修复，并输出可审查的交付证据。

<p align="center">
  <img src="docs/assets/ai-for-ai-platform.png" alt="Agent-native PLC 工程平台：人输入需求专利设计意图，agent 规划、并行实现、编译验证、自主修复并输出证据" width="900">
</p>

这就是 RustPLC 的 **AI for AI**：PLC 工程被拆成 agent 能稳定执行、验证和恢复的结构化任务。

1. 输入层面：支持从需求、专利、设备清单、工艺意图进入 `main.system.md`
2. 规划层面：先建立拓扑、设备语义、工件模型、front-door 和 `process_model`
3. 实现层面：agent 可按结构化目录分工，串行或并行生成 task/step、fault、scenario 和配置
4. 验证层面：编译器把产物收敛到 IR，并用 verification / runtime bridge / codegen 统一约束
5. 修复层面：结构化诊断、report、trace 和 gate 结果能被 agent 用来继续推理和修复

RustPLC 的差异化在于面向 agent 的 PLC 工程系统：agent 能从意图出发，把项目推进到 **可验证、可执行、可审计、可复现**。

### MCP 集成

RustPLC 提供 MCP Server，AI Agent 可以直接调用编译器：

```json
{
  "mcpServers": {
    "rustplc": {
      "command": "python",
      "args": ["-m", "server"],
      "cwd": "rustplc-mcp"
    }
  }
}
```

Agent 可用的工具：
- `validate_plc` — 验证 .plc 文件
- `compile_plc` — 编译并获取 IR
- `get_rustplc_skill_guide` — 获取 DSL 编写指南

---

## 示例库

| 示例 | 特性 | 复杂度 |
|------|------|--------|
| `project_scaffold_demo/` | 最小项目结构、脚手架 | ★ |
| `rp2040_motion_minimal.plc` | 运动控制、板级 I/O 映射 | ★★ |
| `dual_axis_platform.plc` | 双轴联动、parallel、race、conflicts_with | ★★★ |
| `load_unload_concurrent_tasks.plc` | 多设备协调、parallel、requires | ★★★ |
| `nuclear_coolant_isolation.plc` | SIL3 核安全、冗余传感器、OR 容错 | ★★★★ |
| `three_station_assembly.plc` | 大规模拓扑、装配流程 | ★★★★ |
| `process_device_demo.plc` | 过程设备语义动作、runtime handler 边界 | ★★ |
| `recovery_templates/` | 急停恢复、故障分流 | ★★★ |
| `force_override_demo.plc` | 在线强制、调试语义 | ★★ |

---

## 系统架构

> 完整的编译流水线图见上方 [编译流水线](#编译流水线) 章节。下图是分层总览：

```mermaid
graph TB
    subgraph Input["📝 输入层"]
        PLC[".plc DSL 文件<br/>topology · constraints · tasks"]
        YAML["场景 YAML<br/>inputs · faults · tick_ms"]
    end

    subgraph Compiler["⚙️ 编译器核心 · 123 Rust 源文件 · 60K+ 行"]
        PARSE["Parser<br/>PEG 544 规则"] --> AST["AST"]
        AST --> SEM["Semantic<br/>预处理 + IR 降级"]
        SEM --> IR["IR<br/>petgraph DiGraph"]
    end

    subgraph Verify["🔬 四引擎并行验证"]
        S["Safety<br/>BMC"]
        L["Liveness<br/>SCC"]
        T["Timing<br/>关键路径"]
        C["Causality<br/>BFS"]
    end

    subgraph Runtime["🏃 运行时层 · 7 crate"]
        RT["runtime-core<br/>no_std"]
        SIM["SimIO<br/>SIL 仿真"]
        CG["Codegen<br/>IEC 61131-3 ST"]
        RP["board-rp2040"]
        RE["board-renode-stm32"]
        WEB["web-server<br/>Axum"]
    end

    subgraph Output["📦 输出层"]
        VR["verification_report.json"]
        ST["IEC 61131-3 ST"]
        FW["firmware.uf2"]
        TR["trace.jsonl + wave.vcd"]
        RB["release-bundle/"]
    end

    PLC --> PARSE
    YAML --> SIM
    IR --> S & L & T & C
    S & L & T & C --> VR
    IR --> RT & SIM & CG
    RT --> FW
    CG --> ST
    SIM --> TR
    RT --> RB

    style Input fill:#fff3e0,stroke:#e8630a,stroke-width:2px
    style Compiler fill:#e8f4fd,stroke:#0969da,stroke-width:2px
    style Verify fill:#fce4ec,stroke:#cf222e,stroke-width:2px
    style Runtime fill:#e6ffed,stroke:#2ea44f,stroke-width:2px
    style Output fill:#f6f8fa,stroke:#d0d7de,stroke-width:2px
```

---

## 项目规模

| 指标 | 数值 |
|------|------|
| Rust 源文件 | 123 个 |
| 编译器代码 | 60,000+ 行 |
| PEG 语法规则 | 544 行 |
| 测试用例 | 868 个 |
| 示例 .plc 文件 | 32 个 |
| Workspace crate | 7 个 |
| 验证引擎 | 4 个 |
| CLI 子命令 | 20+ 个 |
| Wiki 文档 | 20 篇 |
| 架构文档 | 8 篇 |

---

## 文档

完整文档分为三个层次：

### 架构与开发指南

| 文档 | 用途 |
|------|------|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | 📐 系统架构全景图（推荐首读） |
| [AGENTS.md](AGENTS.md) | 🛠️ 开发者快速上手指南 |
| [CODEX.md](CODEX.md) | 📚 编译器核心设计文档 |
| [快速开始](QUICKSTART.md) | ⚡ 5 分钟上手指南 |

### 仓库内文档

| 文档 | 内容 |
|------|------|
| [`docs/architecture/signal-direction.md`](docs/architecture/signal-direction.md) | 并发 task / blocking step 语义（冻结） |
| [`docs/architecture/process-operation-layer.md`](docs/architecture/process-operation-layer.md) | 拓扑与 task/step 之间的工艺操作调度层 |
| [`docs/architecture/device-semantics-library.md`](docs/architecture/device-semantics-library.md) | 设备族语义抽象 |
| [`docs/architecture/intent_alignment_verification.md`](docs/architecture/intent_alignment_verification.md) | 意图合约验证 |

### 标准项目组织

标准项目分层是 README 的主模型，详见上文 [标准项目分层](#标准项目分层)。更完整的组织思想见 [`docs/wiki/Structured-Fragment-Project-Layout.md`](docs/wiki/Structured-Fragment-Project-Layout.md)。

### Wiki

| 页面 | 内容 |
|------|------|
| [AI for AI 平台愿景](docs/wiki/AI-for-AI-Platform-Vision.md) | AI Agent 工程平台方向 |
| [结构化项目布局](docs/wiki/Structured-Fragment-Project-Layout.md) | `00_topology` 到 `07_hmi` 的组织思想 |
| [状态证明检查](docs/wiki/State-Proof-Check.md) | `state-proof-check`、残料策略与机器可读例外 |
| [PLC 优化管线](docs/wiki/PLC-Optimization-Pipeline.md) | 优化候选生成与排序 |
| [设备库](docs/wiki/Device-Library.md) | TOML 设备定义与约束 |
| [场景资产化](docs/wiki/Scenario-Assetization-Coverage-Feedback.md) | 场景工程与覆盖反馈 |
| [RP2040 运动控制](docs/wiki/RP2040-Motion-Minimal-Example.md) | 嵌入式部署示例 |
| [拓扑信号方向](docs/wiki/Topology-Signal-Direction-Refactor.md) | 端口级拓扑语义 |
| [步进电机安全建模](docs/wiki/Stepper-AB-Encoder-Safety-Modeling.md) | 运动控制安全 |
| [CI Runbook](docs/wiki/CI-Runbook.md) | CI/CD 流程 |
| [故障安全状态](docs/wiki/Fail-Safe-Safe-State.md) | 安全状态建模 |
| [开发者引导包](docs/wiki/Developer-Bootstrap-Pack.md) | 新人上手指南 |

### CLI 帮助

```bash
cargo run --release -- --help              # 命令索引
cargo run --release -- help <command>      # 单个命令详情
cargo run --release -- help compile        # 编译命令帮助
cargo run --release -- help sim-plc        # 仿真命令帮助
```

---

## 工程原则

- 语义必须先于实现
- IR 是唯一语义汇合点
- Verification 是编译主路径
- Runtime 和 Codegen 只消费已闭合的 IR 语义
- 文档、示例、测试、skills 必须与编译器契约同步

---

## Roadmap

### 已完成

**编译器核心** — DSL 设计、四引擎验证、结构化错误报告、DSL v2 语法扩展、优化管线

**设备语义** — 气缸与轴动作语义、过程设备动作、station 协议契约

**仿真与测试** — SIL 仿真、场景工程（init/validate/expand/gen/regress）、KPI 回归

**部署与门禁** — ST 代码生成、RP2040 固件、Renode STM32 仿真、无板交付门禁、发布包

**拓扑与语义** — 端口级拓扑、多维标签、语义 diff、性能门禁、意图合约验证

### 进行中

- ⏳ 硬件抽象层（EtherCAT / Modbus / 更多 GPIO 板卡）
- ⏳ 多控制器协调
- ⏳ LSP 编辑器集成（语法高亮、补全、跳转定义）
- ⏳ Web IDE（在线编辑、验证、仿真）

---

## 许可

[MIT License](LICENSE)

---

<p align="center">
  <sub>用 Rust 写的，所以它不会 panic。至少不会在产线上。</sub>
</p>
