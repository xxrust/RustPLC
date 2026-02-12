---
name: plc-gen
description: "Generate RustPLC DSL code from natural language descriptions of industrial control scenarios. Use when a user describes a PLC control scenario in plain language and wants it converted to a .plc file. Triggers on: generate plc, create plc from, convert to plc, write plc for, 生成plc, 写plc, 转成plc, 帮我写个plc程序."
---

# RustPLC DSL Generator

从工程师的自然语言工艺描述，经过多轮对话确认，生成可通过 RustPLC 编译器四大验证引擎的 `.plc` 文件。

---

## 核心理念

工程师不会告诉你"我需要3个气缸、2个电磁阀"。他会说：

> "推料缸把工件推到位，传感器检测到后，压紧缸下压，压紧到位后冲压缸动作，完成后全部缩回。"

你的工作是从这段工艺描述中：
1. **提取动作流程** — 谁先动、谁后动、什么条件触发下一步
2. **推理出完整设备拓扑** — 每个动作背后需要哪些物理设备
3. **推导安全约束** — 哪些动作存在物理干涉
4. **生成合法 DSL** — 通过编译器验证

整个过程需要与工程师多轮确认，不要一次性生成最终结果。

---

## 工作流程（严格按阶段执行）

### 阶段一：理解工艺

收到工程师的描述后，先用自己的话复述工艺流程，整理成一个清晰的动作时序表：

```
动作序列：
1. [推料缸] 伸出 → 等待 [推料到位传感器]
2. [压紧缸] 下压 → 等待 [压紧到位传感器]
3. [冲压缸] 伸出 → 等待 [冲压到位传感器]
4. [冲压缸] 缩回 → 等待 [冲压缩回传感器]
5. [压紧缸] 缩回 → 等待 [压紧缩回传感器]
6. [推料缸] 缩回 → 等待 [推料缩回传感器]

触发方式：按钮启动
循环模式：单次循环，完成后等待再次启动
```

然后向工程师确认：
- "我理解的动作顺序对吗？"
- "有没有哪些动作是可以同时进行的？"（比如冲压缸缩回的同时压紧缸可以缩回？）
- "有没有我遗漏的动作或状态？"

**等待工程师确认后再进入下一阶段。**

### 阶段二：推理设备拓扑

根据确认后的动作序列，推理出完整的设备清单。展示给工程师：

```
推理出的设备拓扑：

执行机构：
  - 推料缸 (cyl_push)    ← 电磁阀 (valve_push)    ← 输出口 Y0
  - 压紧缸 (cyl_clamp)   ← 电磁阀 (valve_clamp)   ← 输出口 Y1
  - 冲压缸 (cyl_press)   ← 电磁阀 (valve_press)   ← 输出口 Y2

传感器：
  - 推料到位 (sensor_push_ext)   → 输入口 X0，检测 cyl_push.extended
  - 推料缩回 (sensor_push_ret)   → 输入口 X1，检测 cyl_push.retracted
  - 压紧到位 (sensor_clamp_ext)  → 输入口 X2，检测 cyl_clamp.extended
  - 压紧缩回 (sensor_clamp_ret)  → 输入口 X3，检测 cyl_clamp.retracted
  - 冲压到位 (sensor_press_ext)  → 输入口 X4，检测 cyl_press.extended
  - 冲压缩回 (sensor_press_ret)  → 输入口 X5，检测 cyl_press.retracted

人机交互：
  - 启动按钮 (start_button)      → 输入口 X6

默认时序参数：
  - 电磁阀响应: 20ms
  - 气缸行程: 300ms（伸出/缩回）
```

向工程师确认：
- "设备清单完整吗？有没有遗漏的传感器或执行机构？"
- "命名可以吗？你有偏好的命名方式吗？"
- "时序参数需要调整吗？比如某个缸行程特别长？"
- "I/O 口分配需要和实际接线一致吗？如果是，请告诉我实际分配。"

**等待工程师确认后再进入下一阶段。**

### 阶段三：推导约束

根据设备拓扑和动作序列，推导安全约束。展示给工程师：

