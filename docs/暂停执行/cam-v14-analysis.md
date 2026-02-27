# 电子凸轮 v1.3 实施现状分析与 v1.4 升级评估

**日期**: 2026-02-26
**基于**: `ralph/electronic-cam-enhancement-v13-test-closure` 分支
**输入**: `docs/electronic-cam-enhancement.md` §12 + 代码审查 + 多驱动器类型分析

---

## 1. v1.3 实施完成度

### 1.1 prd.json 8 个 User Story 状态

全部 `passes: true`，cam regression gate 全绿（8 个测试点覆盖 parser → semantic → runtime → runtime_bridge → verification 五层）。

| US | 内容 | 关键产出 |
|---|---|---|
| US-001 | periodic 三次样条 | `compute_periodic_spline_coeffs()` 循环三对角求解，C2 连续性测试 |
| US-002 | 插值验收矩阵 | `binary_search_interval` 边界测试、线性精度测试、周期 wrap、oneshot clamp |
| US-003 | 运行时边界保护 | `TooManyCamCouplings`、`InvalidCamTableIndex`、`cam_phase` 越界 |
| US-004 | cam_switch 连续性 | gear_ratio≠1 + phase_offset≠0 下切表连续，switch_decay 衰减路径 |
| US-005 | cam 端口阈值 safety | `cam_xy.following_error > N` 语义建模 + 离散域构建 |
| US-006 | cam 因果链验证 | `encoder → cam → servo` 正例 + 断链反例 |
| US-007 | 设备库注入 + 示例闭环 | `flying_shear.plc` 纳入回归、`error_cam_missing_table.plc` 错误夹具 |
| US-008 | 无退化测试门禁 | `scripts/cam_regression_gate.sh` + CI 集成 |

测试数据：236 单元测试 + 19 集成测试全绿。

### 1.2 §12.1 "当前实现现状" 逐条验证

| §12.1 声明 | 代码位置 | 结论 |
|---|---|---|
| `master_input: AnalogInputId` / `slave_output: AnalogOutputId` / `slave_feedback: AnalogInputId` | `runtime-core/src/lib.rs:196-207` CamCouplingConfig | 准确 |
| 每 tick：读主轴 AI → 插值 → 写从轴 AO → 读反馈 AI → following_error | `update_cam_couplings()` (line 963-1010) | 准确 |
| runtime_bridge 解析 master/slave/slave_feedback 映射到 AI/AO | `runtime_bridge.rs:339-420` `build_cam_configs()` | 准确 |
| 测试中 AI0/AO0 夹具用于最小闭环验证 | `tests/runtime_bridge_us006.rs` PLC_CAM_FIXTURE | 准确 |

### 1.3 §12.2 "为什么先采用 AI/AO 抽象" 评估

四个设计理由均成立：

1. **共享基础设施** — `Io` trait 的 `read_analog_input` / `write_analog_output` 被 PID 和凸轮共用，零额外接口成本。
2. **执行确定性** — `update_cam_couplings` 是直接数组索引访问，无虚函数调度，RP2040 上执行时间可预测。
3. **no_std 兼容** — `MAX_CAM_COUPLINGS=8`、`MAX_CAM_POINTS=256` 编译期常量，固定数组。
4. **闭环优先** — cam regression gate 覆盖五层，8 个测试点全绿。

### 1.4 §12.3 "已识别的语义不足" 验证

**问题 1：主轴/从轴运动语义 vs AI/AO 映射的认知落差**

真实存在。`examples/flying_shear.plc` 用 `master: AI0, slave: AO0`，而文档 §7.1 理想示例用 `master: encoder_conv, slave: servo_knife`。`runtime_bridge.rs:339-420` 的 `build_cam_configs()` 已有设备名→AI/AO 的解析能力，但 flying_shear.plc 没有利用这个能力。

**问题 2：AI0/AO0 弱化设备层语义**

确认。底层单测用 AI0/AO0 合理，但面向用户的示例应该用设备名。

**问题 3：长期需要轴端点语义**

方向正确。当前 `CamCouplingConfig` 接口是 `AnalogInputId` / `AnalogOutputId`，如果要支持 `encoder.position` → AI 的映射，需要在 semantic/bridge 层增加一层抽象，不影响 runtime-core。

### 1.5 §12.4 "下一步升级计划" 可行性

