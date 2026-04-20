# 路线图：从 RustPLC 到「一次生成即可用」的 Arduino 自动仿真/调试闭环

## 背景与目标

你希望达成的终态是：

- 只在软件里把控制逻辑、I/O 映射、被控对象（plant）模型等“都弄好”
- 自动生成可靠的 Rust 程序并装载到 Arduino（或同类 MCU）中
- 自动仿真、自动调试、尽量“一次即可用”
- 理想情况下，尽量减少/取消真实硬件联调成本（用仿真与形式化方法提前暴露问题）

本仓库当前强项是：DSL -> IR + 形式化验证（Safety/Liveness/Timing/Causality）+ JSON IR 输出。
要达到“自动装载 + 自动仿真/调试”的闭环，还需要把“可验证的模型”落到“可执行系统 + 可验证接口合同 + 可重复仿真环境”上。

---

## 还需要完成的 8 个大任务

### 1) 确定性执行语义与运行时（Runtime）

把 DSL/IR 的状态机语义落地为一个“可执行且确定性”的 runtime，包括：

- `step / wait / timeout / delay / goto` 的精确定义
- `parallel / race / repeat` 的调度语义（尤其是并行分支的完成条件、竞争分支的决策点）
- 时间模型（tick、时钟源、调度周期）与日志语义

目标：确保“编译器验证过的模型”与“实际运行的代码”一致，否则验证结论无法外推到实机行为。

### 2) 代码生成（IR -> Rust）与可重复构建产物

从 JSON IR（或内部 IR）生成 Rust 代码，至少包含两条产线：

- `std` 版：跑在 PC 上做 SIL（Software-in-the-Loop）仿真/回归测试
- `no_std` 版：跑在 MCU/Arduino 上

并且产出可追踪/可复现信息：版本、hash、配置、I/O 映射表，保证“同一份 .plc 永远生成同一份固件”。

### 3) 硬件抽象层（HAL）与 I/O 映射系统（软件-硬件接口合同）

设计一套“逻辑设备（Y0/X0/valve/cyl/sensor/AI/AO）-> 物理接口（GPIO/PWM/ADC/UART/RS485/Modbus…）”的映射配置。

目标：同一份控制逻辑可以在不同 I/O 后端间切换，例如：

- `SimIO`（仿真）
- `ArduinoGPIO`（真实板）
- `Modbus/EtherCAT`（未来现场）

这一步决定了“硬件设计跟软件一样”的可操作性：把点表/接线/地址从经验工作变成可验证、可生成的配置资产。

### 4) 被控对象（Plant）仿真器：SIL 的关键闭环

仅仿真 PLC 逻辑不够，还需要对拓扑中的设备建立可执行的 plant 模型（可从简到繁）：

- cylinder/motor/valve/sensor/analog 的动态（响应延迟、行程时间、抖动/噪声、采样周期）
- 故障注入（卡滞、漏气、传感器失效、通信抖动）
- 场景回归（批量运行不同初始条件/扰动，输出报告）

目标：让“一次即可用”的主要风险在仿真里暴露，而不是等到硬件上才发现。

### 5) MCU 级仿真（PIL：Processor-in-the-Loop）与仿真板卡联动（QEMU/替代方案）

把固件放进“像 MCU 的环境”里跑，以验证：

- 定时器与 tick 的行为、串口/总线时序、引脚抽象层的正确性
- 与 plant sim 的联动一致性（同一套输入序列应得到相同输出轨迹）

备注：

- 经典 AVR Arduino（如 ATmega328P）通常更常见 simavr/renode 路线；ARM/RISC-V 板更可能走 QEMU/renode。
- 这里的关键不是“是否 QEMU”，而是“固件级时序/外设接口是否可仿真、可回放、可对比”。

### 6) 自动调试与可观测性（让调试变成产物）

需要把“调试信息”标准化为可比对、可回放的产物：

- 状态机轨迹（task/step 迁移、触发条件、timeout 分支）
- I/O 波形（每个点位边沿、时间戳、触发原因）
- 约束运行时证据（哪条 safety/时序假设在运行时被触发/边界逼近）
- record/replay：失败用例可最小化并稳定复现

目标：调试从“工程师盯波形”转成“自动出报告 + 失败用例可复现”。

