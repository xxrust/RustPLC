# 电子凸轮 v1.4 任务规划：多驱动器类型支持 + 轴端点语义

**版本**: v1.4
**日期**: 2026-02-26
**前置**: v1.3 全部 8 个 US 已通过，cam regression gate 全绿
**输入**: `docs/cam-v14-analysis.md` 分析结论

---

## 1. 目标

在 v1.3 凸轮核心能力（插值、切换、跟随误差、验证闭环）基础上，解决两个正交问题：

1. **输出语义**：凸轮从轴不只是伺服，需要支持 VFD（速度输出）、步进（位置→脉冲）、比例阀（直接输出）等多种驱动器类型
2. **命名语义**：用户应写 `slave: servo_knife` 而非 `slave: AO0`，DSL 层面消除 AI/AO 认知落差

约束：
- runtime-core 保持 no_std 兼容
- 不破坏现有 v1.3 测试和示例
- AI/AO 直接引用作为 fallback 保留

---

## 2. 阶段划分

```
阶段 A：output_mode 支持（运行时输出语义）
  ├── A1: DSL + 解析 + AST + IR 增加 output_mode 属性
  ├── A2: runtime-core 支持 velocity 模式（cubic_derivative 集成）
  ├── A3: runtime_bridge 传递 output_mode 到运行时
  └── A4: 语义层 warning（VFD 从轴 + position 模式提示）
  （A1 → A2/A3 并行 → A4）

阶段 B：轴端点语义层（命名语义）
  ├── B1: semantic 层设备类型→端点映射规则
  ├── B2: runtime_bridge 端点优先 + AI/AO fallback 解析链
  ├── B3: 诊断升级（绑定失败时同时给出设备路径和通道路径）
  └── B4: flying_shear.plc 从 AI0/AO0 迁移到设备名引用
  （B1 → B2 → B3/B4 并行）

阶段 C：示例与回归
  ├── C1: 新增 VFD 从轴凸轮示例
  ├── C2: 新增步进从轴凸轮示例
  ├── C3: cam regression gate 扩展
  └── C4: 文档更新
  （C1/C2/C3/C4 相互独立）
```

阶段 A 和阶段 B 相互独立，可并行开发。阶段 C 依赖 A+B 完成。

---

## 3. 阶段 A：output_mode 支持

### A1: DSL + 解析 + AST + IR 增加 output_mode

**目标**：`cam_coupling` 设备属性支持 `output_mode: position | velocity | direct`。

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `plc.pest` | `attribute_name` 规则 | 新增 `"output_mode"` 关键字 |
| 2 | `plc.pest` | 新增规则 | `cam_output_mode = { "position" \| "velocity" \| "direct" }` |
| 3 | `ast/mod.rs` | `DeviceAttributes` | 新增 `pub output_mode: Option<String>` |
| 4 | `parser/mod.rs` | 属性解析 | 解析 `output_mode` 写入 `DeviceAttributes` |
| 5 | `ir/mod.rs` | `CamCouplingDef` | 新增 `pub output_mode: CamOutputMode` |
| 6 | `ir/mod.rs` | 新增枚举 | `enum CamOutputMode { Position, Velocity, Direct }` |

**验收标准**：
- `device cam_xy: cam_coupling { ..., output_mode: velocity }` 可解析为 AST
- 不指定时默认 `position`
- 无效值报语义错误

### A2: runtime-core 支持 velocity 模式

**目标**：`update_cam_couplings` 根据 `output_mode` 选择输出计算方式。

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `runtime-core/src/lib.rs` | `CamCouplingConfig` | 新增 `pub output_mode: CamOutputMode` |
| 2 | `runtime-core/src/lib.rs` | 新增枚举 | `enum CamOutputMode { Position, Velocity, Direct }` |
| 3 | `runtime-core/src/lib.rs` | `CamState` | 新增 `pub prev_master_pos: f32` |
| 4 | `runtime-core/src/lib.rs` | `update_cam_couplings` | 根据 output_mode 分支计算 slave_cmd |

**velocity 模式核心逻辑**：

