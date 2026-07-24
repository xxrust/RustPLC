# RustPLC 操作说明

RustPLC 是一个面向工业控制交付的建模、编译、验证、仿真和证据审查工具。使用者先描述系统意图，再通过 RustPLC 将意图收敛为 IR，运行安全性、活性、时序、因果性验证，最后由人完成接线点检、HIL 检查和放行。

本手册对应当前仓库版本，主入口是配套 HTML 图文版：[rustplc-user-guide.html](rustplc-user-guide.html)。

## 1. 工作模型

```text
系统意图 / main.system.md
          |
          v
PLC source (.plc or .bundle.toml)
          |
          v
Parser -> AST -> Semantic -> IR
                              |
          +-------------------+-------------------+
          |                   |                   |
          v                   v                   v
      Verification       Runtime / SIL        ST / firmware
          |                   |                   |
          +-------------------+-------------------+
                              |
                              v
              项目证据 -> 接线点检 -> HIL -> 人工放行
```

四个验证引擎分别检查：

- Safety：设备冲突、互斥和必要条件。
- Liveness：死锁、活锁和不可达路径。
- Timing：完成时间和时序预算。
- Causality：从输出到设备反馈的信号链是否闭合。

编译通过代表当前源码通过了工具链门禁。它不代表已经完成真实接线、仪表测量、目标硬件时序验证或安全签字。

## 2. 准备环境

在 Windows PowerShell 中：

```powershell
git clone https://github.com/xxrust/RustPLC.git
cd RustPLC
cargo build --release
```

开发 Web 工作台还需要 Node.js：

```powershell
npm --prefix web-ui install
npm --prefix web-ui run build
```

检查 CLI 是否可用：

```powershell
cargo run --release --bin rust_plc -- --help
cargo run --release --bin rust_plc -- help project-check
```

两种 launcher 的边界：

- 源码仓根目录使用 `cargo run --release --bin rust_plc -- ...`。
- 已安装的二进制使用 `rust_plc ...`。
- scaffold 项目不是 Cargo 项目；进入 scaffold 项目后使用已安装的 `rust_plc`，或从源码仓根目录用绝对路径调用 `cargo run`。

## 3. 创建项目

### 3.1 选择交付层

先判断项目属于哪一层：

| 层级 | 使用场景 |
|---|---|
| `module` | 一个可复用机构或控制模块 |
| `station` | 一个独立可测试的工艺单元 |
| `line` | 多个工站之间的集成流程 |

### 3.2 小型单文件项目

```powershell
cargo run --release --bin rust_plc -- new demo_project
cd demo_project
```

入口文件是 `plc/main.plc`，场景通常放在 `scenarios/nominal/normal.yaml`。

### 3.3 复杂项目

复杂工站或产线使用结构化 fragments：

```powershell
cargo run --release --bin rust_plc -- new station_press --layout structured-fragments --delivery-layer station
cd station_press
```

生成的主要目录：

```text
station_press/
  plc/main.system.md
  rustplc.bundle.toml
  00_topology/
  process_model/process_operation_model.toml
  01_init/
  02_process/
  03_constraints/
  04_faults/
  05_supervision/
  06_manual/
  07_hmi/
  config/state_proof.toml
  scenarios/nominal/normal.yaml
  out/
```

创建后必须替换 scaffold placeholder。至少补齐：

1. `plc/main.system.md`：设备、工艺、任务、操作者边界、故障和安全目标。
2. `process_model/process_operation_model.toml`：离散工件流的 source、destination、资源和 admission 规则。
3. source fragments：拓扑、初始化、自动流程、约束、故障、手动和 HMI 语义。
4. `rustplc.bundle.intent_alignment.contract.json`：将业务里程碑绑定到真实 trace 证据。
5. `scenarios/nominal/normal.yaml` 和 fault scenarios：显式驱动非闭环输入。

## 4. 编写 DSL 时的安全规则

### 4.1 拓扑闭环设备使用高层动作

气缸等已经由设备库闭环建模的机构，直接写设备动作和超时分流：

