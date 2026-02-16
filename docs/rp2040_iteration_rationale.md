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

### 2) RP2040 真实 ADC 输入接入

- 文件：`crates/board-rp2040/src/main.rs`
- 改动：
  - 新增 `Adc` + `AdcPin` 管线；
  - 每个 tick 采样 `analog_inputs` 映射通道；
  - 运行时读取的 AI 值改为真实采样值（当前单位为电压 V，`0.0..3.3`）。
- 价值：`wait: AIx ...` 在板上可由真实传感链触发，不再是占位缓冲。

### 3) I/O 合同补强（AI 引脚约束）

- 文件：`src/io_map.rs`、`crates/board-rp2040/build.rs`
- 改动：
  - 对 RP2040 的 `[analog_inputs]` 强制约束为 GPIO `26..=29`；
  - 非 ADC 引脚在构建期直接报错。
- 价值：把“硬件能力边界”前移到构建期，避免运行时隐性失败。

### 4) 一键化回归门禁脚本

- 文件：`scripts/rp2040_trace_gate.sh`
- 改动：提供可复用流水线脚本，串联：
  1. `build-rp2040 --emit-uf2`
  2. `flash-rp2040`（dry-run + actual）
  3. 板级日志采集（serial/cmd）
  4. `trace-parse` + `trace-diff --fail-on-mismatch`
- 价值：减少人工步骤差异，让“板级回归”可复制、可门禁。

### 5) 文档同步更新

- 文件：`README.md`、`docs/board_rp2040.md`
- 改动：
  - 明确 AI 映射范围与电压语义；
  - 新增 trace gate 脚本使用方式；
  - 更新 RP2040 当前能力边界说明。

## 当前边界（明确说明）

- AI 当前输出语义是**电压值**（V），不是自动工程量（bar/℃ 等）。
- 若 DSL 阈值使用工程单位，需要外部传感链或后续标定层完成换算。

## 验证状态

- 已通过：`cargo test --workspace`
- 板级链路建议回归：使用 `scripts/rp2040_trace_gate.sh` 对实际板卡执行 trace gate。
