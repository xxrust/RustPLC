# RP2040 运动控制示例

从 .plc 到 Raspberry Pi Pico 固件的完整路径。

---

## 文件清单

| 文件 | 说明 |
|------|------|
| `examples/rp2040_motion_minimal.plc` | 双轴运动控制 PLC 程序 |
| `examples/rp2040_motion_minimal.io_map.toml` | I/O 映射（GPIO/ADC 分配） |
| `scenarios/rp2040_motion_minimal/*.yaml` | 场景文件（正常 + 故障） |
| `tests/rp2040_motion_minimal_scenarios.rs` | CI 回归测试 |

---

## 通道约定

### Axis0

| 功能 | 通道 | 说明 |
|------|------|------|
| 使能 | DO24 | 步进驱动器使能 |
| 方向 | DO25 | 运动方向 |
| 速度指令 | AO24 | 脉冲频率 (steps/s) |
| 编码器计数 | AI24 | AB 编码器位置反馈 |
| 编码器速度 | AI25 | 速度反馈 |
| 编码器方向 | DI24 | 正方向标志 |

### Axis1

| 功能 | 通道 | 说明 |
|------|------|------|
| 使能 | DO26 | 步进驱动器使能 |
| 方向 | DO27 | 运动方向 |
| 速度指令 | AO26 | 脉冲频率 (steps/s) |
| 编码器计数 | AI26 | AB 编码器位置反馈 |
| 编码器速度 | AI27 | 速度反馈 |
| 编码器方向 | DI26 | 正方向标志 |

在 `io_map.toml` 中将这些通道映射为 `"virtual"` 可在无物理板卡时运行仿真。

---

## 操作步骤

### 1. 验证场景

```bash
cargo run --release -- scenario-validate examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/normal.yaml
```

### 2. SIL 仿真

```bash
cargo run --release -- sim-plc examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/normal.yaml \
  --out out/rp2040_motion_minimal.normal.trace.jsonl
```

### 3. 无板门禁

```bash
cargo run --release -- no-board-gate examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/normal.yaml \
  --out-dir out/gate/rp2040_motion_minimal --output human
```

### 4. 构建固件

```bash
# 安装交叉编译目标
rustup target add thumbv6m-none-eabi

# 生成固件构建输入
cargo run --release -- build-rp2040 examples/rp2040_motion_minimal.plc \
  --out out/rp2040 \
  --io-map examples/rp2040_motion_minimal.io_map.toml \
  --emit-uf2 out/firmware.uf2

# 烧录到 Pico
cargo run --release -- flash-rp2040 --uf2 out/firmware.uf2 --mount /media/RPI-RP2
```

### 5. CI 回归

```bash
cargo test -p rust_plc --test rp2040_motion_minimal_scenarios
```

---

## 安全态配置

`io_map.toml` 的 `[safe_state]` 段定义固件异常时的安全输出：

```toml
[safe_state]
mode = "profile"

[safe_state.do.Y2]
safe_value = 0
group = 10

[safe_state.ao.AO0]
safe_value = 0.0
group = 30
```

- `mode="all_zero"` — 所有输出置零（默认）
- `mode="profile"` — 按 group 顺序写入指定安全值
- `group` 数值越小越先执行

详见 [故障安全状态](Fail-Safe-Safe-State.md)。

---

## 排错

- `scenario-validate` 报不匹配 → 用 `scenario-init` 重新生成骨架
- 回归测试失败 → 检查 trace JSONL，确认故障场景有 `reason == "timeout"` 的转换
- 交叉编译失败 → 确认已安装 `thumbv6m-none-eabi` 目标

---

## 相关文档

- 运动控制设计：`docs/已实现/board_rp2040.md`
- I/O 映射格式：`docs/已实现/motion_io_map_format_delta.md`
- 步进电机安全建模：[Stepper-AB-Encoder-Safety-Modeling](Stepper-AB-Encoder-Safety-Modeling.md)
