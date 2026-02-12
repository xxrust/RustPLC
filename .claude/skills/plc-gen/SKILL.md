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

因果链：
  - Y0 -> valve_push -> cyl_push -> sensor_push_ext
  - Y1 -> valve_clamp -> cyl_clamp -> sensor_clamp_ext
  - Y2 -> valve_press -> cyl_press -> sensor_press_ext

⚠️ 需要你确认的问题：
  - 推料缸和冲压缸在物理上会干涉吗？
  - 还有其他不能同时动作的组合吗？
  - 整个循环有周期时间要求吗？（比如必须在10秒内完成）
```

**等待工程师确认后再进入下一阶段。**

### 阶段四：生成 DSL 并验证

将以上所有确认过的信息组装成 `.plc` 文件，保存到 `examples/` 目录，然后运行编译器验证：

```bash
cargo run --release -- examples/<filename>.plc
```

- 如果验证通过，展示结果给工程师
- 如果验证失败，阅读错误信息，修复后重新验证，直到全部通过
- 将最终通过验证的文件展示给工程师做最后确认

---

## DSL 语法参考

### 文件结构

```plc
[topology]          # 物理设备与连接
[constraints]       # 安全、时序、因果约束
[tasks]             # 控制逻辑（状态机）
```

### 设备类型

| 类型 | 用途 | 关键属性 |
|------|------|----------|
| `digital_output` | 输出端口 (Y0, Y1...) | — |
| `digital_input` | 输入端口 (X0, X1...) | `debounce` |
| `solenoid_valve` | 电磁阀 | `connected_to`, `response_time` |
| `cylinder` | 气缸 | `connected_to`, `stroke_time`, `retract_time` |
| `motor` | 电机 | `connected_to`, `rated_speed`, `ramp_time` |
| `sensor` | 传感器 | `connected_to`, `type`, `detects` |

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
        wait: <传感器> == true          # 等待条件
        timeout: 500ms -> goto <任务>   # 超时跳转
        allow_indefinite_wait: true     # 允许无限等待（仅用于人工操作）
    on_complete: goto <任务>            # 完成后跳转

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
- `must_complete_within` 检查最坏关键路径
- 关键路径 = 顺序动作时间之和 + 上游 response_time 链
- 并行动作取最大值
- 约束值必须大于计算出的最坏路径

### Causality（因果性）
- 因果链中相邻设备必须通过 `connected_to` 或 `detects` 在拓扑中连通
- 链的方向：输出口 → 阀/电机 → 执行机构 → 传感器

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

1. **fault_handler 任务** — 缩回所有气缸 / 关闭所有电机，输出报警日志
2. **ready 任务** — 等待启动按钮，标记 `allow_indefinite_wait: true`
3. **所有 wait 都有 timeout** — 指向 fault_handler（人工等待除外）
4. **所有执行机构的因果链** — 从输出口到传感器

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