### 7) 数字孪生/3D 仿真对接（可选，但能显著减少机械层试错）

如果要对接 Omniverse/Isaac Sim 等数字孪生平台，建议先抽象稳定的数据交换层：

- 选择一种桥接协议/数据模型（例如 OPC UA / MQTT / ROS2 其一）
- 把 runtime 的 I/O 映射为孪生平台可消费的 actuator/sensor 通道
- 把孪生平台的传感器回读映射为 PLC 输入

价值：提前暴露几何碰撞、工装干涉、节拍瓶颈等“纯逻辑仿真看不到”的问题。

### 8) “尽量不做硬件测试”的前提工程：假设管理、参数标定、安全裕量

想把硬件测试降到最低，需要把差异显式化为“可验证/可仿真的假设”：

- 物理参数范围（阀响应、行程时间、摩擦、负载变化、通信延迟）
- 传感器质量（抖动、去抖、误触发、失效模式）
- 安全裕量（最坏情况覆盖策略）

结论：在强安全要求或高价值设备上，通常仍需要最低限度的验收测试；但完成 1-7 后，硬件联调应当只剩“少量标定 + 烟雾测试”，而不是大量返工。

---

## 需要你确认的两个问题（用于确定优先级与技术路线）

1) 你要支持的 Arduino/MCU 具体是哪一类？
- 经典 AVR（UNO/Nano/ATmega328P）
- ARM/RISC-V（例如各类 Arduino 兼容板、ESP32、RP2040 等）

2) 你当前更看重哪条先跑通？
- 先把 SIL 闭环做强（plant 仿真 + 回归 + 自动报告），再下沉到固件/板级
- 先打通“固件自动生成 + 自动烧录 + 板级仿真/联动”，再补强 plant 与回归体系

---

## 你已确认的选择（2026-02-14）

- 优先支持：ARM 平台
- 优先路径：先把 SIL 闭环做强
- 目标板卡：Raspberry Pi Pico / Pico W（RP2040，Cortex-M0+）

这意味着：先把“可执行 runtime + plant 仿真 + 自动回归/报告 + 可复现失败用例”跑顺，再把同一套语义下沉到 `no_std` 与 ARM HAL。

---

## Pico（RP2040）落地建议（为什么选它 + 需要的最小配套）

为什么适合作为“第一块板”：

- 生态成熟且成本低，适合把运行时/HAL 先打通成参考实现
- USB 直刷 UF2（不依赖专用烧录器也能先跑通自动装载链路）
- Rust 嵌入式生态支持好，容易形成可复制的模板

最小硬件清单（建议）：

- 1x Raspberry Pi Pico（或 Pico W）
- 1x USB 数据线（Micro-USB）

最小软件工具链（建议）：

- Rust target：`thumbv6m-none-eabi`（RP2040 常用）
- 烧录：UF2（通过 USB Mass Storage）或 `probe-rs`（可选，更适合自动化）
- 日志/调试（可选但强烈建议）：`defmt` + RTT（或串口）

工程策略建议（降低风险、加快闭环）：

- 初期优先选择“解释执行/字节码执行”的 runtime（固件内嵌 IR/字节码），而不是一上来就生成大量静态 Rust 代码。
  - 好处：语义集中在 runtime，一个地方对齐验证与执行；回归更稳。
  - 代价：运行开销略高，但对多数 PLC 节拍足够；后续再做“静态代码生成”作为优化路径。

### 期望的“一键体验”（终态命令行草案）

这里先把目标体验写清楚，后续按里程碑逐步实现（命令名仅示意）：

```bash
# 1) 编译 + 形式化验证（现状已有）
rustplc verify examples/rp2040_motion_minimal.plc

# 2) SIL：带 plant 的仿真跑批（你选择优先做强）
rustplc sim examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --report out/report.json

# 3) SIL 回归：批量场景 + 自动最小化失败用例
rustplc sim-regress examples/*.plc --scenarios scenarios/ --out out/regress/

# 4) 生成 RP2040 固件（把 IR/字节码打包进固件）
rustplc build-rp2040 examples/rp2040_motion_minimal.plc --out out/firmware.uf2

# 5) 自动装载到 Pico（UF2 或 probe-rs）
rustplc flash-rp2040 out/firmware.uf2
```

