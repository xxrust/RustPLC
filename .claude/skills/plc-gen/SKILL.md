---
name: plc-gen
description: "Generate RustPLC DSL code from natural language descriptions of industrial control scenarios. Use when a user describes a PLC control scenario in plain language and wants it converted to a .plc file. Triggers on: generate plc, create plc from, convert to plc, write plc for, 生成plc, 写plc, 转成plc, 帮我写个plc程序."
---

# RustPLC DSL Generator

从工程师的自然语言工艺描述，经过多轮对话确认，先生成项目顶层语义描述（`.system.md`），再生成可通过 RustPLC 编译器四大验证引擎的 `.plc` 文件。

## 维护说明（自测）

本 skill 不仅是"生成 DSL"，还要对 **生成物可验证性** 负责。仓库内提供了该 skill 的自测夹具：

- 夹具目录：`.codex/skills/plc-gen/fixtures/valid/*.plc`
- 运行自测：`cargo test --test plc_gen_skill_fixtures`
- 全量回归：`cargo test -q`

当你修改本 `SKILL.md` 的流程/规则时，应同步新增或更新一个 fixture，用来覆盖该规则的典型场景，避免"写了规则但从未验证"。

---

## 核心理念

工程师不会告诉你"我需要3个气缸、2个电磁阀"。他会说：

> "推料缸把工件推到位，传感器检测到后，压紧缸下压，压紧到位后冲压缸动作，完成后全部缩回。"

你的工作是从这段工艺描述中：
1. **建立语义空间** — 先搞清楚这是什么项目、部署在哪、安全等级多高，生成 `.system.md`
2. **提取动作流程** — 谁先动、谁后动、什么条件触发下一步
3. **推理出完整设备拓扑** — 每个动作背后需要哪些物理设备
4. **推导安全约束** — 哪些动作存在物理干涉
5. **生成合法 DSL** — 通过编译器验证

整个过程需要与工程师多轮确认，不要一次性生成最终结果。

---

## 工作流程（严格按阶段执行）

### 阶段零：生成系统描述（.system.md）

**这是整个流程的第一步，必须在写任何 .plc 代码之前完成。**

收到工程师的描述后，首先生成一份 `.system.md` 文件。这份文件是项目的顶层语义描述，定义了"这是什么系统、为谁服务、安全等级多高、核心工艺意图是什么"。它不包含任何 DSL 语法，纯自然语言，工程师和 AI 都能读写。

**`.system.md` 的作用：**
- 为后续所有阶段提供语义锚点 — AI 的每一个决策（冗余策略、超时值、约束严格程度）都从这里推导
- 同一套"两个阀门开关"的工艺，写在核电站 system.md 里就会产生双冗余传感器和 10 秒硬限，写在教学台 system.md 里就只需要简单超时保护

**`.system.md` 必须包含以下段落：**

```markdown
## 项目身份
- 项目名称、所属行业、部署场所、最终用户、监管要求（如有）

## 系统使命
用 2~3 句话说清楚这套系统要干什么，以及它的失效会导致什么后果。

## 安全与可靠性定位
- 安全等级（SIL 等级 / 行业标准 / 常规工业防护）
- 故障后果（人身伤害 / 财产损失 / 演示失败）
- 容错策略（冗余传感器 / 双通道互锁 / 单一超时保护）

## 运行环境
- 介质、气源、电源、控制器、通信、环境条件

## 核心工艺意图
用自然语言描述完整的工艺流程，包括正常流程、异常处理、特殊工况。

## 关键约束
用自然语言列出安全规则、时序要求、互锁关系。

## 设计偏好
- 命名语言、代码风格、时序参数策略、扩展预期

## 对 AI 的指引
基于以上背景，明确告诉 AI 在本项目中应该遵循的决策倾向。
```

**文件命名：** 与 `.plc` 文件同名，后缀为 `.system.md`，放在同一目录。例如 `examples/two_cylinder.system.md` 对应 `examples/two_cylinder.plc`。

