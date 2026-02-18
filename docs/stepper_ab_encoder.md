# 脉冲/方向步进与 AB 编码器接入指南（RustPLC）

日期：2026-02-18

## 1. 适用范围

本文面向以下场景：

- 执行器是 **脉冲/方向（Pulse/Dir）步进轴**（含步进驱动器）。
- 反馈是 **AB 相编码器**（增量式），需要高速计数与方向判定。
- 你希望继续使用 RustPLC 做顺控与形式化验证，而不是把高速运动细节塞进 DSL。

## 2. 核心原则（先看这个）

RustPLC 里建议采用“**分层**”方案：

1. **DSL 层（可验证）**：只表达顺序、互锁、等待条件、安全约束。
2. **I/O/驱动层（实时）**：负责高频脉冲输出、AB 解码、高速计数、速度估计。
3. **反馈回灌 DSL**：把 `inpos/busy/alarm/count/speed` 等结果作为离散或模拟输入给 DSL。

> 不建议在 DSL 中直接生成高频 STEP 脉冲。DSL 的 tick 语义更适合“过程控制决策”，不适合“微秒级运动控制”。

### 2.1 本期边界与非目标（防 scope 膨胀）

本期明确 **不实现** 以下能力：

- 实时脉冲轨迹规划（如 jerk/s-curve 在线规划、细粒度运动控制环）
- 复杂运动学在线求解（如高维联动逆解在 DSL 内求解）
- 原始 AB 边沿在 DSL 直接解码（AB 解码必须在驱动/板级层完成）

本期明确 **实现边界**：

- DSL 负责顺控、互锁、安全约束与状态迁移
- 驱动/板级层负责高速计算（脉冲输出、AB 解码、计数/速度/换算）并回灌工程信号给 DSL

评审口径：凡未在“实现边界”中明确列出的能力，均视为非本期承诺，避免“隐含新增能力”歧义。

## 3. 推荐信号模型

### 3.1 步进轴（Pulse/Dir）

最小信号集合：

- 输出（PLC -> 驱动器）
  - `STEP`：脉冲（由驱动层产生，不建议 DSL 逐脉冲控制）
  - `DIR`：方向
  - `EN`：使能
  - `START_MOVE`（可选）：启动一段运动命令
- 输入（驱动器 -> PLC）
  - `INPOS`：到位
  - `BUSY`：运动中
  - `ALARM`：故障

### 3.2 AB 编码器

驱动层/板级层输出以下“已计算结果”给 PLC：

- `count`（累计计数，建议 64-bit 内部累计后映射）
- `speed`（计数差分得到，可用 count/s 或 mm/s）
- `dir_sign`（可选，+1/-1，可映射为离散输入）
- `z_latched`（可选，若有 Z 相零位）

在 DSL 中通常只用：

- `count >= target`
- `abs(speed) <= stop_threshold`
- `ALARM == false`

## 4. RustPLC 拓扑建模模板

```plc
[topology]
# 端口
device Y0: digital_output  # DIR
device Y1: digital_output  # EN
device X0: digital_input   # INPOS
device X1: digital_input   # ALARM

# 反馈量（外部驱动层提供）
device AI0: analog_input { range: 0..4000000, unit: "count", external: true }
device AI1: analog_input { range: 0..200000, unit: "count_s", external: true }

# 逻辑设备（把步进轴当 motor 进行顺控）
device axis_x: motor {
    connected_to: Y1
    ramp_time: 10ms
}

device inpos_x: sensor {
    connected_to: X0
    detects: axis_x.position_ok
}

device alarm_x: sensor {
    connected_to: X1
    detects: axis_x.alarm
}

[constraints]
safety: axis_x.on requires alarm_x.off

[tasks]
task cycle:
    step enable_axis:
        action: set axis_x on

    step set_dir_positive:
        action: set Y0 on

    # 实际脉冲运动由驱动层执行；DSL 等待反馈
    step wait_target_count:
        wait: AI0 >= 120000
        timeout: 3000ms -> goto fault

    step wait_inpos:
        wait: X0 == true
        timeout: 1000ms -> goto fault

    on_complete: goto idle

task fault:
    step stop:
        action: set axis_x off
    on_complete: goto idle

task idle:
    step hold:
```