```
推导出的约束：

安全约束（物理干涉）：
  - 冲压缸伸出时，压紧缸必须保持压紧状态
    → safety: cyl_press.extended requires cyl_clamp.extended
  - （如果推料缸和冲压缸在同一轴线）推料缸伸出与冲压缸伸出冲突
    → safety: cyl_push.extended conflicts_with cyl_press.extended

因果链（每个被 wait 引用的传感器都需要）：
  - Y0 -> valve_push -> cyl_push -> sensor_push_ext
  - Y0 -> valve_push -> cyl_push -> sensor_push_ret   ← 缩回传感器也需要
  - Y1 -> valve_clamp -> cyl_clamp -> sensor_clamp_ext
  - Y1 -> valve_clamp -> cyl_clamp -> sensor_clamp_ret ← 缩回传感器也需要
  - Y2 -> valve_press -> cyl_press -> sensor_press_ext
  - Y2 -> valve_press -> cyl_press -> sensor_press_ret ← 缩回传感器也需要

⚠️ 需要你确认的问题：
  - 推料缸和冲压缸在物理上会干涉吗？
  - 还有其他不能同时动作的组合吗？
  - 整个循环有周期时间要求吗？（比如必须在10秒内完成）
```

**等待工程师确认后再进入下一阶段。**

**重要：阶段三的约束转化检查清单**

进入阶段四之前，逐条核对：
- 工程师在阶段三确认的每一条安全关系，是否都已转化为 `safety:` 约束？
- 所有在 `wait` 语句中引用的传感器，是否都有对应的因果链？（包括 `_ret` 缩回传感器）
- 因果链不仅要覆盖 `_ext` 伸出传感器，也要覆盖 `_ret` 缩回传感器

**`conflicts_with` vs `requires` 判断指引：**

工程师说"物理干涉"时，需要进一步追问来区分两种情况：

| 工程师的表述 | 真实含义 | 正确约束 |
|-------------|---------|---------|
| "A和B不能同时伸出" "A伸出时B绝对不能动" | 两个状态在任何时刻不能共存 | `conflicts_with` |
| "B动作前A必须先到位" "B工作时A必须保持" | B的状态依赖A的状态 | `requires` |
| "A和B有干涉"（模糊） | **需要追问**：是"不能同时存在"还是"有先后顺序要求"？ | 追问后决定 |

常见陷阱：工程师说"推缸和压装缸有干涉"，但同时又说"压装时推缸必须保持伸出"。这说明干涉是运动过程中的（通过顺序控制保证），不是最终状态的互斥。此时应该用 `requires`（压装需要推缸到位），而不是 `conflicts_with`。

### 阶段四：生成 DSL 并验证

将以上所有确认过的信息组装成 `.plc` 文件，保存到 `examples/` 目录，然后运行编译器验证：

```bash
cargo run --release -- examples/<filename>.plc
```

- 如果验证通过，展示结果给工程师
- 如果验证失败，阅读错误信息，修复后重新验证，直到全部通过
- 将最终通过验证的文件展示给工程师做最后确认

**DSL 延时（delay）与超时（timeout）：**

DSL 支持固定延时：
- `delay: 2000ms` —— 编译为内部有界等待，Liveness 不会报死锁，Timing 会计入关键路径。

常见组合：
- `timeout: 500ms -> goto fault_handler` —— 保护性上界逃生分支
- 同一 step 中 `delay` + `timeout` 可以共存：timeout 作为保护性上界（例如 delay 300ms + timeout 1200ms）。

**DSL repeat N 原语：**

DSL 支持 `repeat N:`，用于将一段 step 语句在语义阶段展开为 N 个顺序 step（`N` 必须在 `2..=100`）。限制：
- 不允许嵌套 repeat
- 不允许在 parallel/race 分支内部使用 repeat（需要时改为手动展开）

示例：
```plc
step glue_cycle:
    repeat 3:
        action: extend cyl_glue
        wait: sensor_glue_ext == true
        timeout: 300ms -> goto fault_handler
        action: retract cyl_glue
        wait: sensor_glue_ret == true
        timeout: 300ms -> goto fault_handler
```

---

## DSL 语法参考

### 文件结构

```plc
[topology]          # 物理设备与连接
[constraints]       # 安全、时序、因果约束
[tasks]             # 控制逻辑（状态机）
```

### 设备类型与可用状态