### RP2040 开发工具链（建议先写成可复制的脚本）

RP2040 常用 Rust target（建议先固定它，避免团队成员环境漂移）：

```bash
rustup target add thumbv6m-none-eabi
```

UF2 路线（最少硬件依赖）的基本思路：

- 进入 BOOTSEL（Pico 按住 BOOTSEL 插 USB）
- 将编译产物转换为 UF2 并复制到 Pico 暴露出来的盘符

备注：为“自动化装载”考虑，后续建议补 `probe-rs` 路线（需要额外硬件/或用 Pico 做 picoprobe）。

---

## 推荐里程碑（按你选择的优先级排序）

### M0：冻结可执行语义（对齐验证与运行）

- 输出：runtime 语义文档（tick/调度/timeout/delay/parallel/race 的确定义）
- 输出：最小可执行 runtime（先跑在 PC 上），能产出可比对的 step 轨迹
- 必须明确的“语义合同”（建议写成可测试的条款）：
  - 时间：tick 精度、delay/timeout 的边界（包含/不包含当前 tick）
  - 并行：parallel 分支的开始时刻、完成判定、冲突动作的优先级/禁止策略
  - 竞争：race 的“先完成”如何定义（同 tick 同时满足怎么办）
  - 动作：同一 step 内多条 action 的顺序性与原子性（是否在同一个 tick 生效）

验收标准（建议）：

- 同一份 `.plc` + 同一 seed，在任意机器上得到完全一致的 step 轨迹（deterministic replay）
- `parallel/race/timeout/delay` 的边界行为可用单测覆盖，并在文档中有明确例子

### M1：SIL v1（SimIO + 轨迹/波形 + 回归跑批）

- 输出：`SimIO` 后端（实现 runtime 需要的最小 IO trait）
- 输出：I/O 记录（变化边沿、时间戳、触发原因、对应 task/step）
- 输出：场景跑批工具
  - 输入：`.plc` + 场景配置（初始状态/外部输入脚本/扰动/seed）
  - 输出：一份结构化报告（通过/失败、失败原因、最小复现参数、波形/轨迹文件）
- 目标：对现有 `examples/*.plc` 建立“仿真回归基线”，让变更可自动验收
- 建议的“回归准则”（先简单可落地）：
  - 轨迹稳定：同 seed 下 step 序列一致
  - 时间稳定：关键 step 的完成时间落在允许区间
  - I/O 稳定：关键输出点位的边沿序列一致

验收标准（建议）：

- 对 `examples/*.plc` 至少建立 1 套“正常场景”回归（场景文件可版本化）
- 任何变更导致轨迹/波形差异时，报告能指出差异发生的 tick 与对应 step

#### 场景文件格式草案（建议 YAML；也可 JSON）

目标：把“外部输入脚本 + 扰动 + seed + 运行时长”变成可版本化资产，支持回放与最小化。

```yaml
# scenarios/rp2040_motion_minimal/normal.yaml（示意）
seed: 42
tick_ms: 1
duration_ms: 12000

# 外部输入脚本：在指定时间点强制某些输入/设定值
inputs:
  - at_ms: 0
    set:
      start_button: true
  - at_ms: 50
    set:
      start_button: false

# 模拟量输入：给一个随时间变化的函数或离散脚本
analog_inputs:
  - name: pressure_ai0
    kind: constant
    value: 60.0

# 故障注入（可选）
faults:
  - at_ms: 3000
    kind: sensor_stuck
    target: sensor_push_L_ext
    value: false
```

约定（建议）：

- `inputs.set` 的 key 使用 `.plc` 中声明的 device 名称（如 `start_button`、`X10`、`sensor_left_arrive`）
- 故障注入优先作用在 plant/传感器层（避免把故障“写进控制逻辑”）

#### 报告格式草案（建议 JSON）

目标：一旦失败，报告里应包含“可复现需要的一切”。

建议字段（示意）：

- `status`: `"pass" | "fail"`
- `seed`, `tick_ms`, `duration_ms`
- `failure`: `{ kind, message, at_ms, task, step }`（失败时必填）
- `artifacts`: `{ trace_path, waveform_path }`