说明：

- `AI0/AI1` 标记为 `external: true`，表示其值由外部采样/计算提供。
- `axis_x` 在 DSL 里承担“控制对象”角色；高频 Step 细节放在驱动层。
- 若板级部署需更强兼容性，建议 wait 条件保持单条件、分步写法。

## 5. 板级/驱动层实现建议

### 5.1 Pulse/Dir 输出

建议在板级层提供“运动命令接口”，而不是“每 tick 发一脉冲”：

- 命令：`target_count`、`vmax`、`acc`、`dir`
- 执行：硬件定时器/PIO/PWM 生成 STEP
- 状态：`busy/inpos/alarm`

DSL 只负责“何时下命令、何时允许下一步”。

### 5.2 AB 解码与高速计数

- 使用硬件外设（QEI/定时器编码器模式/PIO）优先。
- 计数器内部建议用 `i64` 累计，避免溢出后语义不清。
- 每个 PLC tick 输出一次快照：
  - `count_snapshot`
  - `speed_estimate = (count_now - count_prev) / dt`
- 再映射到 RustPLC 的 `analog_input` 通道。

### 5.3 单位与量纲

- `count`：建议整数计数，DSL 用阈值。
- `speed`：建议统一成 `count_s` 或 `mm_s`。
- 若做 `count -> mm` 转换，固定齿距/丝杆导程参数应放在驱动层配置，不放 DSL。

## 6. 规则模板：角度范围与执行器互斥（防碰撞）

这是步进机构最关键的建模点：**“危险角度窗口” 与 “其他执行器动作” 互斥**。

> 回归场景怎么写/怎么校验/怎么批量回归：见 [Scenario Playbook](scenario_playbook.md)。

### 6.0 术语与定义（建模基线）

本节给出可复用的最小安全抽象，后续示例与回归都以此为基准。

**zone_code（区间编码 / 碰撞窗口编码）**

- 由驱动层计算并回灌给 PLC 的离散工程信号，推荐建模为 `analog_input { external: true }`。
- 语义约定：`0` = 安全区（safe），`1..N` = 禁入区/危险窗口（collision window）。
- 目标：把区间判定、回差、LUT 等复杂计算下沉到驱动层，DSL 只消费编码结果做互锁。

**危险窗口（danger window / collision window）**

- 以某个主坐标（如 `axis_theta_deg` / `axis_count` / `axis_pos_mm`）定义的危险区间集合。
- 进入/退出建议采用带回差判定以抑制边界抖动；DSL 侧不直接表达该算法，仅接收 `zone_code`。

**双向互锁（bi-directional interlock）**

- 窗口互锁（状态侧）：`zone_code != 0` 时，禁止执行器进入危险姿态。
- 命令互锁（命令侧）：`move_cmd.on` 时，要求执行器已处于安全姿态。

最小组合规则（必须同时具备）：

1. `zone_code != 0 -> 禁止执行器危险状态`
2. `move_cmd.on -> 禁止执行器危险状态`（或等价 `requires`）

### 6.1 单阈值互斥（可直接写）

当约束是单边界时，可直接使用模拟量阈值和状态互斥：

```plc
[topology]
device axis_theta: analog_input { range: 0..360, unit: "deg", external: true }
device cyl_clamp: cylinder

[constraints]
safety: axis_theta > 120 conflicts_with cyl_clamp.extended

[tasks]
```

### 6.2 区间互斥（推荐：先信号化再互斥）

如果是“角度在 `[low, high]` 区间内禁止某机构动作”，推荐在驱动层先生成**区间编码**，再在 DSL 做互斥：

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true } # 0=safe, 1..N=collision window
device cyl_clamp: cylinder