| 类型 | 用途 | 关键属性 | 可用状态（用于 safety/wait） |
|------|------|----------|------------------------------|
| `digital_output` | 输出端口 (Y0, Y1...) | — | `on`, `off` |
| `digital_input` | 输入端口 (X0, X1...) | `debounce` | `on`, `off` |
| `solenoid_valve` | 电磁阀 | `connected_to`, `response_time`, `states`(可选) | 默认 `on`, `off`（可用 `states: [...]` 自定义） |
| `cylinder` | 气缸 | `connected_to`, `stroke_time`, `retract_time`, `states`(可选) | 默认 `extended`, `retracted`（可用 `states: [...]` 自定义） |
| `motor` | 电机 | `connected_to`, `rated_speed`, `ramp_time` | `on`, `off` |
| `sensor` | 传感器 | `connected_to`, `type`, `detects`, `states`(可选) | 默认 `on`, `off` + `detects` 声明的状态（可用 `states: [...]` 自定义） |

状态在 `safety:` 约束中使用时格式为 `device.state`，例如：
- `cyl_A.extended` — 气缸伸出状态
- `motor_conveyor.on` — 电机运转状态
- 不同类型设备的状态可以混合使用在 `conflicts_with` / `requires` 中

当需要建模超过 2 个状态的设备（如三位电磁阀），在设备属性中声明自定义状态集：
```plc
device valve_3pos: solenoid_valve {
    states: [extend, neutral, retract]
}
```
声明后，该设备在 `safety:` / `detects:` 等需要 `device.state` 的位置将使用自定义状态集合（states 数量 > 8 会输出 warning，非错误）。

### 连接链规则

设备通过 `connected_to` 形成上游链：
```
digital_output ← solenoid_valve ← cylinder
                                        ↘ sensor (通过 detects 关联)
digital_output ← motor
                    ↘ sensor (通过 detects 关联)
```

### 约束语法

```plc
# 互斥：两个状态不能同时为真
safety: cyl_A.extended conflicts_with cyl_B.extended

# 依赖：状态A为真时，状态B必须也为真
safety: cyl_press.extended requires cyl_clamp.extended

# 时序：任务/步骤必须在指定时间内完成
timing: task.cycle must_complete_within 8000ms
timing: task.cycle must_complete_within_worst_case 12000ms

# 因果链：信号传播路径必须在拓扑中连通
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
```

### 任务语法

```plc
task <名称>:
    step <步骤名>:
        action: extend <气缸>          # 伸出
        action: retract <气缸>         # 缩回
        action: set <设备> on/off      # 开关
        action: log "<消息>"           # 日志
        delay: 2000ms                  # 固定延时（有界等待）
        wait: <传感器> == true          # 等待条件
        wait: A == true AND B == true  # AND 条件（不可与 OR 混用）
        wait: A == true OR B == true   # OR 条件（不可与 AND 混用）
        timeout: 500ms -> goto <任务>   # 超时跳转（goto 目标同下）
        goto <任务>                     # 跳转到 task 首步
        goto <任务>.<step>              # 跳转到 task 内指定 step
        if: A == true goto T1 else: goto T2  # 条件分支（两条互补守卫）
        repeat 3:                      # 重复块（2..=100，不允许嵌套/不允许在 parallel/race 内）
            action: log "tick"
        allow_indefinite_wait: true     # 允许无限等待（仅用于人工操作）
    on_complete: goto <任务>            # 完成后跳转（也支持 task.step）

# 并行（所有分支同时执行，全部完成后继续）
step do_both:
    parallel:
        branch_A:
            action: extend cyl_A
        branch_B:
            action: extend cyl_B
    wait: sensor_A_ext == true
    timeout: 600ms -> goto fault

# 竞争（所有分支同时执行，第一个满足条件的胜出）
step detect:
    race:
        branch_A:
            wait: sensor_A == true
            then: goto process_A
        branch_B:
            wait: sensor_B == true
            then: goto process_B
    timeout: 800ms -> goto fault
```

### 新语法完整片段（示例）

```plc
[topology]
device mode_switch: digital_input

[constraints]

[tasks]
task choose:
    step decide:
        if: mode_switch == true goto process_A.run else: goto process_B.run

task process_A:
    step run:
        action: log "A"
    on_complete: goto done

task process_B:
    step run:
        action: log "B"
    on_complete: goto done

task done:
    step finish:
        action: log "done"
```

