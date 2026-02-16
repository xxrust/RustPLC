# RP2040 本轮迭代说明（为什么做、做了什么）

## 为什么要做这轮

SIL 主线完成后，板级链路的瓶颈集中在三点：

1. **tick 时基不稳定风险**  
   之前固件用 `asm::delay` 近似 1ms，依赖 CPU 频率常量，时基漂移会放大 SIL vs 板级差异。

2. **模拟量等待缺少真实输入源**  
   虽然运行时已支持 `wait: AIx ...`，但板级 AI 仍是内存缓冲，无法反映真实 ADC 通道输入。

3. **端到端操作步骤分散**  
   构建、烧录、采集日志、解析与 diff 分成多条命令，人工执行容易漏步骤，回归门禁不稳定。

因此这轮目标是：**把板级时基、模拟输入、回归链路三个关键短板一次补齐**。

## 涉及的主要内容

### 1) 硬件定时器驱动 tick

- 文件：`crates/board-rp2040/src/main.rs`
- 改动：用 RP2040 `Timer` 驱动 `1ms` 循环节拍，替代固定 CPU cycle 的 busy delay 常量。
- 价值：板级时序更可解释，trace 对齐更稳定。

### 2) RP2040 真实 ADC 输入接入 + 工程量映射

- 文件：`crates/board-rp2040/src/main.rs`
- 改动：
  - 新增 `Adc` + `AdcPin` 管线；
  - 每个 tick 采样 `analog_inputs` 映射通道；
  - 新增 `analog_contract.toml`（由 `build-rp2040` 生成），把 ADC 电压线性映射到 DSL 的工程量 `range`。
- 价值：`wait: AIx ...` 在板上可由真实传感链触发，并与 DSL 阈值语义对齐。

### 3) AO 真实输出（PWM + ramp）

- 文件：`crates/board-rp2040/src/main.rs`、`crates/board-rp2040/build.rs`、`src/main.rs`
- 改动：
  - AO 通道按 `io_map.toml` 映射到 RP2040 PWM；
  - `set_analog` 值按 `analog_contract.toml` 的 `min/max` 归一化到 duty；
  - `ramp_ms` 在固件 tick 循环内实现最小斜坡。
- 价值：AO 不再是内存缓冲，板上输出可用于真实执行器链路。

### 3.5) 模拟量标定入口（scale/offset）

- 文件：`src/main.rs`、`crates/board-rp2040/build.rs`、`crates/board-rp2040/src/main.rs`
- 改动：
  - `build-rp2040` 新增 `analog_calibration.template.toml`（可选标定模板）；
  - 支持 `build-rp2040 --analog-calibration <file>` 把 AI/AO 的 `scale/offset` 合并写入 `analog_contract.toml`；
  - 固件侧在 AI 映射后、AO 下发前统一应用：`eng_cal = eng_raw * scale + offset`。
- 价值：把“板级偏置/斜率”从代码里拿出来，作为可版本化资产进入回归链路。

### 4) I/O 合同补强（AI 引脚约束）

- 文件：`src/io_map.rs`、`crates/board-rp2040/build.rs`
- 改动：
  - 对 RP2040 的 `[analog_inputs]` 强制约束为 GPIO `26..=29`；
  - 非 ADC 引脚在构建期直接报错。
- 价值：把“硬件能力边界”前移到构建期，避免运行时隐性失败。

### 5) 回归门禁脚本（板级 + PIL）

- 文件：`scripts/rp2040_trace_gate.sh`
- 改动：提供可复用流水线脚本，串联：
  1. `build-rp2040 --emit-uf2`
  2. `flash-rp2040`（dry-run + actual）
  3. 板级日志采集（serial/cmd）
  4. `trace-parse` + `trace-diff --fail-on-mismatch`
- 价值：减少人工步骤差异，让“板级回归”可复制、可门禁。

- 文件：`scripts/pil_trace_gate.sh`
- 改动：新增 PIL 样式 trace gate（无需实板烧录，runner 可替换为 Renode 或任意日志生产命令）。
- 价值：让“无实板回归”也能走标准 `trace-parse` + `trace-diff` 门禁。

### 6) 文档与流程固化

- 文件：`README.md`、`docs/board_rp2040.md`、`docs/board_semantics_contract_v1.md`、`examples/rp2040_end_to_end/*`
- 改动：
  - 明确 `analog_contract.toml`、AI/AO 语义、PIL/板级脚本入口；
  - 新增 Board Semantics Contract v1；
  - 新增端到端示例包（含 `.plc` / `io_map.toml` / `scenario` / 操作说明）。

## 当前边界（明确说明）

- AI/AO 默认换算模型为线性映射（`0.0..3.3V -> min..max`）；复杂传感器曲线仍需后续标定层。
- PIL 脚本已就绪，但具体 Renode 平台脚本仍按项目硬件模型补充（runner-cmd 可先接已有日志源）。
- `sim-regress --minimize-failure` 会输出最小失败用例（缩短 duration、移除无关输入/故障），但目前仅做结构化删减（不做连续参数求解/最小化）。

## 验证状态

- 已通过：`cargo test --workspace`
- 已通过：`cargo build -p board-rp2040 --target thumbv6m-none-eabi --release`
- 板级链路建议：`scripts/rp2040_trace_gate.sh`
- 无板链路建议：`scripts/pil_trace_gate.sh`