[constraints]
safety: zone_code > 0 conflicts_with cyl_clamp.extended

[tasks]
```

说明：`safety` 规则是二元关系，表达“单个条件 与 单个状态”的冲突/依赖最直接；区间逻辑建议先在驱动层折叠成 `zone_code`，不要在 DSL 里拼接多条阈值规则去模拟同一窗口。

为什么推荐这样做：

- 语义更清晰：碰撞窗口是一个明确的工程信号（`zone_code`）。
- 验证更稳定：避免把复杂区间逻辑硬塞进单条 safety 规则。
- 工程更可维护：以后窗口变化只改驱动层配置（阈值或 LUT），不改顺控结构。

实现建议（驱动层）：

- 用带回差（hysteresis）的窗口判定，避免边界抖动：
  - 进入窗口：`theta >= low_enter && theta <= high_enter`
  - 退出窗口：`theta <= low_exit || theta >= high_exit`（`low_exit < low_enter`，`high_exit > high_enter`）

### 6.3 多执行器碰撞矩阵

若一个旋转机构会与多个执行器发生几何干涉，建议每个干涉关系单独建“区间信号”：

- `zone_for_clamp`
- `zone_for_press`
- `zone_for_eject`

然后写成多条独立 safety 规则，而不是一条超长复合规则。

可复制模板：

```plc
[topology]
device zone_for_clamp: analog_input { range: 0..3, unit: "zone", external: true }
device zone_for_press: analog_input { range: 0..3, unit: "zone", external: true }
device cyl_clamp: cylinder
device cyl_press: cylinder

[constraints]
safety: zone_for_clamp > 0 conflicts_with cyl_clamp.extended
safety: zone_for_press > 0 conflicts_with cyl_press.extended

[tasks]
```

### 6.4 双向互锁（推荐做成“命令互锁 + 窗口互锁”）

仅写“禁区 -> 禁止执行器动作”还不够；更稳妥的是再加一条反向约束：**当执行器处于危险姿态时，禁止轴继续运动**。

推荐组合：

- 窗口互锁（状态侧）：`zone_code != 0` 时，禁止 `cyl_clamp` 伸出。
- 命令互锁（命令侧）：`move_cmd` 发出时，必须保证 `cyl_clamp` 已缩回（或处于安全姿态）。

示例：

```plc
[topology]
device move_cmd: digital_output
device zone_code: analog_input { range: 0..3, unit: "zone", external: true }
device cyl_clamp: cylinder

[constraints]
safety: zone_code > 0 conflicts_with cyl_clamp.extended
safety: move_cmd.on conflicts_with cyl_clamp.extended

[tasks]
```

这样即使驱动层/机构存在漂移、回差或外力扰动，控制侧也不会继续“推着进入”危险窗口。

### 6.5 正反例（推荐写法/反模式）

下面给出两组“推荐写法 vs 反模式”，用于在评审时快速对齐建模基线。所有片段都刻意保持为“可解析的最小 PLC 文件”，便于作为回归夹具。

#### 例 1：推荐（双向互锁） vs 反模式（只写窗口互锁）

推荐（同时具备窗口互锁 + 命令互锁）：

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true }
device move_cmd: digital_output
device cyl_clamp: cylinder

[constraints]
safety: zone_code > 0 conflicts_with cyl_clamp.extended
safety: move_cmd.on conflicts_with cyl_clamp.extended

[tasks]
```

反模式（只写窗口互锁，缺少命令互锁；当夹爪已伸出时仍可能继续下发运动命令）：

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true }
device move_cmd: digital_output
device cyl_clamp: cylinder

[constraints]
safety: zone_code > 0 conflicts_with cyl_clamp.extended

[tasks]
```

#### 例 2：推荐（zone_code 折叠窗口） vs 反模式（在 DSL 里拼阈值模拟窗口）

推荐（窗口判定下沉驱动层，DSL 只消费 `zone_code`）：

```plc
[topology]
device zone_code: analog_input { range: 0..3, unit: "zone", external: true }
device cyl_clamp: cylinder