```rust
CamOutputMode::Position | CamOutputMode::Direct => {
    state.slave_cmd = interpolate_cam(table, lookup_pos, cfg.interpolation);
}
CamOutputMode::Velocity => {
    // 主轴速度 = 位置差 / tick 周期
    let master_vel = state.master_pos - state.prev_master_pos;
    // 从轴速度 = 凸轮导数 × 主轴速度
    state.slave_cmd = cubic_derivative(table, lookup_pos) * master_vel;
}
// 所有模式结束后：
state.prev_master_pos = state.master_pos;
```

注意：`velocity` 模式下 `cubic_derivative` 要求 `CubicSpline` 插值。如果用户配置了 `interpolation: linear` + `output_mode: velocity`，语义层应报错（线性插值的导数是分段常数，不适合速度控制）。

**验收标准**：
- position 模式行为与 v1.3 完全一致（现有测试不回归）
- velocity 模式下，匀速主轴 + 线性凸轮表 → 从轴输出恒定速度
- velocity 模式下，主轴静止 → 从轴输出 0
- direct 模式行为与 position 相同

### A3: runtime_bridge 传递 output_mode

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `runtime_bridge.rs` | `build_cam_configs` | 从 IR `CamCouplingDef.output_mode` 映射到运行时 `CamCouplingConfig.output_mode` |

**验收标准**：
- bridge 测试验证 output_mode 正确传递

### A4: 语义层 warning

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `semantic/mod.rs` | `extract_cam_coupling_defs` | 查找从轴设备类型，当 `DeviceType::Vfd` + `output_mode != velocity` 时发出 warning |
| 2 | `semantic/mod.rs` | 同上 | 当 `interpolation: linear` + `output_mode: velocity` 时报语义错误 |

**验收标准**：
- VFD 从轴 + position 模式 → warning（不阻断编译）
- linear 插值 + velocity 模式 → 语义错误（阻断编译）

---

## 4. 阶段 B：轴端点语义层

### B1: 设备类型→端点映射规则

**目标**：在 semantic 层定义标准端点协议，让 `cam_coupling.master/slave` 可以引用设备名而非 AI/AO 名。

**端点映射表**：

| 设备类型 | 位置输出端点 | 位置反馈端点 | 速度输出端点 |
|---|---|---|---|
| ServoDrive | `cmd_pos` (AO) | `fb_pos` (AI) | `cmd_vel` (AO) |
| StepperMotor | `cmd_pos` (AO) | `fb_pos` (AI, 来自编码器) | — |
| Vfd | — | — | `cmd_freq` (AO) |
| Sensor (encoder) | — | `position` (AI) | — |
| AnalogOutput | `out` (AO) | — | — |
| AnalogInput | — | `in` (AI) | — |

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `semantic/mod.rs` | 新增函数 | `fn cam_endpoint_for_device(device_type, role) -> Option<&str>` |
| 2 | `semantic/mod.rs` | `extract_cam_coupling_defs` | 解析 master/slave 时先尝试端点映射，失败则 fallback 到直接设备名 |

**解析优先级**：
1. 如果 master/slave 值是已知设备名且设备类型有对应端点 → 使用端点映射
2. 如果 master/slave 值是 `AI0`/`AO0` 格式 → 直接使用（v1.3 兼容）
3. 否则 → 报错

**验收标准**：
- `master: encoder_main`（Sensor 类型）→ 自动映射到 encoder_main 的 AI 通道
- `slave: servo_knife`（ServoDrive 类型）→ 自动映射到 servo_knife 的 AO 通道
- `master: AI0` → 直接使用（兼容）
- `master: nonexistent` → undefined_reference 错误

### B2: runtime_bridge 端点优先解析链

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `runtime_bridge.rs` | `build_cam_configs` | 解析 master 时：先尝试 `resolve_analog_input_id`（设备名 BFS），失败则尝试直接 AI 名解析 |
| 2 | `runtime_bridge.rs` | 同上 | 解析 slave 时：先尝试 `resolve_analog_output_id`（设备名 BFS），失败则尝试直接 AO 名解析 |

当前 `build_cam_configs` 已经调用 `resolver.resolve_analog_input_id(&ctx, &cam.master)`，这个函数会做 BFS 查找设备名对应的 AI 节点。所以 bridge 层**可能已经支持设备名解析**，需要验证。

**验收标准**：
- `master: encoder_main` 在 bridge 层正确解析到对应 AI 通道
- `slave: servo_knife` 在 bridge 层正确解析到对应 AO 通道
- 解析失败时错误信息包含设备语义路径和通道路径

### B3: 诊断升级

