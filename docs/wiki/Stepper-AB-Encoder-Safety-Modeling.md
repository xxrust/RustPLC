# 步进电机 + AB 编码器安全建模

工业场景中最常见的运动控制组合：步进电机（STEP/DIR/EN）+ 增量式 AB 编码器。RustPLC 的建模原则是严格分层 — DSL 只管状态机和安全约束，驱动层负责脉冲生成和编码器解码。

---

## 分层原则

| 层 | 职责 | 可验证性 |
|---|---|---|
| DSL（RustPLC） | 顺序控制、联锁、wait 条件、安全约束 | 四引擎形式化验证 |
| 驱动/板卡层 | 脉冲生成、AB 解码、计数、滤波、单位换算 | 实时性保证，不在 DSL 验证范围 |

驱动层的输出以简单信号（digital/analog）回馈 DSL，保持验证可处理。

---

## zone_code：碰撞窗口编码

`zone_code` 是推荐的碰撞窗口抽象信号：

- 拓扑中建模为 `analog_input { external: true }`
- 语义：`0 = 安全`，`1..N = 碰撞窗口`
- 由驱动层产生（含迟滞/LUT/几何逻辑），DSL 消费

好处：
- 复杂几何/窗口逻辑留在驱动层，不污染 DSL
- 产生稳定、可审查的 `safety:` 规则
- 窗口变更是驱动层配置变更，不需要改 DSL

---

## 双向联锁（最小组合）

碰撞防护不能只写单向规则。推荐最小组合：

1. **窗口侧联锁**（状态侧）：`zone_code != 0` 时禁止危险姿态
2. **指令侧联锁**（命令侧）：发出运动指令时也禁止危险姿态

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true }
device move_cmd: digital_output
device cyl_clamp: cylinder

[constraints]
# 窗口侧：碰撞窗口内禁止危险姿态
safety: zone_code > 0 conflicts_with cyl_clamp.extended

# 指令侧：运动指令只在安全姿态下合法
safety: move_cmd.on conflicts_with cyl_clamp.extended

[tasks]
task cycle:
    step hold:
```

---

## 标准信号集

推荐的拓扑抽象接口信号：

| 类型 | 信号 | 说明 |
|------|------|------|
| Analog | `axis_count` | 编码器计数（主坐标） |
| Analog | `axis_theta` | 角度（派生） |
| Analog | `axis_pos_mm` | 线性位置（派生） |
| Analog | `axis_speed` | 速度 |
| Digital | `range_valid` | 量程有效 |
| Digital | `pos_consistent` | 位置一致性（编码器 vs 外部传感器） |
| Digital | `inpos` | 到位 |
| Digital | `alarm` | 报警 |
| Analog | `zone_code` | 碰撞窗口编码 |

坐标换算（count → theta → pos_mm）在驱动层完成，DSL 只消费输出信号。

---

## 常见反模式

| 反模式 | 问题 |
|--------|------|
| 只写窗口侧联锁，忘记指令侧 | 运动指令发出时无保护 |
| 在 DSL 中用多阈值编码窗口 | 难维护、难表达迟滞、容易出错 |
| 把 AB 原始边沿暴露给 DSL | 应先解码为 count/speed/dir |

---

## 回归覆盖

配合安全建模的场景回归最小集：

| 场景 | 覆盖 |
|------|------|
| 正常运动 | safe → move → stop/inpos |
| 计数卡死 | count stuck → timeout |
| 方向错误 | wrong direction / bad sign |
| 报警触发 | alarm → fault |

可复制的仓库 fixture：
- `examples/stepper_collision_guard.plc` + `scenarios/stepper_collision_guard/*.yaml`
- `examples/rp2040_motion_minimal.plc` + `scenarios/rp2040_motion_minimal/*.yaml`

CI 门禁：`tests/rp2040_motion_minimal_scenarios.rs`

---

## 相关文档

- 设计文档：`docs/已实现/stepper_ab_encoder.md`
- 场景工作流：`docs/已实现/scenario_playbook.md`
- 拓扑抽象：[Topology-Abstraction-PLS-Angle-Distance](Topology-Abstraction-PLS-Angle-Distance.md)
- RP2040 示例：[RP2040-Motion-Minimal-Example](RP2040-Motion-Minimal-Example.md)