```plc
step feed_forward:
    action: extend cyl_feed
        timeout: 800ms -> goto feed_warning.feed_cyl_warn
```

设备动作已经包含到位反馈语义。任务中不重复编排普通传感器等待来模拟同一个闭环。

### 4.2 blocking step 使用自然完成

`wait`、`delay` 和长时设备动作由自己的完成条件离开 step。不要在同一个 blocking step 中放无条件 `goto`，以免路由跳过阻塞语义。

```plc
step wait_start:
    wait: rising_edge(start_button)
    allow_indefinite_wait: true

step recheck_empty:
    wait: residual_present == false
    timeout: 2s -> goto startup_fault.residual_unknown
```

`allow_indefinite_wait: true` 只用于操作者命令、上游交接或其他任务拥有的外部事件。设备自己的 home、limit、vacuum、empty 等反馈需要 timeout 和故障路径。

### 4.3 状态必须有反馈证明

离开 step 的状态应来自传感器、控制器输入、工件 token、操作者 front-door 事件、拓扑闭环动作，或明确记录的 no-feedback 例外。不要使用 `bool = true`、`*_ready = true` 或内部 flag 预设生产状态。

### 4.4 操作者属于 front-door

按钮、复位、人工确认和 HMI 命令属于操作者边界。它们通过现场设备和 `controller_io` 映射进入 PLC；不要把人建模成普通设备，也不要创建 `plc_main -> button` 的反向物理关系。

### 4.5 有真实工件就建模 workpiece

真实零件流必须使用 `workpiece`、位置、holder/carrier 和 `effect: acquire/transfer/finish`。仿真中的一次 seed 不代表生产现场存在无限供料。

## 5. CLI 验证流程

下面是一条复杂项目的推荐顺序。所有产物放在 `out/`，源码侧的 process model 和 intent contract 留在项目源目录。

### 5.1 生成场景骨架

```powershell
rust_plc scenario-init rustplc.bundle.toml `
  --out scenarios/nominal/normal.yaml `
  --preset normal
```

骨架生成后，补充所有普通 PLC 输入、传感器、复位和人工事件。仅脉冲 start 通常不足以推进真实工艺。

### 5.2 检查工艺操作模型

```powershell
rust_plc process-model-check rustplc.bundle.toml `
  --model process_model/process_operation_model.toml `
  --output human
```

`OP-002` 表示同一 task 内的相邻工艺操作缺少共享端点或资源依据。`OP-003` 表示当前 split/merge/carrier 语义仍有模型限制。两类结果都应作为工程问题处理。

### 5.3 运行统一项目门禁

```powershell
rust_plc project-check rustplc.bundle.toml `
  --scenario scenarios/nominal/normal.yaml `
  --out-dir out/project-check/normal `
  --require-process-model `
  --output human
```

`project-check` 会编排：

- compile / verification
- sequence-lint
- state-proof-check（bundle、variable 或 workpiece flow 会自动加入）
- process-model-check（检测到 process model 时自动加入）
- scenario-doctor
- no-board-gate
- 提供 `--intent-contract` 与 `--intent-evidence` 时追加 intent alignment

建议在 CI 或交付脚本中使用 `--output json`，并保留 `out/project-check/...` 下的每步报告。

### 5.4 仿真并生成 trace

```powershell
rust_plc sim-plc rustplc.bundle.toml `
  --scenario scenarios/nominal/normal.yaml `
  --out out/sim/normal/trace.jsonl
```

用真实 trace 冻结 intent contract 的 milestone 时：

```powershell
rust_plc intent-doctor rustplc.bundle.toml `
  --trace out/sim/normal/trace.jsonl `
  --output human
```

### 5.5 运行 no-board gate

```powershell
rust_plc no-board-gate rustplc.bundle.toml `
  --scenario scenarios/nominal/normal.yaml `
  --out-dir out/gate/normal `
  --max-p99-exec-us 500 `
  --max-overrun-count 0 `
  --output human