**参考样板：**
- `examples/two_cylinder.system.md` — 教学演示台（低安全等级）
- `examples/nuclear_coolant_isolation.system.md` — 核电站隔离阀（SIL3 高安全等级）

生成后展示给工程师确认：
- "项目定位和安全等级对吗？"
- "系统使命的描述准确吗？"
- "有没有遗漏的工况或约束？"

**等待工程师确认 `.system.md` 后再进入阶段一。**

### 阶段一：理解工艺

基于已确认的 `.system.md`，深入理解具体工艺流程。用自己的话复述工艺流程，整理成一个清晰的动作时序表：

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

**阶段一最小提问清单（缺一不可，不能凭空假设）：**
- 启动方式：按钮/信号？是否需要复位/急停？
- 循环模式：单次/自动循环？循环结束停在哪里（全部缩回/保持夹紧/保持到位）？
- 初始状态：上电后各执行器默认位置（尤其是气缸是否要求"全部回原位"）？
- 人工介入点：是否存在人工装料/取料/确认按钮？对应等待是否允许无限等待？
- 同步关系：是否允许并行动作？哪些动作必须互锁（互斥/依赖）？

**等待工程师确认后再进入下一阶段。**

### 模拟量与数值比较指引

当工艺描述涉及"压力/温度/位置/流量"等数值比较时：
- 使用 `analog_input` 建模，并**必须声明 `range` 与 `unit`**。
- 数值比较直接写在 `wait` 或 `safety` 中，例如：`wait: pressure >= 60`。
- **避免 `==`**，用区间或容差带代替（例如 `>= 58 AND <= 62`）。
- 传感器（`sensor`）只用于离散反馈信号（on/off），不承担数值语义。
- 若输入来自操作员设定值/上位系统，标记为 `external: true`（避免因果误报）。

### PID 控制指引

当工艺需要闭环控制（温度/压力/流量恒定）时：
- 使用 `pid` 设备类型，声明 `pv`（过程变量）、`sp`（设定值）、`kp/ki/kd`、`out`（输出）、`period_ms`、`limit`。
- PID 设备自动关联 `analog_input`（pv）和 `analog_output`（out）。
- 参考 fixture：`13_analog_pid_loop.plc`。

### 阶段二：推理设备拓扑

根据确认后的动作序列，推理出完整的设备清单。展示给工程师：

```
推理出的设备拓扑：

PLC 控制器：
  - plc_main: plc { ports: [Y0, Y1, Y2, X0..X6] }

执行机构：
  - 推料缸 (cyl_push)    ← 电磁阀 (valve_push)    ← plc_main.Y0
  - 压紧缸 (cyl_clamp)   ← 电磁阀 (valve_clamp)   ← plc_main.Y1
  - 冲压缸 (cyl_press)   ← 电磁阀 (valve_press)   ← plc_main.Y2

传感器：
  - 推料到位 (sensor_push_ext)   → plc_main.X0，检测 cyl_push.extended
  - 推料缩回 (sensor_push_ret)   → plc_main.X1，检测 cyl_push.retracted
  - ...（其余类推）

人机交互：
  - 启动按钮 (start_button)      → plc_main.X6

默认时序参数：
  - 电磁阀响应: 20ms
  - 气缸行程: 300ms（伸出/缩回）
```

向工程师确认：
- "设备清单完整吗？有没有遗漏的传感器或执行机构？"
- "命名可以吗？你有偏好的命名方式吗？"
- "时序参数需要调整吗？比如某个缸行程特别长？"
- "I/O 口分配需要和实际接线一致吗？如果是，请告诉我实际分配。"

**I/O 未知时的约定（避免编造）：**
- 如果工程师没有给出真实 I/O，对外展示时明确标注"占位分配"，例如 `Y?`/`X?` 或按顺序 `Y0..` `X0..`。
- fixture/示例代码中允许使用按顺序分配的 I/O（用于验证 DSL 语义），但在工程落地时必须替换为真实接线表。

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

因果链（可选，仅用于文档可读性；编译器会从拓扑 relation 自动推断）：
  - Y0 -> valve_push -> cyl_push -> sensor_push_ext
  - ...（其余类推）