**改动清单**：

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 1 | `runtime_bridge.rs` | `BridgeError` | 扩展 `UnresolvableAnalogInput` / `UnresolvableAnalogOutput` 错误信息，包含设备类型和期望端点 |

**验收标准**：
- 绑定失败时错误信息示例：`cam_coupling cam_xy 的 master 'encoder_main' (Sensor) 无法解析到模拟输入通道。请确认 encoder_main 连接了 analog_input 设备，或直接使用 AI0 格式。`

### B4: flying_shear.plc 迁移

**改动**：将 `examples/flying_shear.plc` 从 AI0/AO0 改为设备名引用。

**改前**：
```plc
device AI0: analog_input { range: 0..360, unit: "deg", external: true }
device AO0: analog_output { range: 0..360, unit: "deg" }
device cam_xy: cam_coupling {
    master: AI0,
    slave: AO0,
    slave_feedback: AI0,
    ...
}
```

**改后**：
```plc
device encoder_conv: sensor { detects: conveyor, response_time: 1ms }
device servo_knife: servo_drive { enable: true }
device AI0: analog_input { range: 0..360, unit: "deg", external: true }
device AO0: analog_output { range: 0..360, unit: "deg" }
relation { from: encoder_conv, to: AI0, via: reports_to }
relation { from: AO0, to: servo_knife, via: driven_by }
device cam_xy: cam_coupling {
    master: encoder_conv,
    slave: servo_knife,
    slave_feedback: encoder_conv,
    ...
}
```

**验收标准**：
- 迁移后 `examples_integration` 测试通过
- cam regression gate 全绿

---

## 5. 阶段 C：示例与回归

### C1: VFD 从轴凸轮示例

新增 `examples/cam_vfd_pump_sync.plc`：

```plc
[topology]
device encoder_main: sensor { detects: conveyor, response_time: 1ms }
device vfd_pump: vfd { max_freq: 50.0 }
device AI0: analog_input { range: 0..1000, unit: "pulse", external: true }
device AO0: analog_output { range: 0..50, unit: "Hz" }
relation { from: encoder_main, to: AI0, via: reports_to }
relation { from: AO0, to: vfd_pump, via: driven_by }

cam_table pump_speed_cam: periodic [
    (0, 10),
    (250, 30),
    (500, 50),
    (750, 30),
    (1000, 10),
]

device cam_pump: cam_coupling {
    master: encoder_main,
    slave: vfd_pump,
    table: pump_speed_cam,
    interpolation: cubic_spline,
    output_mode: velocity,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 2.0,
    slave_feedback: AI0,
}

[constraints]
safety: cam_pump.fault.on conflicts_with cam_pump.engage.on
causality: AI0 -> cam_pump -> AO0

[tasks]
task pump_control:
    step engage:
        action: cam_engage cam_pump
    step running:
        wait: cam_pump.in_sync == true
        timeout: 3000ms -> goto fault
        allow_indefinite_wait: true
    step fault:
        action: cam_disengage cam_pump
        allow_indefinite_wait: true
    on_complete: goto pump_control.engage
```

### C2: 步进从轴凸轮示例

新增 `examples/cam_stepper_labeler.plc`：

```plc
[topology]
device encoder_conv: sensor { detects: conveyor, response_time: 1ms }
device stepper_label: stepper_motor { steps_per_rev: 200 }
device AI0: analog_input { range: 0..360, unit: "deg", external: true }
device AO0: analog_output { range: 0..360, unit: "deg" }
relation { from: encoder_conv, to: AI0, via: reports_to }
relation { from: AO0, to: stepper_label, via: driven_by }

cam_table label_cam: periodic [
    (0, 0),
    (90, 0),
    (180, 180),
    (270, 360),
    (360, 360),
]

device cam_label: cam_coupling {
    master: encoder_conv,
    slave: stepper_label,
    table: label_cam,
    interpolation: cubic_spline,
    output_mode: position,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 5.0,
    slave_feedback: AI0,
}

[constraints]
safety: cam_label.fault.on conflicts_with cam_label.engage.on
causality: AI0 -> cam_label -> AO0

[tasks]
task label_control:
    step engage:
        action: cam_engage cam_label
    step running:
        wait: cam_label.in_sync == true
        timeout: 2000ms -> goto fault
        allow_indefinite_wait: true
    step fault:
        action: cam_disengage cam_label
        allow_indefinite_wait: true
    on_complete: goto label_control.engage
```