```plc
[topology]
device valve_3pos: solenoid_valve {
    states: [extend, neutral, retract]
}
device sensor_a: sensor
device sensor_b: sensor

[constraints]
safety: valve_3pos.extend conflicts_with valve_3pos.retract
timing: task.cycle must_complete_within 3000ms
timing: task.cycle must_complete_within_worst_case 6000ms

[tasks]
task cycle:
    step work:
        repeat 3:
            action: log "tick"
            delay: 200ms
            wait: sensor_a == true AND sensor_b == true
            timeout: 1500ms -> goto fault
    on_complete: goto idle

task idle:
    step ready:
        wait: valve_3pos == neutral OR sensor_a == true
        allow_indefinite_wait: true

task fault:
    step stop:
        action: log "fault"
```

---

## 验证规则速查

### Safety（安全性）
- 声明了 `conflicts_with` 的两个状态不能在任何可达路径中同时为真
- `parallel` 块中不能同时触发冲突状态
- 顺序执行天然安全（前一个缩回后下一个才伸出）

### Liveness（活性）
- 每个 `wait` 必须有 `timeout`，除非标记了 `allow_indefinite_wait: true`
- `allow_indefinite_wait` 仅用于人工触发的等待（如启动按钮）
- 每个 task 必须有 `on_complete`
- 不能有孤立的死胡同状态

### Timing（时序）
- `must_complete_within`：基于动作/延时的关键路径估计（忽略 timeout 上界），更贴近“设备实际动作时间 + 固定 delay”。并行动作取最大值。
- `must_complete_within_worst_case`：将 timeout 作为最坏上界纳入估计（保守），适合把容错超时也算进周期 SLA 的场景。并行动作取最大值。
- 经验：如果你给每个 step 都加了较大的 timeout（容错），但仍希望约束按真实节拍衡量，用 `must_complete_within`；如果你希望“连超时都算进去仍要满足”，用 `must_complete_within_worst_case`。
- `delay:` 会计入两种估计中的关键路径。

### Causality（因果性）
- 因果链中相邻设备必须通过 `connected_to` 或 `detects` 在拓扑中连通
- 链的方向：输出口 → 阀/电机 → 执行机构 → 传感器

**parallel 块的因果链陷阱：**

编译器会把 parallel 块中**所有分支**的 action 与 step 级别的 wait 做因果检查。如果两个分支操作不同设备，但 step 的 wait 只关联其中一个设备的传感器，另一个分支就会报因果链断裂。

例如以下代码会报错：
```plc
step start_both:
    parallel:
        branch_left:
            action: set motor_left on    # 操作 motor_left
        branch_right:
            action: set motor_right on   # 操作 motor_right
    wait: sensor_left_arrive == true     # 只等 motor_left 的传感器
    # → 编译器会检查 motor_right -> sensor_left_arrive，因果链断裂！
```

**解决方案：** 当多个不同设备需要"同时"启动但各自等待不同传感器时，不要用 parallel，改用顺序 step 分别启动（`set` 动作是瞬时的，顺序启动在物理上等同于同时）：
```plc
step start_left:
    action: set motor_left on
step start_right:
    action: set motor_right on
step wait_left:
    wait: sensor_left_arrive == true
    timeout: 5000ms -> goto fault_handler
step wait_right:
    wait: sensor_right_arrive == true
    timeout: 5000ms -> goto fault_handler
```

parallel 块适合的场景：同类设备同时动作，且 step 的 wait 不需要区分来源（如两个气缸同时伸出，等待任一到位传感器）。

---

## 默认时序参数

当工程师未指定具体参数时，使用以下默认值：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| 电磁阀 response_time | 20ms | 阀芯响应时间 |
| 气缸 stroke_time | 300ms | 伸出行程时间 |
| 气缸 retract_time | 300ms | 缩回行程时间 |
| 电机 ramp_time | 50ms | 启动爬坡时间 |
| 按钮 debounce | 20ms | 消抖时间 |
| 超时倍率 | 1.5x | timeout = stroke_time × 1.5 + response_time |

---

## 命名约定

| 对象 | 格式 | 示例 |
|------|------|------|
| 气缸 | `cyl_<功能>` | `cyl_push`, `cyl_clamp`, `cyl_press` |
| 电磁阀 | `valve_<对应气缸功能>` | `valve_push`, `valve_clamp` |
| 传感器 | `sensor_<气缸>_<状态>` | `sensor_push_ext`, `sensor_push_ret` |
| 电机 | `motor_<功能>` | `motor_conveyor`, `motor_spindle` |
| 输出口 | `Y0`, `Y1`, ... | 按声明顺序分配 |
| 输入口 | `X0`, `X1`, ... | 按声明顺序分配 |
| 任务 | 动作导向 | `cycle`, `init`, `fault_handler`, `ready` |
| 步骤 | 动词_名词 | `extend_push`, `wait_clamp`, `retract_all` |