⚠️ 需要你确认的问题：
  - 推料缸和冲压缸在物理上会干涉吗？
  - 还有其他不能同时动作的组合吗？
  - 整个循环有周期时间要求吗？（比如必须在10秒内完成）
```

**等待工程师确认后再进入下一阶段。**

**重要：阶段三的约束转化检查清单**

进入阶段四之前，逐条核对：
- 工程师确认的每一条安全关系，是否都已转化为 `safety:` 约束？
- 拓扑中 `relation` 和 `detects` 链是否完整？（编译器会自动从拓扑推断因果可达性，无需显式写 `causality:` 约束。显式声明仅用于文档可读性。）

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

**可选（推荐）：SIL 仿真做一次"软件在环"冒烟测试**

在不接硬件的情况下，用内置 SIL 运行一次场景，尽早暴露超时/死锁/点位脚本问题（需要你准备一个 scenario YAML）：

```bash
cargo run --release -- sim examples/<filename>.plc --scenario scenarios/<scenario>.yaml --out out/<run>/
```

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
[topology]          # 物理设备、PLC 端口与 relation 连接
[constraints]       # 安全、时序、因果约束
[tasks]             # 控制逻辑（状态机）
```

### 设备类型与可用状态

| 类型 | 用途 | 关键属性 | 可用状态（用于 safety/wait） |
|------|------|----------|------------------------------|
| `plc` | PLC 控制器本体 | `purpose`(必填), `ports` | — |
| `solenoid_valve` | 电磁阀 | `purpose`(必填), `response_time`, `states`(可选) | 默认 `on`, `off`（可用 `states: [...]` 自定义） |
| `cylinder` | 气缸 | `purpose`(必填), `stroke_time`, `retract_time`, `states`(可选) | 默认 `extended`, `retracted`（可用 `states: [...]` 自定义） |
| `motor` | 电机 | `purpose`(必填), `rated_speed`, `ramp_time` | `on`, `off` |
| `sensor` | 离散传感器 | `purpose`(必填), `subtype`(标准做法), `debounce` | `on`, `off` |
| `analog_input` | 模拟量输入 | `purpose`(必填), `range`(必填), `unit`(必填), `external` | 数值比较 |
| `analog_output` | 模拟量输出 | `purpose`(必填), `range`(必填), `unit` | 数值设定 |
| `pid` | PID 控制器 | `purpose`(必填), `pv`, `sp`, `kp/ki/kd`, `out`, `period_ms`, `limit` | — |
| `digital_input` | 独立数字输入 | `purpose`(必填), `debounce` | `on`, `off` |
| `digital_output` | 独立数字输出 | `purpose`(必填), `subtype` | `on`, `off` |

**必填字段规则：**
- `purpose:` — 所有设备必填，描述该设备在工艺中的职责。编译器会做门禁检查。
- `subtype:` — 传感器的标准做法（`push_button`、`e_stop_button`、`limit_switch`、`proximity_sensor`、`selector_switch`）。`e_stop_button` 必须同时声明 `inverted: true`。
- `external: true` — 标记来自外部系统的输入（操作员设定值、上位系统），避免因果性误报。

状态在 `safety:` 约束中使用时格式为 `device.state`，例如：
- `cyl_A.extended` — 气缸伸出状态
- `motor_conveyor.on` — 电机运转状态
- 不同类型设备的状态可以混合使用在 `conflicts_with` / `requires` 中

当需要建模超过 2 个状态的设备（如三位电磁阀），在设备属性中声明自定义状态集：
```plc
device valve_3pos: solenoid_valve {
    purpose: "三位电磁阀，支持 extend/neutral/retract 三种状态",
    states: [extend, neutral, retract]
}
```

### I/O 声明与拓扑模式

**所有 PLC I/O 端口统一在 `plc_main` 设备中声明：**

```plc
device plc_main: plc {
    purpose: "控制器本体与工艺 I/O 端口映射",
    ports: [Y0:digital:producer, Y1:digital:producer, X0:digital:consumer, X1:digital:consumer, AI0:analog:consumer, AO0:analog:producer]
}
```