```

这个 gate 比较 SIL 与虚拟板执行结果，并记录 trace diff、tick timing、p99 执行时间和 overrun。

### 5.6 生成 ST 或发布包

```powershell
rust_plc gen-st rustplc.bundle.toml --out out/codegen/st/main.st

rust_plc release-bundle rustplc.bundle.toml `
  --scenario scenarios/nominal/normal.yaml `
  --out-dir out/release/normal
```

`gen-st` 只从已经编译并验证的 IR 生成 IEC 61131-3 ST。`release-bundle` 汇总 compile、仿真、时序和 gate 证据，并生成可审计的 manifest。

## 6. 启动 Web 工作台

在 RustPLC 仓根目录执行：

```powershell
npm --prefix web-ui install
npm --prefix web-ui run build
cargo run -p web-server
```

打开：<http://127.0.0.1:8080>

Loopback 开发模式提供演示身份，密码均为 `password`：

| 用户名 | 角色 | 主要职责 |
|---|---|---|
| `engineer` | Compiler Engineer | 查看源码、编译阶段、验证和诊断 |
| `electrical` | Electrical Engineer | 接线和物理点检 |
| `commissioning` | Commissioning Engineer | 仿真、调试和现场调试证据 |
| `safety` | Safety Reviewer | safety review 签字 |
| `release` | Release Approver | release approval 签字 |
| `admin` | Administrator | 管理和测试 |

开发密码只适用于 loopback。部署到其他地址前必须配置真实用户、密码和允许来源。

## 7. Web 工作台操作

### 7.1 选择项目

左侧 Explorer 固定展示三类 canonical 项目：`module`、`station`、`line`。先选交付层，再查看该项目的 source revision、run ID、delivery status 和 holds。

### 7.2 读取 Project Overview

Project Overview 是项目首屏。重点看四段责任链：

1. Agent source authoring：源码归因是否 proven。
2. Compiler verification：当前 run 的编译和验证 stage 是否通过。
3. Wiring and point checks：人工接线点检和物理观察是否完成。
4. Release authorization：安全签字、HIL 和放行前置条件是否满足。

右侧 Evidence Inspector 显示当前选中对象的来源、digest、责任归属和 release boundary。底部 Problems / Tests / Verification 面板提供可定位的诊断和测试证据。

### 7.3 打开源文件和证据

Explorer 中常用节点：

- `Project Overview`：交付状态和责任链。
- `system.md`：系统意图。
- `Source Bundle`：`.plc` 或 `.bundle.toml`。
- `Topology`：设备、连接和 I/O 映射。
- `Controller I/O and Point Checks`：接线表和物理点检。
- `Formal and Observed Evidence`：编译验证、仿真/HIL 和人工观察。
- `Run Timeline`：Agent 事件、工具调用、修正和 anomaly。
- `Trace Replay`：运行轨迹。

`Ctrl+K` 打开 command palette。可以搜索项目、打开视图、运行常用工作台命令。编辑器支持 split group，右侧 Inspector 和底部 panel 可以独立收起。

### 7.4 处理 Problems 和 Tests

点击 Problems 中的代码或 artifact deep link，工作台会打开对应报告或源码位置。先确认：

- 问题属于哪个 stage。
- 诊断代码和来源文件是什么。
- 当前 run ID 是否与项目摘要一致。
- 是否是 `blocked`、`fail`、`warning` 或 `not_proven`。

历史 run 只用于审计。Verification、Tests、Problems、Evidence 和 Geometry 使用当前 run，避免旧 run 覆盖当前结论。

### 7.5 完成接线和点检

打开 `Controller I/O and Point Checks`：

1. 按 alias、PLC channel、terminal 和 wire ID 找到物理点。
2. 对照电气图和实际端子完成接线。
3. 由具备权限的电气或调试人员记录 observation。
4. 上传点位照片、测量值或仪表证据。
5. 复核 safe state、信号方向和 wiring diagnostic。

点检 observation 是人写入的现场证据。浏览器测试中的 synthetic observation 不属于真实接线证明。

### 7.6 HIL 和人工放行

HIL 状态直接来自 `hil_review` hold。普通 observed evidence 不会自动变成 HIL 通过。

放行顺序：

```text
compile / verification
        -> scenario / no-board gate
        -> wiring point checks
        -> safety review
        -> HIL review
        -> release approval