| 升级项 | 可行性 | 工作量 | 风险 |
|---|---|---|---|
| 轴端点语义层 | 高 — bridge 层已有设备名解析基础 | 中 | 低 — 不动 runtime-core |
| 桥接策略升级（兼容优先） | 高 — 可做 fallback 链 | 低 | 低 — 旧路径保留 |
| 诊断与报错升级 | 高 — PlcError 已有结构化诊断 | 低 | 极低 |
| 示例与测试升级 | 高 — 框架已就绪 | 低 | 极低 |
| 验收标准补充 | 高 — cam regression gate 可扩展 | 低 | 极低 |

### 1.6 §12 未覆盖但值得关注的问题

1. **阶段 0（表达式引擎）完成度不明确**。runtime-core 中已有 `ExprOp` / `eval_expr`（用于 `cam_phase` offset 求值），但 DSL 层面的 `variable` 声明和 `compute` 语句的完整实现状态未在 §12 标注。

2. **SIL 仿真集成（阶段 3d）状态缺失**。prd.json 的 8 个 US 聚焦编译器/运行时/验证层，无 SIL 仿真 US。

3. **速度前馈未使用**。`cubic_derivative()` 已实现但 `update_cam_couplings()` 未调用。文档 §1.3 提到速度前馈需求，当前只做位置同步。

4. **故障自动脱开缺少专项测试**。`update_cam_couplings()` 中 `following_error > 3 * limit` 时自动 `engaged = false`，但无专门回归测试覆盖此路径。

---

## 2. 多驱动器类型分析

### 2.1 工业场景中凸轮实际驱动的设备类型

电子凸轮的从轴不限于伺服驱动器。工业现场常见的从轴类型：

| 从轴类型 | 典型场景 | 控制接口 | 凸轮输出含义 |
|---|---|---|---|
| 伺服驱动器 (ServoDrive) | 飞剪、印刷套色、旋转灌装 | 位置指令（脉冲或模拟量） | slave_cmd = 目标位置 |
| 步进电机 (StepperMotor) | 低成本包装机、贴标机、简易分拣 | 脉冲+方向（RP2040 PIO 生成） | slave_cmd = 目标位置 → HAL 层转换为脉冲频率 |
| 变频器 (VFD) | 输送带同步、泵站流量跟随、搅拌同步 | 频率指令（模拟量 0-10V 或通信） | slave_cmd = 目标速度（非位置） |
| 气动比例阀 | 旋转灌装阀门开度控制 | 模拟量 4-20mA | slave_cmd = 阀门开度 |
| 液压伺服阀 | 重型冲压、锻造 | 模拟量 ±10V | slave_cmd = 阀芯位移 |

核心区别：

- **伺服/步进**：凸轮输出是位置，following_error 是位置偏差
- **VFD**：凸轮输出是速度（位置凸轮的一阶导数），following_error 是速度偏差
- **比例阀/伺服阀**：凸轮输出是开度/位移，following_error 是开度偏差

### 2.2 当前 AI/AO 抽象的适配能力

当前架构的 AI/AO 抽象是**类型无关的数值通道**：

```
master_input: AnalogInputId   → 读主轴数值（不管主轴是编码器还是虚拟轴）
slave_output: AnalogOutputId  → 写从轴指令（不管从轴是什么类型）
slave_feedback: AnalogInputId → 读从轴反馈（不管反馈来源）
```

凸轮核心只做 `读数值 → 查表插值 → 写数值`，不关心下游驱动器类型。这意味着：

- **伺服**：AO 写位置指令 → 伺服驱动器接收位置 ✅ 已验证
- **VFD**：AO 写频率指令 → VFD 接收频率 ⚠️ 需要速度模式支持
- **步进**：AO 写目标位置 → RP2040 HAL 层将位置差转换为脉冲 ⚠️ 需要 HAL 层配合
- **比例阀**：AO 写开度值 → 比例阀接收 ✅ 直接可用

runtime-core 层面，当前设计**已经是驱动器类型无关的**。

### 2.3 已识别的 Gap

**Gap 1：VFD 场景需要速度输出模式**

伺服场景：凸轮表定义 `主轴位置 → 从轴位置`，直接输出位置指令。

VFD 场景有两种用法：
1. 直接定义速度凸轮表：`主轴位置 → 从轴速度`（凸轮表 slave 列就是速度值）
2. 定义位置凸轮表，运行时自动求导：`slave_vel = cubic_derivative(table, master_pos) * master_vel`

当前 `cubic_derivative()` 已实现但未在 `update_cam_couplings` 中使用。需要 `output_mode` 配置来选择输出模式。

**Gap 2：步进电机的位置→脉冲转换**