端口格式：`<名称>:<类型>:<角色>`
- 类型：`digital` | `analog` | `pneumatic` | `logical` | `generic`
- 角色：`producer`（输出）| `consumer`（输入）| `bidirectional`

**设备间连接使用 `relation` 声明：**

```plc
# PLC 输出 → 电磁阀线圈
relation { from: plc_main.Y0, to: valve_push.coil, via: driven_by }

# 电磁阀气路 → 气缸命令
relation { from: valve_push.out, to: cyl_push.cmd, via: driven_by }

# 气缸状态 → 传感器检测
relation { from: cyl_push.extended, to: sensor_push_ext.sense, via: detects }

# 传感器输出 → PLC 输入
relation { from: sensor_push_ext.out, to: plc_main.X0, via: reports_to }

# 按钮 → PLC 输入
relation { from: start_button.out, to: plc_main.X6, via: reports_to }
```

三种关系类型：
- `driven_by` — 驱动链（PLC→阀→缸，PLC→电机）
- `reports_to` — 反馈链（传感器→PLC，按钮→PLC）
- `detects` — 检测链（气缸状态→传感器，电机位置→传感器）

**隐式端口（无需在设备上声明 ports）：**
- `solenoid_valve`：`coil`(consumer) + `out`(producer)
- `cylinder`：`cmd`(consumer) + `extended`(producer) + `retracted`(producer)
- `sensor`：`sense`(consumer) + `out`(producer)
- `motor`：`cmd`(consumer) + `on`(producer)

**模拟量 I/O 不需要放在 plc_main 中，可以独立声明：**

```plc
device AI0: analog_input {
    purpose: "压力传感器反馈",
    range: 0..100,
    unit: "bar",
    external: true
}
```

### 约束语法

```plc
# 互斥：两个状态不能同时为真
safety: cyl_A.extended conflicts_with cyl_B.extended

# 依赖：状态A为真时，状态B必须也为真
safety: cyl_press.extended requires cyl_clamp.extended

# 模拟量安全约束
safety: AI0 > 90 conflicts_with AI0 < 10

# 时序：任务/步骤必须在指定时间内完成
timing: task.cycle must_complete_within 8000ms
timing: task.cycle must_complete_within_worst_case 12000ms

# 因果链：信号传播路径必须在拓扑中连通（可选，编译器自动推断）
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
```

### 任务语法

