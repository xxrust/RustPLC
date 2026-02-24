# RP2040 Motion io_map 格式变更说明（old -> new）

日期：2026-02-19

本文用于解释 RP2040 motion（脉冲/方向步进 + AB 编码器）引入后，`io_map.toml` 的格式变化，以及为什么要新增 `"virtual"` 绑定与 `[motion.*]` 配置段。

适用对象：
- 需要在 `crates/board-rp2040` 固件上使用 motion 子系统的人
- 需要把“运动命令/反馈”暴露给 RustPLC DSL（作为 AI/DI）的人

相关文档：
- `docs/board_rp2040.md`（RP2040 板级流程与 motion 配置模板）
- `docs/motion_virtual_channels.md`（motion 信号如何映射为 DSL 可见的虚拟通道）

## 1. 变更动机

Motion 子系统同时涉及：

1) 物理引脚（STEP/DIR/EN、Encoder A/B）
2) PLC 侧可见信号（enable/dir/vel_cmd/count/speed/enc_dir_positive）

如果把 2) 也映射到真实 GPIO/ADC，会引发两个问题：

- 运动反馈（count/speed）不应该消耗 ADC 引脚（RP2040 ADC 只有 GPIO 26..29）
- 运动通道在 DSL 层应当是“工程信号”，并非真实电气输入/输出（由固件子系统合成/消费）

因此引入：

- `"virtual"`：表示该 DI/DO/AI/AO 通道不绑定物理引脚，由固件子系统消费/发布
- `[motion.stepper.axis*]` / `[motion.encoder.axis*]`：集中声明真实运动引脚与参数，避免散落在代码里

## 2. old 格式（仅 DI/DO/AI/AO -> GPIO）

早期 `io_map.toml` 只允许把通道映射到 GPIO 数字：

```toml
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16

[analog_inputs]
ai0 = 26

[analog_outputs]
ao0 = 20
```

## 3. new 格式（加入 virtual + motion 段）

### 3.1 `"virtual"` 绑定

new 格式允许把 DI/DO/AI/AO 绑定写成字符串 `"virtual"`：

- virtual DI：返回板级子系统发布的合成值（例如 motion 的 enc_dir_positive）
- virtual DO：只锁存 PLC 输出值，不驱动引脚
- virtual AI：不采样 ADC，可由板级子系统发布合成值（例如 motion 的 count/speed）
- virtual AO：只锁存 PLC 输出值，不驱动 PWM

示例（motion 通道推荐用 virtual）：

```toml
[digital_inputs]
di24 = "virtual" # axis0 enc_dir_positive
di26 = "virtual" # axis1 enc_dir_positive

[digital_outputs]
do24 = "virtual" # axis0 enable
do25 = "virtual" # axis0 dir
do26 = "virtual" # axis1 enable
do27 = "virtual" # axis1 dir

[analog_inputs]
ai24 = "virtual" # axis0 count
ai25 = "virtual" # axis0 speed
ai26 = "virtual" # axis1 count
ai27 = "virtual" # axis1 speed

[analog_outputs]
ao24 = "virtual" # axis0 vel_cmd_sps
ao26 = "virtual" # axis1 vel_cmd_sps
```

### 3.2 `[motion.*]` 配置段

new 格式允许新增 `motion` 配置段，用于声明真实运动引脚与参数：

```toml
[motion.stepper.axis0]
step_gpio = 2
dir_gpio = 3
en_gpio = 4
dir_inverted = false
v_max_sps = 20000
acc_sps2 = 40000
dec_sps2 = 40000

[motion.encoder.axis0]
a_gpio = 8
b_gpio = 9
ppr = 1024
quad = 4
count_sign = "normal"  # normal | inverted
scale = 1.0
```

完整 dual-axis 模板见 `docs/board_rp2040.md`。

## 4. old vs new：字段对照与推荐写法

当前 dev-stage 约定（固件固定消费/发布以下通道 ID）：

- Axis0
  - Commands: `DO24` enable, `DO25` dir, `AO24` vel_cmd_sps
  - Feedback: `AI24` count, `AI25` speed, `DI24` enc_dir_positive
- Axis1
  - Commands: `DO26` enable, `DO27` dir, `AO26` vel_cmd_sps
  - Feedback: `AI26` count, `AI27` speed, `DI26` enc_dir_positive

推荐做法：

- 在 `.plc` topology 里显式声明这些通道，并用逻辑别名连接（便于 review/验证/写 scenario）。
- 在 `io_map.toml` 里将这些 motion 通道映射成 `"virtual"`，避免占用真实 GPIO/ADC。
- 用 `[motion.*]` 段集中声明 stepper 与 encoder 的物理引脚与参数。

## 5. 输出/Trace 格式变化说明

### 5.1 Trace JSONL

Motion 引入后，trace JSONL 的结构不需要改变，仍是 step 迁移事件（如 `tick/task/from_step/to_step/reason`）。

差异主要体现在：PLC 会新增 `axis*_count/axis*_speed/axis*_enc_dir_positive` 等通道；这些值在固件上来自 motion 子系统（virtual AI/DI）。

### 5.2 build-rp2040 输出物

`build-rp2040` 输出目录结构保持不变；生成的 `io_map.template.toml` 会增加：

- `"virtual"` 说明
- `[motion.*]` 可选模板段

## 6. 可复现示例

- `examples/rp2040_motion_minimal.plc`
- `examples/rp2040_motion_minimal.io_map.toml`
- `scenarios/rp2040_motion_minimal/*.yaml`

本地回归：

```bash
cargo test -p rust_plc --test rp2040_motion_minimal_scenarios
```