### C3: cam regression gate 扩展

在 `scripts/cam_regression_gate.sh` 中新增测试点：

| 测试点 | 覆盖内容 |
|---|---|
| output_mode 解析 | `output_mode: velocity` 可解析 |
| velocity 模式运行时 | 匀速主轴 + 线性表 → 恒定速度输出 |
| 端点映射正例 | `master: encoder_main` 正确解析 |
| 端点映射 fallback | `master: AI0` 兼容解析 |
| VFD 示例编译 | `cam_vfd_pump_sync.plc` 无错误 |
| 步进示例编译 | `cam_stepper_labeler.plc` 无错误 |

### C4: 文档更新

| 文档 | 更新内容 |
|---|---|
| `docs/electronic-cam-enhancement.md` | §4.1 cam_coupling 属性表增加 output_mode；§7 新增 VFD/步进示例；§12 更新实施现状 |
| `CLAUDE.md` | DSL 结构段增加 output_mode 说明 |

---

## 6. 验收测试矩阵

### 阶段 A

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| output_mode 解析 | `output_mode: position/velocity/direct` 可解析 | parser 单元测试 |
| output_mode 默认值 | 不指定时默认 position | parser 单元测试 |
| output_mode 无效值 | `output_mode: invalid` 报语义错误 | error fixture |
| velocity 模式正确性 | 匀速主轴 + 线性凸轮表 → 恒定速度输出 | runtime-core 单元测试 |
| velocity 模式静止 | 主轴静止 → 从轴输出 0 | runtime-core 单元测试 |
| position 模式不回归 | 现有 v1.3 测试全部通过 | cam regression gate |
| linear + velocity 报错 | `interpolation: linear` + `output_mode: velocity` → 语义错误 | error fixture |
| VFD warning | VFD 从轴 + position 模式 → warning | 语义测试 |

### 阶段 B

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| 设备名解析正例 | `master: encoder_main` → 正确 AI 通道 | bridge 单元测试 |
| 设备名解析正例 | `slave: servo_knife` → 正确 AO 通道 | bridge 单元测试 |
| AI/AO fallback | `master: AI0` → 直接使用 | bridge 单元测试 |
| 解析失败诊断 | 无法解析时错误信息包含设备类型和端点 | bridge 单元测试 |
| flying_shear 迁移 | 迁移后 examples_integration 通过 | 集成测试 |

### 阶段 C

| 测试项 | 验证内容 | 测试方式 |
|---|---|---|
| VFD 示例编译 | `cam_vfd_pump_sync.plc` 无错误 | examples_integration |
| 步进示例编译 | `cam_stepper_labeler.plc` 无错误 | examples_integration |
| cam gate 扩展 | 新增测试点全绿 | cam regression gate |
| 现有测试不回归 | 236+ 单元测试 + 19+ 集成测试全绿 | cargo test |

---

## 7. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| velocity 模式首 tick prev_master_pos=0 导致速度跳变 | 从轴首 tick 输出异常 | cam_engage 时初始化 prev_master_pos 为当前 master_pos |
| 端点映射 BFS 在复杂拓扑下解析到错误通道 | 绑定错误 | 保留 AI/AO 直接引用作为 fallback；BFS 歧义时报错而非猜测 |
| output_mode 增加 CamCouplingConfig 大小 | RP2040 内存 | 1 字节枚举，8 路凸轮共 8 字节，可忽略 |
| flying_shear.plc 迁移破坏现有用户 | 兼容性 | 旧写法（AI0/AO0）继续支持，迁移只改示例文件 |

---

## 8. 实施优先级

| 优先级 | 任务 | 理由 |
|---|---|---|
| P1 | A1 + A2 + A3（output_mode 核心） | 解锁 VFD 场景，复用已有 cubic_derivative |
| P1 | B4（flying_shear.plc 迁移） | 快速胜利，验证 bridge 层设备名解析能力 |
| P2 | A4（语义 warning） | 防止用户误配置 |
| P2 | B1 + B2（端点映射） | 提升 DSL 可读性 |
| P2 | C1 + C2（新示例） | 验证多驱动器场景 |
| P3 | B3（诊断升级） | 改善开发体验 |
| P3 | C3 + C4（gate + 文档） | 长期维护 |