```plc
task <名称>:
    step <步骤名>:
        action: extend <气缸>          # 伸出
        action: retract <气缸>         # 缩回
        action: set <设备> on/off      # 开关
        action: set_analog <AO> <值>   # 模拟量输出
        action: log "<消息>"           # 日志
        delay: 2000ms                  # 固定延时（有界等待）
        wait: <传感器> == true          # 等待条件
        wait: A == true AND B == true  # AND 条件（不可与 OR 混用）
        wait: A == true OR B == true   # OR 条件（不可与 AND 混用）
        wait: AI0 >= 60               # 模拟量阈值等待
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

---

## 验证规则速查

### Safety（安全性）
- 声明了 `conflicts_with` 的两个状态不能在任何可达路径中同时为真
- `parallel` 块中不能同时触发冲突状态
- 顺序执行天然安全（前一个缩回后下一个才伸出）

### Liveness（活性）
- 每个 `wait` 必须有 `timeout`，除非标记了 `allow_indefinite_wait: true`
- `allow_indefinite_wait` 仅用于人工触发的等待（如启动按钮）
- 每个 task 需要 `on_complete`。若最后一步的所有路径都通过 goto 离开（if/else 两分支都 goto、race 所有分支都有 then: goto 且有 timeout -> goto），可省略。
- 不能有孤立的死胡同状态

### Timing（时序）
- `must_complete_within`：基于动作/延时的关键路径估计（忽略 timeout 上界），更贴近"设备实际动作时间 + 固定 delay"。并行动作取最大值。
- `must_complete_within_worst_case`：将 timeout 作为最坏上界纳入估计（保守），适合把容错超时也算进周期 SLA 的场景。并行动作取最大值。
- 经验：如果你给每个 step 都加了较大的 timeout（容错），但仍希望约束按真实节拍衡量，用 `must_complete_within`；如果你希望"连超时都算进去仍要满足"，用 `must_complete_within_worst_case`。
- `delay:` 会计入两种估计中的关键路径。

### Causality（因果性）
- 编译器自动从拓扑图（`relation` + `detects`）推断 action→sensor 的因果可达性
- 只要拓扑声明完整，**不需要**显式写 `causality:` 约束（显式声明仅用于文档可读性）
- `wait: sensor == false` 与 `== true` 的因果检查完全相同
- AND/OR wait 中的每个传感器都会被独立检查
- parallel 块中跨分支的无关因果配对会被自动跳过（不会误报）

**race 块注意事项：**
- 每个 branch 需要 `wait:` + `then: goto`，step 级需要 `timeout:`
- action 应在 race 之前的 step 中完成，race 内部只放 wait

**`goto task.step`：** 可跳转到目标 task 的指定 step。目标 step 必须存在。`on_complete` 必须是 task 最后一行，不能在其后再写 step。

---

## 推荐的"可验证任务骨架"（模板）

当工程师没有提供更复杂的状态机需求时，优先使用这个骨架，能显著降低 Liveness/Timing/Causality 的失败概率：

- `ready`：只做人工启动等待（`allow_indefinite_wait: true`）
- `cycle`：完整自动流程，每个 `wait` 必带 `timeout`
- `fault_handler`：收回到安全位 + 报警日志 + 回到 `ready`

示例结构（仅示意，不要原样照抄，按你的设备名替换）：
```plc
[tasks]
task cycle:
    step do_something:
        # action + wait + timeout
    on_complete: goto ready

task fault_handler:
    step safe:
        # retract / stop motor
    step alarm:
        action: log "fault"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto cycle
```

---

## 完整代码示例

### 基础气缸顺序控制（plc_main + relation 模式）

```plc
[topology]

device plc_main: plc {
    purpose: "控制器本体与工艺 I/O 端口映射",
    ports: [Y0:digital:producer, Y1:digital:producer, X0:digital:consumer, X1:digital:consumer, X2:digital:consumer, X3:digital:consumer, X4:digital:consumer]
}

device start_button: sensor {
    purpose: "操作员启动按钮，触发双缸顺序动作循环",
    subtype: "push_button",
    debounce: 20ms
}

device valve_A: solenoid_valve {
    purpose: "驱动 A 缸伸出/缩回的气动电磁阀",
    response_time: 20ms
}

device cyl_A: cylinder {
    purpose: "执行顺序动作第一步的气缸",
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor {
    purpose: "检测 A 缸已完全伸出到位的限位开关",
    subtype: "limit_switch"
}

device sensor_A_ret: sensor {
    purpose: "检测 A 缸已完全缩回到位的限位开关",
    subtype: "limit_switch"
}

relation { from: start_button.out, to: plc_main.X4, via: reports_to }
relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }
relation { from: sensor_A_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]

task cycle:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto fault_handler
    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler
    on_complete: goto ready

task fault_handler:
    step safe:
        action: retract cyl_A
    step alarm:
        action: log "动作超时报警"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto cycle
```

### 条件分支（无物理拓扑极简示例）

```plc
[topology]

device mode_switch: digital_input {
    purpose: "模式选择开关，true 选择 A 流程，false 选择 B 流程"
}

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
| PLC 控制器 | `plc_main` | `plc_main` |
| 气缸 | `cyl_<功能>` | `cyl_push`, `cyl_clamp`, `cyl_press` |
| 电磁阀 | `valve_<对应气缸功能>` | `valve_push`, `valve_clamp` |
| 传感器 | `sensor_<气缸>_<状态>` | `sensor_push_ext`, `sensor_push_ret` |
| 电机 | `motor_<功能>` | `motor_conveyor`, `motor_spindle` |
| 输出口 | `Y0`, `Y1`, ... | 在 plc_main.ports 中声明 |
| 输入口 | `X0`, `X1`, ... | 在 plc_main.ports 中声明 |
| 模拟输入 | `AI0`, `AI1`, ... | 独立设备或 plc_main.ports |
| 模拟输出 | `AO0`, `AO1`, ... | 独立设备或 plc_main.ports |
| 任务 | 动作导向 | `cycle`, `init`, `fault_handler`, `ready` |
| 步骤 | 动词_名词 | `extend_push`, `wait_clamp`, `retract_all` |