[constraints]
safety: zone_code > 0 conflicts_with cyl_clamp.extended

[tasks]
```

反模式（试图用多条阈值在 DSL 里“拼”一个区间窗口；不仅难维护，还很容易表达成错误的过宽/过窄约束，并且无法自然表达回差）：

```plc
[topology]
device axis_theta: analog_input { range: 0..360, unit: "deg", external: true }
device cyl_clamp: cylinder

[constraints]
safety: axis_theta > 120 conflicts_with cyl_clamp.extended
safety: axis_theta < 240 conflicts_with cyl_clamp.extended

[tasks]
```

## 7. 拓扑抽象：从 PLS 到角度/距离的建模

你提到的 `pls -> 角度 -> 距离`，本质是同一运动链的多种坐标。推荐做法是：

### 7.1 选一个“主坐标”做控制闭环

- 原则：**主坐标 + 派生坐标**。同一运动链允许多个坐标同时“可见”，但只能有一个作为控制与安全互锁的“真值源”（主坐标）。
- 常见主坐标：`count`（编码器计数）或 `pos_mm`（机构实际位移）。
- 其他量（`theta_deg`, `speed`, `distance_mm`）作为派生观测量；它们用于显示、诊断或形成更工程化的离散信号（如 `zone_code`），不要与主坐标并列成为闭环判定依据。
- DSL 里优先用主坐标做关键互锁（例如危险窗口/到位/超程），避免多坐标同时参与“真值判断”导致矛盾约束、验证困难与线上歧义。

### 7.2 把换算留在驱动层

驱动层维护统一换算链路（示例）：

- `theta_deg = count / (ppr * 4 * gear_ratio) * 360`
- `distance_mm = theta_deg / 360 * lead_mm_per_rev`

若机构是非线性连杆，建议用 LUT/分段拟合在驱动层计算 `distance_mm`，DSL 仅消费结果。

### 7.3 DSL 层只暴露“可验证信号”

推荐暴露为：

- `analog_input`：`axis_count`, `axis_theta`, `axis_pos_mm`, `axis_speed`
- `digital_input`：`inpos`, `alarm`, `range_valid`, `pos_consistent`
- `analog_input`：`zone_code`（例如 `0=safe`，`1..N=collision window`）

其中：

- `range_valid`（数据新鲜度/有效性）很重要，可避免“旧值参与互锁”。
- `pos_consistent` 是“多传感器一致性”在驱动层下沉后的结果信号（见 7.4）；DSL 层消费 bool/枚举即可，避免引入差值/滤波等复杂算术。

### 7.4 多坐标/多传感器如何“比较”

当你同时有：

- 编码器推算的 `pos_mm`（由 `count` 换算）
- 外部测距传感器的 `laser_mm`

RustPLC DSL 层不适合写复杂算术（差值、绝对值、滤波、置信度），建议在驱动层先完成对比与诊断，再回灌为可验证信号：

- `pos_consistent`（bool）：`abs(pos_mm - laser_mm) <= tol` 且持续 N 个周期成立
- `sensor_fault_code`（analog/enum 编码）：0=正常，1=激光丢帧，2=误差超限，...

然后在 DSL 中做互锁/降级策略（例如不一致则禁止进入危险动作或转 fault）。

### 7.5 拓扑上拆成两条链

- 命令链：`控制输出 -> 轴执行机构 -> 机械部件运动`
- 观测链：`机械部件运动 -> 编码器/距离传感 -> count/theta/distance`

这样做的好处是：顺控和几何/计量关系解耦，后续换编码器或换传感器时只改观测链映射。

### 7.6 何时进 fault / 何时降级（degrade）

建议把“数值换算 + 健康诊断 + 降级策略判定”尽量下沉到驱动层，DSL 只消费结果信号（`range_valid` / `pos_consistent` / `alarm` / `zone_code` 等），并用顺控把行为写清楚。

常见经验策略：

- 直接进 `fault`（强制停机/需要人工干预）的情况：
  - `alarm` 为真（驱动/伺服报警、急停链路断开等）。
  - `range_valid` 为假持续超过一个短窗口（例如 >N 个周期），意味着反馈链路不可信或冻结。
  - `pos_consistent` 为假且当前/目标动作涉及危险区（例如 `zone_code > 0` 时仍尝试进入/继续危险运动）。
- 允许降级（限制功能、只允许“撤离/回零/低速安全移动”）的情况：
  - `pos_consistent` 为假但 `zone_code == 0` 且系统处于安全姿态：可以禁止危险动作（夹紧/高速进入窗口），只允许安全方向撤离，并持续监控一致性恢复。
  - `range_valid` 短暂抖动：可以先进入降级态（冻结窗口编码、禁止进入危险动作），若在超时内恢复则回到 normal，否则转 fault。

落地时，推荐让驱动层输出一个离散的健康/模式编码（例如 `sensor_health_code` 或 `safety_mode`），DSL 以它为入口做“进入 fault / 进入降级 / 解除降级”的顺控与互锁，避免把策略分散在多处阈值判断中。

## 8. 验证友好写法（重要）

1. 把复杂条件拆成多个 step，便于定位故障。
2. 每个 wait 都给 timeout + fault 跳转。
3. 安全约束优先写离散状态（`alarm/off`, `inpos/on`）和区间编码（`zone_code`），模拟量主要用于阈值门槛。
4. 对 AB 编码器相关风险，重点覆盖：
   - 计数不增长（丢脉冲/断线）
   - 方向反了（DIR 与计数符号不一致）
   - 到位但速度未归零（机械抖动）
5. 对碰撞风险，至少覆盖两类故障：
   - “误入禁区但执行器未被拦截”
   - “区间边界抖动导致频繁进出禁区”

## 9. SIL 与回归测试建议

场景初始化/校验/仿真/批量回归与最小化失败的标准流程，见 [Scenario Playbook](scenario_playbook.md)。

当前 sim plant 对电机/编码器动态支持仍较基础，建议：

- 短期：在 scenario 里脚本化 `AI0/AI1/X0/X1` 输入序列，验证顺控与故障恢复。
- 中期：扩展 plant，补齐 `stepper + encoder` 动态模型（响应延迟、失步、噪声）。

建议最少准备 4 组场景：

1. 正常到位（count 达标 + inpos=true）
2. 计数卡住（count 不变，触发 timeout）
3. 方向错误（count 反向，触发安全或超时）
4. ALARM 触发（立即转 fault）

## 10. 常见误区 -> 修正方式

- 误区 1：在 DSL 里直接“逐脉冲”控制步进。
  - 正解：DSL 控流程，驱动层控脉冲。
- 误区 2：把 AB A/B 原始电平直接暴露给 DSL。
  - 正解：先在驱动层解码成 `count/speed/dir`，DSL 只看结果。
- 误区 3：把复杂运动学公式塞进 `wait`。
  - 正解：计算下沉到驱动层，DSL 只做阈值和状态机决策。
- 误区 4：直接用“角度上下限”拼接复杂互锁，而不抽象碰撞窗口信号。
  - 正解：先形成 `zone_code`/`collision_window` 这类工程信号，再写 safety 规则。
- 误区 5：只写“窗口互锁”，不写“命令互锁”（或缺少 `requires` 等价约束）。
  - 正解：采用 6.4 的双向互锁组合，避免“危险姿态仍可继续下发运动命令”的单向漏洞。

---

如果你下一步要落地到具体硬件（如 RP2040 PIO、STM32 TIM Encoder、LinuxCNC/伺服驱动器），建议再补一份“板级适配说明”：

- 脉冲生成外设选择
- 编码器计数读取路径
- 采样周期与 PLC tick 对齐策略
- `io_map` 与量纲映射约定