步进电机不接受模拟量位置指令，需要脉冲+方向信号。转换逻辑应在 HAL 层（rp2040-runner）：

```
delta_pos = new_pos - last_pos
pulse_count = delta_pos * steps_per_unit
pulse_freq = pulse_count / tick_period
```

凸轮核心只管输出目标位置到 AO，HAL 层负责转换。不影响 runtime-core 设计。

**Gap 3：following_error 语义因驱动器类型而异**

当前 `|slave_cmd - slave_actual|` 在数值上是通用的，但：
- 伺服：单位是度/mm，`following_error_limit` 典型值 2.0
- VFD：单位是 Hz，`following_error_limit` 典型值 0.5
- 故障阈值 `3 * limit` 的倍率对不同驱动器可能不合适

`following_error_limit` 已是用户可配置参数，数值层面不需要改代码。但文档和示例应说明不同驱动器下此参数的含义差异。

**Gap 4：DSL 层面缺少从轴类型语义提示**

`extract_cam_coupling_defs()` 只检查设备名是否存在，不检查设备类型。用户可以写 `slave: vfd_pump` 而不会收到任何提示。

不应限制从轴类型（各种类型都可能做从轴），但可以在语义层加 warning：当从轴是 VFD 且 `output_mode` 不是 `velocity` 时，提示用户可能需要速度模式。

### 2.4 output_mode 设计方案

在 `cam_coupling` 设备属性中新增 `output_mode`：

```plc
device cam_shear: cam_coupling {
    master: encoder_conv,
    slave: servo_knife,
    table: shear_cam,
    output_mode: position,          # 默认值
    ...
}

device cam_pump: cam_coupling {
    master: encoder_main,
    slave: vfd_pump,
    table: speed_cam,
    output_mode: velocity,          # VFD 速度跟随
    ...
}
```

| output_mode | 运行时行为 | 适用驱动器 |
|---|---|---|
| `position`（默认） | `slave_cmd = interpolate(table, adjusted_master)` | 伺服、步进 |
| `velocity` | `slave_cmd = cubic_derivative(table, adjusted_master) * master_velocity` | VFD |
| `direct` | `slave_cmd = interpolate(table, adjusted_master)`（凸轮表直接定义输出值） | 比例阀、自定义 |

`position` 和 `direct` 在运行时行为相同，区别是语义层面的：`position` 暗示从轴有位置反馈闭环，`direct` 暗示开环输出。这影响 following_error 的计算是否有意义。

`velocity` 模式需要额外读取主轴速度（两次采样的位置差 / tick 周期），在 `update_cam_couplings` 中增加：

```rust
if output_mode == Velocity {
    let master_vel = (state.master_pos - state.prev_master_pos) / tick_period;
    state.slave_cmd = cubic_derivative(table, lookup_pos) * master_vel;
    state.prev_master_pos = state.master_pos;
}
```

需要在 `CamState` 中增加 `prev_master_pos: f32` 字段。

---

## 3. 与 §12.4 升级计划的关系

§12.4 提出的 5 项升级计划聚焦于"轴端点语义"，即解决 AI/AO → encoder/servo 的认知落差。多驱动器类型支持是一个**正交但互补**的维度：

```
§12.4 解决的问题：用户写 AI0 还是 encoder_main？（命名语义）
多驱动器解决的问题：凸轮输出是位置还是速度？（输出语义）
```

两者可以独立推进，也可以合并到 v1.4：

- 轴端点语义层让用户写 `slave: vfd_pump` 而非 `slave: AO0`
- output_mode 让系统知道 `vfd_pump` 需要速度输出而非位置输出
- 两者结合后，语义层可以根据从轴设备类型自动推断 output_mode 默认值

---

## 4. 附录：当前系统设备类型端口清单

从 `semantic/mod.rs:420-452` `implicit_port_ids_for_device_type` 提取：

| 设备类型 | 端口列表 | 凸轮从轴适用性 |
|---|---|---|
| ServoDrive | enable, direction, pulse, clear_fault, ready, in_position, fault, zero_speed | 位置模式 ✅ |
| StepperMotor | enable, direction, pulse, fault | 位置模式（HAL 层转脉冲）⚠️ |
| Vfd | run, direction, running, fault, freq_arrive | 速度模式 ⚠️ |
| Motor | run, direction, running, fault, cmd, on | 速度/直接模式 |
| AnalogOutput | out | 直接模式 ✅ |
| CamCoupling | engage, in_sync, fault, following_error, master_pos, slave_cmd | — |