工程师可以在阶段二要求使用自己的命名习惯，以上仅为默认值。

---

## 必须包含的标准结构

每个生成的 `.plc` 文件都必须包含：

1. **对应的 `.system.md` 文件** — 在阶段零生成并经工程师确认，作为所有决策的语义锚点
2. **plc_main 设备** — 声明所有 I/O 端口，必须有 `purpose:`
3. **所有设备必须有 `purpose:`** — 编译器门禁会拒绝缺少 purpose 的设备
4. **传感器必须有 `subtype:`** — 标准做法，明确传感器类型
5. **fault_handler 任务** — 缩回所有气缸 / 关闭所有电机，输出报警日志
6. **ready 任务** — 等待启动按钮，标记 `allow_indefinite_wait: true`
7. **所有 wait 都有 timeout** — 指向 fault_handler（人工等待除外）
8. **工程师确认的所有安全约束** — 不能遗漏
9. **完整的 relation 连接链** — 每个设备的 relation 和传感器的 detects 必须正确声明（编译器据此自动推断因果性）
10. **每个气缸都应有 _ext 和 _ret 两个传感器** — 即使当前流程中某个方向没有 `wait` 确认

---

## 更多参考示例

`examples/` 目录下有多个已通过验证的 `.plc` 文件，生成前务必参考：

| 文件 | 场景 | 涉及模式 |
|------|------|----------|
| `two_cylinder.plc` | 双气缸顺序动作 | 基础顺序、conflicts_with、plc_main + relation |
| `assembly_station.plc` | 双传送带+推缸+压装+出料 | 多设备顺序、requires vs conflicts_with、timing |
| `analog_pressure_demo.plc` | 液压站比例阀压力控制 | analog_input/output、set_analog、external、阈值 wait |
| `pid_loop.plc` | PID 闭环压力控制 | pid 设备、analog I/O |
| `nuclear_coolant_isolation.plc` | 核电站隔离阀控制 | SIL3 双冗余传感器、OR 容错、parallel 并行关阀、严格时序硬限 |

`.system.md` 参考样板：

| 文件 | 场景 | 安全等级 |
|------|------|----------|
| `two_cylinder.system.md` | 教学演示台 | 常规工业防护 |
| `nuclear_coolant_isolation.system.md` | 核电站一回路隔离阀 | SIL3 / 核安全 1E 级 |

遇到类似场景时，先读取对应示例文件及其 `.system.md` 了解已验证的模式，再生成新文件。

---

## 生成覆盖度自检

生成 `.system.md` + `.plc` 文件后，对照以下清单确认：

- [ ] 是否先生成了 `.system.md` 并经工程师确认？
- [ ] `.system.md` 中的安全等级是否与 `.plc` 中的约束严格程度匹配？
- [ ] 所有设备是否都有 `purpose:` 字段？
- [ ] 所有传感器是否都有 `subtype:` 字段？
- [ ] 是否使用 `plc_main: plc { ports: [...] }` 声明 I/O？
- [ ] 所有设备间连接是否使用 `relation { from, to, via }` 声明？
- [ ] 所有气缸是否都有 _ext 和 _ret 传感器，且 `detects` 关系正确？
- [ ] 所有 wait 是否都有 timeout？（人工等待除外）
- [ ] fault_handler 是否覆盖了所有执行器的安全复位？
- [ ] 是否需要 `race`/`parallel`/`repeat`/`if-else`/`goto task.step`？
- [ ] 是否有时序约束？
- [ ] 模拟量设备是否声明了 `range` 和 `unit`？