波形格式建议：

- 数字量：VCD（可用 GTKWave 直接查看）
- 模拟量：CSV/JSONL（按时间戳记录）

### M2：Plant v1（足够逼真但可控）

- 输出：基础 plant 模型（可参数化、可控、可复现）
  - cylinder：响应延迟、行程时间、可选卡滞/不到位
  - valve：响应时间、抖动/粘滞的简单模型
  - motor：启动/停机斜坡的离散近似、速度区间
  - sensor：去抖、误触发概率、失效（常开/常闭/卡死）
  - analog：采样周期、噪声、量程夹紧、传感器漂移
- 输出：故障注入框架 + 场景集（覆盖常见“现场首发故障”）
- 目标：把“一次即可用”的主要失败模式在 SIL 中提前暴露，并可稳定复现（seed 固化）

验收标准（建议）：

- 故障注入至少覆盖：传感器卡死、动作超时、随机抖动（带 seed）
- 能自动产出“最小失败用例”：最小扰动集合/最短时间窗/最小输入脚本

### M3：代码生成 v1（IR -> Rust std + no_std），为 ARM 下沉做准备

- 输出：`std` 版可执行产物（继续作为 SIL 基准实现）
- 输出：`no_std` 版可执行产物（先不接真实引脚，先接最小 HAL trait）
- 输出：IR 打包方式（建议二选一，优先 A）：
  - A) IR/字节码作为 `include_bytes!()` 内嵌到固件（同一 runtime，跨平台一致）
  - B) 生成静态 Rust 源码（更快，但需要更强的“语义对齐”回归体系）
- 输出：可重复构建信息（hash/版本/I/O 映射表）

验收标准（建议）：

- `std` 版 runtime 与 `no_std` 版 runtime 对同一 IR/字节码执行轨迹一致（用 golden/回放用例对齐）
- IR/字节码格式带版本号；不兼容变更必须显式 bump

### M4：ARM HAL v1（先选 1 个板卡打通）

- 板卡：Raspberry Pi Pico / Pico W（RP2040）
- 目标：把“同一份 .plc”跑到板上，并且与 SIL 的轨迹对齐（允许噪声但可解释）
- 输出：RP2040 HAL 后端（先实现最小集，再扩展）
  - GPIO（digital input/output）
  - 定时器 tick（驱动 runtime）
  - 串口或 RTT 日志（可观测性最低保障）
- 输出：自动装载（建议阶段性实现）
  - v1：UF2 拖拽/脚本化复制到盘符（最少依赖）
  - v2（可选）：`probe-rs` 一键烧录 + 运行 + 收集日志（更适合自动化回归）
- 输出：最小板级 demo（建议先做“灯/按键 + 一个气缸拓扑”的端到端）

验收标准（建议）：

- 同一份 `.plc` 在 SIL 与 Pico 上的“关键输出点位边沿序列”可对齐（允许引入可解释的抖动/延迟）
- 至少 1 个端到端示例：从 `.plc` 生成 UF2 -> 上板运行 -> 收集到可用的最小观测信号（LED/串口/RTT 其一）

### M5（可选）：PIL/板级仿真联动（Renode/QEMU 路线）

- 输出：固件在仿真器内运行 + 与 plant sim 联动 + record/replay
- 目标：在不接硬件的情况下验证“固件级时序/外设接口”的一致性

---

## 建议的代码组织（便于逐步落地，不强制一次性重构）

现状是单 crate（编译器/验证/CLI 在一起）。为了把 SIL + runtime + 嵌入式后端逐步加进来，建议最终形态演进为 workspace（也可以先放在 `src/` 下，后续再拆 crate）：

- `crates/runtime-core/`：确定性 runtime（与平台无关）
- `crates/io-traits/`：最小 I/O trait（SimIO/板级 IO 都实现它）
- `crates/sim/`：SIL 驱动 + 场景/跑批/报告 + plant 模型
- `crates/codegen/`：IR 打包（字节码/内嵌）与可选的静态代码生成
- `crates/board-rp2040/`：RP2040 入口、HAL 适配、烧录/日志支持

目标：让“编译器验证”与“运行时执行/仿真/板级”在工程上解耦，但语义上通过回归测试严格对齐。