```

Release approval 需要当前 source revision、交付状态和前置签字全部满足。当前系统的电子签名属于内部 engineering attestation，不代表特定法规电子签名合规。

## 8. 状态含义

| 状态 | 含义 | 操作 |
|---|---|---|
| `pass` / `verified` | 当前阶段有可追溯通过证据 | 继续下一阶段，并保留 artifact |
| `blocked` | 前置条件缺失或人工门禁未完成 | 补齐指定前置条件，不把它改写成 pass |
| `fail` | 工具或项目自身发现错误 | 阅读诊断、修复源模型或场景后重跑 |
| `pending` | 尚未完成人工动作 | 由对应角色执行接线、测量、审查或签字 |
| `not_proven` | 证据链没有证明该命题 | 补 provenance、trace 或人工证据 |
| `stale` | 证据对应旧 revision 或旧 run | 重新运行当前 revision，避免复用旧结果 |

## 9. 常见问题

### `project-check` 失败

先打开 `out/project-check/<name>/project_check_report.json`，再按 stage 处理：

- parser / semantic：修正 DSL 语法、名称、设备动作或 I/O 映射。
- `sequence-lint`：补齐 blocking action 的 timeout 和 fault route。
- `state-proof-check`：移除未经证明的 `true`、ready、done 或 available 初值。
- `process-model-check`：修正 task/step 与工艺操作模型的 refine 关系。
- `scenario-doctor`：为非闭环输入添加明确 scenario event。
- `no-board-gate`：检查 SIL 与虚拟板 trace、时序预算和 overrun。
- `intent_alignment`：使用真实 trace 重新选择业务 milestone anchor。

### Web 工作台看不到新结果

确认项目摘要中的 source revision 和 run ID。工作台只投影当前 run；历史 run 仍在 Runs 审计视图。重新运行项目门禁并生成新的 current run 后刷新页面。

### HIL 仍是 blocked

检查 `release/human-holds.json` 中的 `hil_review`，确认目标硬件时序、物理观察和 HIL 证据已经由责任人补齐。点检表中的普通 observed evidence 不会解除 HIL hold。

### release approval 无法提交

确认 delivery status 为 `pass` 或 `current`，并且 wiring、safety review、HIL review 等前置签字是当前 revision 的 approved 状态。过期签字需要重新签署。

## 10. 交付前检查清单

- [ ] 已确认交付层：module、station 或 line。
- [ ] `main.system.md`、architecture、verification 和 intent contract 已完成。
- [ ] process model 存在时，`process-model-check` 通过。
- [ ] `project-check` 的 compile、lint、state proof、scenario、no-board 和 intent alignment 结果已保存。
- [ ] 正常场景和关键故障场景都有 trace。
- [ ] ST、固件或 release bundle 来自当前 source revision。
- [ ] 接线表逐点完成，照片/测量值归档。
- [ ] HIL review 有独立证据。
- [ ] safety review 和 release approval 由正确角色完成。
- [ ] 现场电气安全链、急停、STO、断电恢复和残料状态已由人确认。

## 11. 关键文件

| 文件 | 用途 |
|---|---|
| `plc/main.system.md` | 系统意图和边界 |
| `rustplc.bundle.toml` | 结构化 source entry |
| `process_model/process_operation_model.toml` | 工艺操作调度意图 |
| `scenarios/` | 正常和故障场景 |
| `out/project-check/` | 项目门禁的逐步报告 |
| `out/sim/` | SIL trace 和审计输出 |
| `out/gate/` | no-board gate 证据 |
| `out/release/` | release bundle 和 manifest |
| `release/human-holds.json` | 人工门禁和放行前置条件 |
| `wiring/point-checks.json` | 接线点检和物理观察 |

完整工作台验收证据见：[autonomous_plc_delivery_workbench_selftest.html](../reports/autonomous_plc_delivery_workbench_selftest.html)。