工程师可以在阶段二要求使用自己的命名习惯，以上仅为默认值。

---

## 必须包含的标准结构

每个生成的 `.plc` 文件都必须包含：

1. **fault_handler 任务** — 缩回所有气缸 / 关闭所有电机，输出报警日志。注意：fault_handler 中的 retract/set off 动作虽然不强制要求 wait 确认（因为这是紧急恢复），但应在 log 中明确提示操作员需要人工确认设备状态。
2. **ready 任务** — 等待启动按钮，标记 `allow_indefinite_wait: true`
3. **所有 wait 都有 timeout** — 指向 fault_handler（人工等待除外）
4. **所有被 wait 引用的传感器的因果链** — 不仅是 `_ext` 伸出传感器，也包括 `_ret` 缩回传感器。规则：如果某个传感器出现在 `wait:` 语句中，就必须有一条从输出口到该传感器的因果链。
5. **工程师确认的所有安全约束** — 阶段三中工程师确认的每一条安全关系都必须转化为 `safety:` 约束，不能遗漏

---

## 完整示例

**工程师输入：**
> "电机转动，转到A位置或B位置停下来，分别做不同的处理。有启动按钮。"

**阶段一回复：**

我理解的工艺流程：
1. 按下启动按钮
2. 电机开始旋转
3. 竞争检测：先到A位置传感器 → 执行A工艺；先到B位置传感器 → 执行B工艺
4. 处理完成后停机，等待下次启动

需要确认：
- 电机旋转超时（比如转了一圈还没检测到）怎么处理？我建议加一个超时故障处理。
- A工艺和B工艺具体做什么？还是目前只需要记录日志占位？

**（工程师确认后，经过阶段二、三，最终生成：）**

```plc
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input

device start_button: digital_input {
    connected_to: X2
    debounce: 20ms
}

device motor_ctrl: motor {
    connected_to: Y0
    rated_speed: 60rpm
    ramp_time: 50ms
}

device sensor_A: sensor {
    type: proximity
    connected_to: X0
    detects: motor_ctrl.position_A
}

device sensor_B: sensor {
    type: proximity
    connected_to: X1
    detects: motor_ctrl.position_B
}

[constraints]

timing: task.search.detect must_complete_within 800ms
    reason: "旋转加启动不应超过800ms"

causality: Y0 -> motor_ctrl -> sensor_A
causality: Y0 -> motor_ctrl -> sensor_B

[tasks]

task search:
    step start_motor:
        action: set motor_ctrl on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 800ms -> goto motor_fault

task process_A:
    step stop_motor:
        action: set motor_ctrl off
    step do_work_A:
        action: log "工件在A位置，执行A工艺"
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl off
    step do_work_B:
        action: log "工件在B位置，执行B工艺"
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl off
    step alarm:
        action: log "电机旋转超时"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
```

---

## 更多参考示例

`examples/` 目录下有多个已通过验证的 `.plc` 文件，生成前务必参考：

| 文件 | 场景 | 涉及模式 |
|------|------|----------|
| `two_cylinder.plc` | 双气缸顺序动作 | 基础顺序、conflicts_with |
| `half_rotation.plc` | 电机+竞争检测 | motor、race、多 task 跳转 |
| `drill_station.plc` | 传送带+夹紧+钻孔 | motor+cylinder 混合、requires、传感器 false 等待 |
| `label_station.plc` | 传送带+升降+贴标 | conflicts_with(cylinder vs motor)、完整因果链（含 _ret） |
| `assembly_station.plc` | 双传送带+推缸+压装+出料 | 多设备顺序启动替代 parallel、requires vs conflicts_with 区分、timing 约束 |
| `grind_station.plc` | 选择开关+打磨+延时 | race 做模式选择、timeout 模拟延时、对立条件等待、多 task 共享收尾 |
| `stamp_bend_line.plc` | 冲压+折弯串联产线 | 多工位 task 链、共用传送带多位置传感器、大量 conflicts_with + requires |
| `glue_station.plc` | 涂胶+循环展开 | 循环动作展开为顺序步骤、同一气缸多次伸缩 |

遇到类似场景时，先读取对应示例文件了解已验证的模式，再生成新文件。
