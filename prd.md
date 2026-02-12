# RustPLC — 基于形式化验证的工业控制系统

## Product Requirements Document (PRD)

**版本**: 2.0
**日期**: 2026-02-12
**状态**: V1 Complete / V2 Draft

---

## 1. 项目愿景

用 Rust 替代传统 PLC 编程语言（梯形图/ST/FBD），建立一套 **"程序即结构"** 的工业控制系统。用户用极简的领域专用语言（DSL）描述物理拓扑、控制逻辑和安全约束，系统在编译期自动完成形式化验证（安全性、活性、时序、因果链），验证通过后生成确定性 Rust 执行代码。

**核心理念**：不是"写程序控制设备"，而是"声明物理事实和意图，让编译器证明它是安全的，然后自动执行"。

---

## 2. 目标用户

| 用户角色 | 使用方式 | 技术水平 |
|---------|---------|---------|
| 自动化工程师 | 用 DSL 直接编写 `.plc` 文件 | 了解气缸、电磁阀、传感器，不需要懂 Rust |
| 现场调试人员 | 用自然语言描述需求，AI 翻译为 DSL | 只需要懂设备工艺流程 |
| 系统集成商 | 定义硬件拓扑，复用逻辑模块 | 了解总线协议和硬件接线 |

---

## 3. 核心需求

### 3.1 需求总览

| 编号 | 需求 | 优先级 | 对应阶段 |
|------|------|--------|---------|
| R1a | 用 DSL 描述控制逻辑 | P0 | Phase 1 |
| R1b | 自然语言 AI 翻译为 DSL | P1 | Phase 5 |
| R2 | 用简单方式定义 IO、电磁阀、气缸之间的物理关系 | P0 | Phase 1 |
| R3 | 用简单语言输入安全约束和限制条件 | P0 | Phase 1 |
| R4 | 编译时自动转成 Rust，自动进行形式化验证 | P0 | Phase 2-3 |
| R5 | 运行时确定性执行 + 实时故障诊断 | P1 | Phase 4-5 |

### 3.2 形式化验证的四项核心检查

#### 3.2.1 状态互斥（Safety）

**目标**：证明在所有可达状态下，不会出现违反互斥约束的状态。

**场景示例**：
- 用户声明 `cyl_A.extended conflicts_with cyl_B.extended`
- 编译器对所有可达状态进行验证，证明不存在两缸同时伸出的状态
- 如果逻辑中存在并行路径可能导致两缸同时伸出，编译报错并给出反例路径

**验证方法**：
- 将状态机转为 SMT 布尔公式
- 采用分层验证策略：
  1. 首先尝试 k-induction（归纳证明）：若归纳步成立，则性质对所有深度成立，获得完备证明
  2. 若 k-induction 无法收敛（存在归纳反例），回退到 BMC（有界模型检查）搜索反例。展开深度按以下策略确定：
     - 默认最大展开深度 = 状态机中不同状态节点数量（即无循环时的最长简单路径上界）
     - 用户可通过配置项 `bmc_max_depth` 手动指定上限
     - 若状态机包含 SCC（强连通分量），对每个 SCC 至少展开其节点数 + 1 层以覆盖完整循环
     - 展开深度超过用户配置上限时，停止搜索并输出警告
  3. 对于状态空间较小的系统（状态数 < 可配置阈值），直接进行穷举验证
- 用 Z3 求解器检查 `safety_constraint AND reachable_state` 是否有解
- 有解 = 存在违反安全约束的路径 = 编译失败
- 验证结论分两级：
  - **完备证明**（k-induction 或穷举通过）：输出"Safety 已完备证明：性质对所有可达状态成立"
  - **有界验证**（仅 BMC 通过）：输出"WARNING: Safety 在深度 N 内未发现反例，但未获得完备证明。建议增大 bmc_max_depth 以提升有界覆盖，或调整模型以帮助 k-induction 收敛"

**错误输出示例**：

```
ERROR [safety] 状态互斥违反
  约束: cyl_A.extended conflicts_with cyl_B.extended (main.plc:12)
  违反路径:
    1. init -> step_1 (extend cyl_A)
    2. step_1 中 sensor_A_ext 超时 -> error_recovery
    3. error_recovery 未等待 cyl_A 缩回 -> step_3 (extend cyl_B)
    4. 此时 cyl_A 可能仍处于 extended 状态  <-- 冲突
  建议: 在 error_recovery 中添加 retract cyl_A 并等待 sensor_A_ret
```

#### 3.2.2 活性保障（Liveness）

**目标**：证明程序绝不会死锁——每个非人工等待的状态都有出路。注意：工控程序通常是常驻循环运行的（如 ready -> work -> ready -> ...），Liveness 不要求程序"最终终止"，而是要求"不会卡死在某个中间状态"。

**场景示例**：
- 用户写了 `wait: sensor_A == true` 但没写 timeout
- 如果 sensor_A 物理损坏永远为 false，程序将永远卡在此处
- 编译器检测到无超时的等待，报错

**验证方法**：
- 构建状态转移图
- 检查是否存在无出边的非终态节点（注意：常驻循环中的 task 通过 on_complete: goto 形成闭环，这是合法的，不算死锁）
- 标记为 `on_complete: unreachable` 的 task 视为"所有出边均为内部跳转的已封闭节点"，不算无出边的非终态（前提：该 task 内所有执行路径都以 goto 或 timeout->goto 结尾，编译器会验证这一前提，若存在未跳转的路径则报错）
- 标记为 `allow_indefinite_wait: true` 的 wait 语句豁免 timeout 检查（典型用途：等待人工按下启动按钮）
- 除上述两种显式豁免外，所有 `wait` 语句必须有对应的 `timeout` 分支
- 检查是否存在不包含 `allow_indefinite_wait` 或 `timeout` 出边的强连通分量（即：一组状态互相可达但全都没有超时出口，意味着一旦进入就可能永远出不来）

**错误输出示例**：

```
ERROR [liveness] 潜在死锁
  位置: task init, step 2 (main.plc:25)
  原因: wait: sensor_A == true 没有 timeout 分支
  物理分析: sensor_A 依赖链路 [Y0 -> valve_A -> cyl_A -> sensor_A]
            如果链路中任一环节故障, sensor_A 将永远为 false
  建议: 添加 timeout: 600ms -> goto error_handler
```

#### 3.2.3 时序包络（Timing）

**目标**：如果物理属性定义的动作时间与时序要求矛盾，直接编译报错。

**场景示例**：
- 用户定义 `cyl_A.stroke_time: 200ms`
- 用户要求 `step_1 must_complete_within 100ms`
- 物理上不可能：气缸需要 200ms 才能到位，但要求 100ms 完成

**验证方法**：
- 收集每个 step 涉及的所有物理动作时间参数（阀响应 + 机构行程等）
- 计算最长完成时间（保守最坏情况关键路径）：
  - 同一 step 内多条 action 并行执行，取最大动作时间
  - 如果 step 含 `wait + timeout`，则用 timeout 作为该 step 的上界
  - 含 `allow_indefinite_wait` 的 step 不允许被 `must_complete_within` 约束覆盖（否则无法给出上界，编译报错）
- 与用户声明的时序约束对比
- 最长完成时间 > 约束时间 = 编译失败（最坏情况下无法按时完成）

**错误输出示例**：

```
ERROR [timing] 时序包络违反
  约束: step_1 must_complete_within 100ms (main.plc:30)
  分析: step_1 包含动作 extend cyl_A
        cyl_A.stroke_time = 200ms (main.plc:6)
        最长完成时间 = 200ms > 约束 100ms
  结论: 最坏情况下无法在 100ms 内完成此动作
```

#### 3.2.4 因果链闭环（Causality）

**目标**：验证从输出到传感器反馈的物理路径是否完整连通。

**场景示例**：
- 用户在逻辑中写了 `extend cyl_A` 然后 `wait sensor_A_ext`
- 但拓扑定义中漏写了 `valve_A -> cyl_A` 的连接
- 编译器发现从 Y0 到 sensor_A_ext 的因果链断裂

**验证方法**：
- 对每个动作+等待对，在拓扑图上做可达性分析
- 从输出端口沿拓扑边遍历，检查是否能到达等待的传感器
- 不可达 = 因果链断裂 = 编译失败

**错误输出示例**：

```
ERROR [causality] 因果链断裂
  动作: extend cyl_A (main.plc:22)
  等待: sensor_A_ext == true (main.plc:23)
  期望链路: Y0 -> valve_A -> cyl_A -> sensor_A_ext
  实际链路: Y0 -> valve_A -> ??? (valve_A 未连接到任何气缸)
  建议: 在拓扑定义中添加 connected_to: valve_A 到 cyl_A
```

---

## 4. 系统架构

### 4.1 整体分层

系统分为三层：输入层、编译层、运行时层。

**输入层 (Input Layer)**

```
  自然语言 ---AI翻译---> .plc DSL 源文件
  "A缸伸出后B缸再伸出"    device cyl_A: ...
                          task init: ...
```

**编译层 (Compiler Layer)**

```
  Phase A: 解析器 (Parser)
    .plc 源文件 --> AST (抽象语法树)
           |
           v
  Phase B: 语义分析 + IR 生成
    AST --> TopologyGraph  (有向图: 节点=设备, 边=物理连接)
        --> StateMachine   (状态+转移+守卫条件)
        --> ConstraintSet  (safety/timing/causality 约束)
        --> TimingModel    (每个动作的时间区间)
           |
           v
  Phase C: 形式化验证引擎
    IR --> Z3 SMT 公式
    [x] Safety:    状态互斥检查
    [x] Liveness:  死锁/活锁检测
    [x] Timing:    时序包络验证
    [x] Causality: 因果链闭环检查
    验证失败 -> 人类可读的错误报告 + 修复建议
    验证通过 -> 继续
           |
           v
  Phase D: 代码生成
    IR --> 确定性状态查找表
       --> Rust 源代码 (可审计)
       --> 编译为目标平台二进制
```

**运行时层 (Runtime Layer)**

```
  确定性执行内核:
    读取输入 --> 查表决策 --> 写入输出
       ^                        |
       +--- 硬实时循环 ----------+

  运行时监控:
    超时检测 -> 因果回溯 -> 诊断报告
    状态日志 -> 可回放调试

  硬件抽象层 (HAL):
    EtherCAT | Modbus | GPIO | 模拟器
```

### 4.2 数据流

```
.plc 源文件
    |
    v
  [Parser] --> AST
    |
    v
  [Semantic Analyzer]
    |--- TopologyGraph   (有向图: 节点=设备, 边=物理连接)
    |--- StateMachine    (状态+转移+守卫条件)
    |--- ConstraintSet   (safety/timing/causality 约束)
    |--- TimingModel     (每个动作的时间区间)
    |
    v
  [Verification Engine]
    |-- Safety Checker    --> Z3: k-induction 或 BMC 验证不存在冲突
    |-- Liveness Checker  --> 死锁检测: 不存在无出路的中间状态
    |-- Timing Checker    --> 区间算术: max_time <= constraint
    |-- Causality Checker --> 图可达性: output --*--> sensor
    |
    v (全部通过)
  [Code Generator]
    |--- state_table.rs   (查找表: input_snapshot + timeout_events -> output_action)
    |--- runtime.rs       (主循环 + HAL 调用)
    |--- diagnostics.rs   (运行时因果诊断逻辑)
```

### 4.3 关键技术选型

| 组件 | 技术选择 | 理由 |
|------|---------|------|
| 实现语言 | Rust | 内存安全、零成本抽象、适合嵌入式和实时系统 |
| DSL 解析器 | pest 或 nom | Rust 生态成熟的 parser 库，pest 用 PEG 语法更直观 |
| 形式化验证 | Z3 (通过 z3 crate) | 工业级 SMT 求解器，支持布尔逻辑、整数算术、位向量 |
| 拓扑图 | petgraph | Rust 标准图算法库，支持有向图遍历和可达性分析 |
| 硬件通信 | 可插拔 HAL trait | 首选 EtherCAT (ethercrab)，兼容 Modbus、GPIO |
| 可视化 | Graphviz DOT 输出 | 编译器可导出拓扑图和状态机的 DOT 文件供调试 |
| 序列化 | serde | IR 的序列化/反序列化，支持调试和缓存 |

---

## 5. DSL 语言设计

### 5.1 设计原则

1. **声明式优先**：描述"是什么"而不是"怎么做"
2. **物理直觉**：关键词对应物理概念（device, extend, retract, wait）
3. **约束显式**：所有安全限制必须明确写出，不依赖隐式假设
4. **零歧义**：每条语句只有一种解释方式
5. **错误友好**：编译错误信息用中文，包含位置、原因、建议

### 5.2 文件结构

一个 `.plc` 文件由三个段落组成，必须按顺序出现：

```
[topology]    # 物理拓扑定义（必须）
[constraints] # 约束声明（必须）
[tasks]       # 控制逻辑（必须）
```

### 5.3 物理拓扑定义 [topology]

```plc
[topology]

# ===== 控制器端口 =====
device Y0: digital_output               # 数字输出端口
device Y1: digital_output
device Y2: digital_output               # 报警灯输出
device X0: digital_input                # 数字输入端口
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input                # 启动按钮

# ===== 操作面板 =====
device start_button: digital_input {    # 启动按钮
    connected_to: X4
    debounce: 20ms
}

device alarm_light: digital_output {    # 报警灯
    connected_to: Y2
}

# ===== 电磁阀 =====
device valve_A: solenoid_valve {
    type: "5/2"                         # 五口二位
    connected_to: Y0                    # 电气连接到 Y0
    response_time: 15ms                 # 电磁阀响应时间
}

device valve_B: solenoid_valve {
    type: "5/2"
    connected_to: Y1
    response_time: 15ms
}

# ===== 气缸 =====
device cyl_A: cylinder {
    type: double_acting                 # 双作用气缸
    connected_to: valve_A               # 气路连接到 valve_A
    stroke: 100mm                       # 行程
    stroke_time: 200ms                  # 全行程伸出时间
    retract_time: 180ms                 # 缩回时间
}

device cyl_B: cylinder {
    type: double_acting
    connected_to: valve_B
    stroke: 150mm
    stroke_time: 300ms
    retract_time: 250ms
}

# ===== 传感器 =====
device sensor_A_ext: sensor {
    type: magnetic                      # 磁性开关
    connected_to: X0                    # 电气连接到 X0
    detects: cyl_A.extended             # 检测 cyl_A 伸出到位
}

device sensor_A_ret: sensor {
    type: magnetic
    connected_to: X1
    detects: cyl_A.retracted            # 检测 cyl_A 缩回到位
}

device sensor_B_ext: sensor {
    type: magnetic
    connected_to: X2
    detects: cyl_B.extended
}

device sensor_B_ret: sensor {
    type: magnetic
    connected_to: X3
    detects: cyl_B.retracted
}
```

**设备类型关键词**：

| 类型 | 关键词 | 必填属性 | 可选属性 |
|------|--------|---------|---------|
| 数字输出 | digital_output | 无 | inverted, connected_to |
| 数字输入 | digital_input | 无 | inverted, debounce, connected_to |
| 电磁阀 | solenoid_valve | connected_to, response_time | type |
| 气缸 | cylinder | connected_to, stroke_time, retract_time | type, stroke |
| 传感器 | sensor | connected_to, detects | type |
| 电机 | motor | connected_to | rated_speed, ramp_time, positions |

说明：

- `motor` 的 `positions`（可选）声明可检测的离散位置，自动生成状态 `motor_name.position_<label>` 供 `sensor.detects` 使用
- `digital_input` 和 `digital_output` 有两种用法：
  - **裸端口声明**：`device Y0: digital_output` — 仅声明物理端口，无额外属性
  - **别名设备声明**：`device start_button: digital_input { connected_to: X4 }` — 为已声明的裸端口创建语义别名，`connected_to` 指向裸端口名称。别名设备在逻辑中可直接引用（如 `wait: start_button == true`），编译器会自动解析到底层端口

### 5.4 约束声明 [constraints]

```plc
[constraints]

# ===== 状态互斥 (Safety) =====
safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

safety: valve_A.on conflicts_with valve_B.on
    reason: "气源压力不足以同时驱动两个阀"

# ===== 时序约束 (Timing) =====
timing: task.init must_complete_within 5000ms
    reason: "初始化超过5秒视为异常"

timing: task.init.extend_A must_complete_within 500ms
    reason: "单步动作不应超过500ms"

timing: task.init.retract_A must_start_after 100ms
    reason: "反向动作前需要泄压等待"

# ===== 因果链声明 (Causality) =====
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext
    reason: "Y1 驱动 valve_B 推动 cyl_B 由 sensor_B_ext 检测"
```

**约束类型汇总**：

| 类型 | 语法 | 含义 |
|------|------|------|
| safety | A conflicts_with B | 状态 A 和 B 不能同时为真 |
| safety | A requires B | 状态 A 为真时 B 必须为真 |
| timing | X must_complete_within Nms | X 必须在 N 毫秒内完成 |
| timing | X must_start_after Nms | X 必须在 N 毫秒后才能开始 |
| causality | A -> B -> C -> D | 从 A 到 D 的物理因果链必须连通 |

**约束作用域**：

- task 级：`task.<task>`
- step 级：`task.<task>.<step>`

### 5.5 控制逻辑 [tasks]

#### 5.5.0 执行语义规则

**单个 step 内多条 action 的执行语义**：

同一个 step 中出现多条 action 时，它们被视为**同时发出**（并行），在同一个扫描周期内写入所有输出。这意味着：

- **Timing 计算**：同一 step 内多条 action 的时间取最大值（并行），而非累加（串行）
- **Safety 检查**：同一 step 内的多条 action 会被视为同时生效，编译器会检查它们是否违反 conflicts_with 约束
- **物理含义**：对应"同时给多个电磁阀通电"这类操作

示例：

```plc
# 这两条 action 在同一周期同时执行（并行）
step safe_position:
    action: retract cyl_A        # 同时发出
    action: retract cyl_B        # 同时发出
    # 时间 = max(cyl_A.retract_time, cyl_B.retract_time)，不是两者之和
```

如果需要**串行执行**（先做 A 再做 B），必须拆成多个 step：

```plc
# 这两条 action 串行执行
step retract_A_first:
    action: retract cyl_A
    wait: sensor_A_ret == true
    timeout: 500ms -> goto fault_handler
step retract_B_second:
    action: retract cyl_B
    wait: sensor_B_ret == true
    timeout: 700ms -> goto fault_handler
```

#### 5.5.1 基本顺序执行

```plc
[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 600ms -> goto fault_handler

    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler

    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 800ms -> goto fault_handler

    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 700ms -> goto fault_handler

    on_complete: goto ready
```

#### 5.5.2 等待与跳转

```plc
task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true     # 显式允许无限等待
    on_complete: goto main_cycle
```

#### 5.5.3 故障处理

```plc
task fault_handler:
    step safe_position:
        action: retract cyl_A
        action: retract cyl_B
    step alarm:
        action: set alarm_light on
        action: log "动作超时，已执行安全复位"
    on_complete: goto ready
```

#### 5.5.4 并行执行

两个动作同时开始，全部完成后继续：

```plc
task parallel_demo:
    step move_together:
        parallel:
            branch_A:
                action: extend cyl_A
                wait: sensor_A_ext == true
                timeout: 600ms -> goto fault_handler
            branch_B:
                action: extend cyl_B
                wait: sensor_B_ext == true
                timeout: 800ms -> goto fault_handler
    on_complete: goto next_task
```

注意：如果 cyl_A 和 cyl_B 有 conflicts_with 约束，编译器会在此处报 safety 错误。

**并行语义补充**：
- `parallel` 内所有分支同时开始，所有分支完成后该 step 才算完成
- 任一分支触发 `goto`（包含 timeout 跳转）时，整个 `parallel` 立即结束并跳转，其他分支被取消
- 同周期内多个分支同时触发跳转时，按分支声明顺序优先（编译器提示此优先级）

#### 5.5.5 竞争分支

两个条件谁先满足走谁的分支（典型场景：半圈旋转判断）：

```plc
task search_position:
    step start_motor:
        action: set motor on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 2000ms -> goto fault_handler
    on_complete: unreachable          # race 必定跳转，不会走到这里
```

**race 语义补充**：
- 每个分支必须包含 `wait` 与 `then: goto <task>`（可指向 task 内 step）
- 首个满足 `wait` 的分支获胜并跳转，其余分支取消
- 同周期多个分支同时满足时，按分支声明顺序优先

#### 5.5.6 流程控制关键词汇总

| 关键词 | 含义 | 示例 |
|--------|------|------|
| step | 定义一个执行步骤 | step extend_A: |
| action | 执行一个物理动作 | action: extend cyl_A |
| wait | 等待条件成立 | wait: sensor_A == true |
| timeout | 超时跳转 | timeout: 500ms -> goto error |
| goto | 跳转到另一个 task | goto fault_handler |
| on_complete | task 正常完成后的跳转 | on_complete: goto ready |
| parallel | 并行执行多个分支 | parallel: ... |
| race | 竞争执行，先到先得 | race: ... |
| then | race 分支成立后的跳转 | then: goto process_A |
| allow_indefinite_wait | 显式允许无限等待 | allow_indefinite_wait: true |
| unreachable | 标记不可达的完成点 | on_complete: unreachable |

---

## 6. 自然语言接口

### 6.1 目标

允许不懂 DSL 语法的现场人员用自然语言描述控制意图，由 AI（LLM）自动翻译为合法的 `.plc` 文件。翻译后的 DSL 仍然经过完整的编译和形式化验证流程，AI 只负责"写代码"，不负责"保证安全"。

### 6.2 工作流程

```
用户输入自然语言
    |
    v
[LLM 翻译层]
    |  输入: 自然语言 + 已有的拓扑定义（作为上下文）
    |  输出: .plc DSL 代码
    |
    v
[人工审查] (可选但推荐)
    |  用户确认生成的 DSL 是否符合意图
    |
    v
[编译器] --> 形式化验证 --> 通过/报错
```

### 6.3 自然语言输入示例

**输入**：

> 设备有两个气缸A和B，各有伸出和缩回传感器。A缸用Y0控制，B缸用Y1控制。
> 初始化时先伸出A缸，等到位后缩回，然后伸出B缸，等到位后缩回。
> A缸和B缸不能同时伸出。每个动作超时500毫秒就报警。

**AI 翻译输出**：

```plc
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input

device start_button: digital_input {
    connected_to: X4
    debounce: 20ms
}

device valve_A: solenoid_valve {
    connected_to: Y0
    response_time: 20ms
}
device valve_B: solenoid_valve {
    connected_to: Y1
    response_time: 20ms
}
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 300ms
    retract_time: 300ms
}
device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 300ms
    retract_time: 300ms
}
device sensor_A_ext: sensor { connected_to: X0, detects: cyl_A.extended }
device sensor_A_ret: sensor { connected_to: X1, detects: cyl_A.retracted }
device sensor_B_ext: sensor { connected_to: X2, detects: cyl_B.extended }
device sensor_B_ret: sensor { connected_to: X3, detects: cyl_B.retracted }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸不能同时伸出"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto fault_handler
    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler
    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 500ms -> goto fault_handler
    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 500ms -> goto fault_handler
    on_complete: goto ready

task fault_handler:
    step safe:
        action: retract cyl_A
        action: retract cyl_B
    step alarm:
        action: log "动作超时报警"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
```

### 6.4 AI 翻译层的约束规则

1. **AI 不能跳过物理参数**：如果用户没说 stroke_time，AI 必须用保守默认值并在注释中标注"需人工确认"
2. **AI 不能省略约束**：用户提到的任何安全限制必须出现在 [constraints] 段
3. **AI 生成的代码必须通过编译**：AI 的输出不享有任何特权，必须经过完整验证
4. **AI 应生成注释**：对不确定的参数值加注释说明推断依据

---

## 7. 编译器内部设计

### 7.1 编译流水线

```
Phase A: 词法分析 + 语法分析
    输入: .plc 源文件 (UTF-8 文本)
    工具: pest PEG parser
    输出: AST (抽象语法树)
    错误: 语法错误 (缺少冒号、缩进错误、未知关键词)

Phase B: 语义分析
    输入: AST
    检查:
      - 所有 device 引用是否已定义
      - connected_to 目标是否存在且类型兼容
      - detects 目标是否是合法的设备状态
      - 所有 goto 目标 task 是否存在
      - 所有 wait 引用的传感器是否已定义
    输出: IR (中间表示)
      - TopologyGraph: petgraph 有向图
      - StateMachine: 状态 + 转移 + 守卫
      - ConstraintSet: 约束列表
      - TimingModel: 时间区间集合
    错误: 语义错误 (未定义引用、类型不匹配)

Phase C: 形式化验证
    输入: IR
    子阶段:
      C1. Safety 检查
          - 遍历所有 conflicts_with / requires 约束
          - 对每条约束生成 Z3 公式
          - 分层验证策略:
            1. 首先尝试 k-induction (归纳证明)，若成立则获得完备证明
            2. 若 k-induction 无法收敛，回退到 BMC 搜索反例，
               展开深度 = 状态节点数 (用户可通过 bmc_max_depth 配置上限，
               含 SCC 时至少展开 SCC 节点数 + 1 层)
            3. 状态空间较小时 (状态数 < 可配置阈值) 直接穷举
          - 验证结论分两级:
            完备证明 (k-induction/穷举): "Safety 已完备证明"
            有界验证 (仅 BMC): "WARNING: 深度 N 内未发现反例，未获完备证明"
      C2. Liveness 检查
          - 遍历所有 wait 语句
          - 无 timeout 且无 allow_indefinite_wait -> 报错
          - 检查状态图是否存在无出边的非终态
            (通过 on_complete: goto 形成的常驻循环是合法的，不算死锁)
          - on_complete: unreachable 的 task 不算"无出边非终态"，
            但编译器必须验证该 task 内所有执行路径均以 goto 结尾；
            若存在未跳转的路径，报错："unreachable 标记无效，
            存在可能到达 on_complete 的路径"
          - 检查是否存在不包含 allow_indefinite_wait 或 timeout 出边的
            强连通分量 (一组状态互相可达但全都没有超时出口)
      C3. Timing 检查
          - 对每个 must_complete_within 约束:
            计算对应 task/step 的最坏情况关键路径时间（保守上界）
            关键路径 = 串行动作时间之和（同一 step 内多条 action 视为并行取最大值）
            若 step 含 wait + timeout，则该 step 上界取 timeout
            若 step 含 allow_indefinite_wait，则不能被 must_complete_within 覆盖，直接报错
            关键路径 > 约束 -> 报错 (最坏情况下无法按时完成)
          - 对每个 must_start_after 约束:
            计算从 task/step 的前驱结束到该 task/step 开始的最短间隔
            若前驱与当前之间无显式延迟机制 (如 wait 或 timeout) 能保证间隔 >= N，
            则报错："无法保证 X 在 Nms 后才开始，当前最短间隔为 Mms"
            典型用途: 电磁阀切换后需要泄压时间，气缸反向动作前需要等待
      C4. Causality 检查
          - 对每条 causality 链
          - 在 TopologyGraph 上做可达性检查
          - 对每个 action+wait 对，推断隐式因果链并验证
    输出: 验证报告 (通过/失败+详细错误)

Phase D: 代码生成
    输入: 已验证的 IR
    输出:
      - state_table.rs: 状态查找表
        每个状态键 = (当前task, 当前step, 输入快照, timeout_events)
        每个转移 = (目标状态, 输出动作集, 定时器操作)
      - runtime.rs: 主循环框架
        loop {
            inputs = hal.read_all();
            (next_state, outputs, timers) = STATE_TABLE[(current_state, inputs, timeout_events)];
            hal.write_all(outputs);
            current_state = next_state;
            update_timers(timers);
            check_diagnostics();
        }
      - diagnostics.rs: 运行时诊断
        基于 TopologyGraph 的因果回溯逻辑
```

### 7.2 IR 数据结构概要

```
TopologyGraph:
    nodes: [Device]           // 所有设备节点
    edges: [(Device, Device, ConnectionType)]
    // ConnectionType = Electrical | Pneumatic | Logical

StateMachine:
    states: [State]           // 每个 (task, step) 组合 = 一个状态
    transitions: [Transition] // (from, to, guard, actions, timers)
    initial: State

ConstraintSet:
    safety:    [(StateExpr, StateExpr, ConflictType)]
    timing:    [(Scope, Duration)]
    causality: [Vec<Device>]  // 因果链路列表

TimingModel:
    intervals: [(Action, MinTime, MaxTime)]
    // 从物理属性自动推导
```

---

## 8. 运行时设计

### 8.1 执行模型

运行时内核极其简单：经过形式化验证的状态查找表保证了确定性，内核只需要做三件事：

1. **读取输入**：从 HAL 读取所有数字输入的当前状态
2. **查表决策**：用 (当前状态, 输入快照, timeout_events) 查找状态表，得到 (下一状态, 输出动作)
3. **写入输出**：通过 HAL 写入所有数字输出

### 8.2 硬实时循环

```
循环周期: 可配置 (默认 1ms)
每个周期:
    1. 读取输入快照 (所有 digital_input)
    2. 更新定时器 (elapsed += cycle_time)
    3. 检查定时器超时 (生成超时事件)
    4. 查表: (state, inputs, timeout_events) -> (next_state, outputs)
    5. 写入输出
    6. 如果状态变化，记录日志
    7. 如果进入故障状态，触发诊断
```

**同周期冲突事件优先级规则**：

当传感器信号满足 wait 条件与 timeout 超时在同一个扫描周期内同时成立时，按以下固定优先级处理（编号即优先级，1 最高）：

1. **safety 约束优先于一切** — 如果同周期内某个输入变化会导致违反 safety 约束（如两缸同时到位），无论正常转移还是超时转移，都必须先进入安全处理。
2. **传感器信号（wait 成立）优先于超时** — 如果 wait 条件已满足，即使同周期 timeout 也到达，视为正常完成而非超时。理由：传感器信号代表"物理动作已完成"这一事实，超时只是"怀疑出了问题"的推测，事实优先于推测。
3. **race 分支中多个 wait 同时满足** — 按 race 中 branch 的声明顺序，排在前面的分支优先。编译器会在验证报告中提示此优先级，用户可通过调整 branch 顺序来控制。

总结：`safety > wait 成立（若多分支同时满足，按 race 声明顺序）> timeout`

这些优先级规则在代码生成阶段被固化到状态查找表中，运行时不做动态判断。

### 8.3 硬件抽象层 (HAL)

HAL 定义为 Rust trait，支持多种硬件后端：

```
trait HardwareAbstraction:
    fn read_digital(pin) -> bool
    fn write_digital(pin, value)
    fn read_all_inputs() -> InputSnapshot
    fn write_all_outputs(OutputSnapshot)

实现:
    EtherCatHal   -- EtherCAT 总线 (ethercrab crate)
    ModbusHal     -- Modbus RTU/TCP
    GpioHal       -- 直接 GPIO (树莓派等)
    SimulatorHal  -- 软件模拟器 (开发调试用)
```

### 8.4 运行时故障诊断

当检测到超时或异常状态时，运行时执行因果回溯：

```
诊断流程:
    1. 确定当前卡住的 step 和 wait 条件
    2. 从 wait 的传感器出发，在拓扑图上反向遍历
    3. 检查链路上每个节点的实际状态
    4. 找到第一个"期望状态 != 实际状态"的节点
    5. 生成诊断报告

示例:
    当前: task init, step extend_A, 等待 sensor_A_ext
    超时: 已等待 600ms (阈值 600ms)
    回溯:
      sensor_A_ext = false (期望 true) -- 传感器未触发
      cyl_A 状态未知 -- 无直接反馈
      valve_A = Y0 = true -- 输出正常
    诊断: "Y0 已输出但 sensor_A_ext 未触发。
           请检查: valve_A 是否卡死 / cyl_A 是否机械卡住 /
           sensor_A_ext 接线或位置是否正确"
```

---

## 9. 完整示例：半圈旋转判断

这是一个典型的非标自动化场景：电机旋转半圈，根据哪个传感器先触发来判断工件位置。

```plc
[topology]

device Y0: digital_output                # 电机控制
device X0: digital_input                 # 传感器A
device X1: digital_input                 # 传感器B
device X2: digital_input                 # 启动按钮

device start_button: digital_input {     # 启动按钮
    connected_to: X2
    debounce: 20ms
}

device motor_ctrl: motor {
    connected_to: Y0
    rated_speed: 60rpm
    ramp_time: 50ms                      # 启动到额定转速时间
    positions: [A, B]                    # 声明可检测的离散位置
}

device sensor_A: sensor {
    type: proximity
    connected_to: X0
    detects: motor_ctrl.position_A       # 检测A位置
}

device sensor_B: sensor {
    type: proximity
    connected_to: X1
    detects: motor_ctrl.position_B       # 检测B位置
}

[constraints]

# 半圈旋转时间: 60rpm = 1圈/秒, 半圈 = 500ms, 加上启动时间
timing: task.search.detect must_complete_within 800ms
    reason: "半圈旋转加启动不应超过800ms"

causality: Y0 -> motor_ctrl -> sensor_A
    reason: "电机旋转应能被传感器A检测"
causality: Y0 -> motor_ctrl -> sensor_B
    reason: "电机旋转应能被传感器B检测"

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
        # ... A 工艺的具体步骤
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl off
    step do_work_B:
        action: log "工件在B位置，执行B工艺"
        # ... B 工艺的具体步骤
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl off
    step alarm:
        action: log "电机旋转超时: 半圈内未检测到任何传感器信号"
        action: log "请检查: 电机是否旋转 / 传感器A,B是否正常 / 工件是否到位"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
```

**编译器对此示例的验证**：

- Safety: 无互斥约束，跳过
- Liveness: search.detect 有 timeout，process_A/B 有 on_complete，motor_fault 有 on_complete，ready 有 allow_indefinite_wait -- 全部通过
- Timing: motor 半圈理论时间 = ramp_time(50ms) + 半圈时间(500ms) = 550ms < 800ms -- 通过
- Causality: Y0 -> motor_ctrl -> sensor_A 连通，Y0 -> motor_ctrl -> sensor_B 连通 -- 通过

---

## 10. 实施路线图

### Phase 1: DSL 设计与解析器 (核心基础)

**目标**: 能解析 .plc 文件并生成 AST

**交付物**:
- DSL 语法的 PEG 文法定义 (pest grammar)
- Parser 实现: .plc -> AST
- 语义分析器: AST -> IR (TopologyGraph + StateMachine + ConstraintSet)
- 基本错误报告 (行号 + 中文错误信息)
- 3 个以上真实场景的 .plc 示例文件

**验收标准**:
- 能正确解析本 PRD 中所有示例
- 语法错误有清晰的中文提示
- IR 可序列化为 JSON 供调试检查

### Phase 2: 形式化验证引擎

**目标**: 四项核心验证全部可用

**交付物**:
- Z3 集成 (通过 z3 crate)
- Safety Checker: 状态互斥验证
- Liveness Checker: 死锁检测
- Timing Checker: 时序包络验证
- Causality Checker: 因果链闭环验证
- 人类可读的验证报告 (中文，含修复建议)

**验收标准**:
- 对故意写错的 .plc 文件，四类错误都能正确检出
- 对正确的 .plc 文件，验证通过且无误报
- 错误报告包含具体行号、违反路径、修复建议

### Phase 3: 代码生成与模拟运行

**目标**: 验证通过后能生成可执行的 Rust 代码

**交付物**:
- 状态查找表生成器: IR -> state_table.rs
- 运行时框架生成器: IR -> runtime.rs
- SimulatorHal: 软件模拟器后端
- 诊断逻辑生成器: IR -> diagnostics.rs
- CLI 工具: rustplc compile input.plc -> 可执行文件

**验收标准**:
- 生成的 Rust 代码可编译通过
- 在 SimulatorHal 上能正确执行所有示例场景
- 模拟注入传感器故障时，诊断报告正确

### Phase 4: 硬件接入与实时执行

**目标**: 在真实硬件上运行

**交付物**:
- EtherCAT HAL 实现 (ethercrab)
- Modbus HAL 实现
- 硬实时循环调优
- 运行时状态监控界面 (终端 TUI 或 Web)

**验收标准**:
- 在真实 EtherCAT/Modbus 设备上跑通半圈旋转判断场景
- 循环周期抖动 < 10%
- 运行时诊断在真实故障场景下给出正确报告

### Phase 5: 自然语言接口

**目标**: 支持自然语言输入

**交付物**:
- LLM 翻译层 (调用 Claude/GPT API)
- Prompt 工程: 包含 DSL 语法规范和示例的 system prompt
- 翻译结果的人工审查界面
- 翻译质量的自动化测试集

**验收标准**:
- 对 10 个典型自然语言描述，翻译成功率 > 80%
- 翻译结果能通过编译器验证
- 用户可以在审查界面修改后重新编译

---

## 11. 项目边界

### 11.1 本项目是什么

- 一个面向非标自动化设备的专用控制系统
- 一个带形式化验证的 DSL 编译器
- 一个确定性的实时执行内核
- 一个基于物理拓扑的自动故障诊断系统

### 11.2 本项目不是什么

- 不是通用编程语言（不支持任意计算）
- 不是 PLC 编程软件的替代品（不兼容 IEC 61131-3）
- 不是 SCADA 系统（不包含 HMI/组态画面）
- 不是运动控制系统（不处理插补、轨迹规划）

### 11.3 未来可能扩展（不在当前范围内）

- 模拟量输入输出 (analog_input / analog_output)
- PID 控制回路
- 多控制器协同
- 图形化 DSL 编辑器
- OPC UA 通信支持

---

## 12. 术语表

| 术语 | 含义 |
|------|------|
| DSL | Domain Specific Language，领域专用语言 |
| AST | Abstract Syntax Tree，抽象语法树 |
| IR | Intermediate Representation，中间表示 |
| SMT | Satisfiability Modulo Theories，可满足性模理论 |
| Z3 | 微软开发的 SMT 求解器 |
| BMC | Bounded Model Checking，有界模型检查 |
| HAL | Hardware Abstraction Layer，硬件抽象层 |
| PEG | Parsing Expression Grammar，解析表达式文法 |
| EtherCAT | 工业以太网总线协议 |
| Modbus | 工业通信协议 |
| Safety | 安全性：坏事永远不会发生 |
| Liveness | 活性：好事最终一定会发生 |
| petgraph | Rust 图数据结构和算法库 |
| pest | Rust PEG 解析器生成器 |
| serde | Rust 序列化/反序列化框架 |
| ethercrab | Rust EtherCAT 主站库 |

---

## 13. V2 迭代：DSL 语言增强（Phase 2.1）

**版本**: 2.0
**日期**: 2026-02-12
**来源**: 6 轮三方角色迭代测试（工程师 × AI 智能体 × 程序员），详见 `docs/dsl-defects-analysis.md`
**状态**: Draft

### 13.1 迭代背景

Phase 1-2 完成后，通过 plc-gen skill 进行了 6 轮真实工控场景测试（钻孔、贴标、装配、打磨、冲压折弯产线、涂胶），暴露了 DSL 在表达力上的 10 项缺陷。其中 8 项可在不破坏形式验证可判定性的前提下修复。

本迭代聚焦于 DSL 语言层面的增强，不涉及代码生成（Phase 3）和硬件接入（Phase 4）。

### 13.2 需求总览

| 编号 | 需求 | 优先级 | 工作量 | 对验证引擎的影响 |
|------|------|--------|--------|----------------|
| US-101 | `delay` 延时原语 | P0 | 小 | 无（语法糖） |
| US-102 | `repeat N` 有界循环 | P0 | 小 | 无（编译期展开） |
| US-103 | `wait` 支持 AND/OR 组合条件 | P0 | 中 | Causality 小改 |
| US-104 | Timing 关键路径计算修复 | P0 | 中 | 改善 Timing 准确性 |
| US-105 | Safety 验证器增加 `requires` 检查 | P0 | 中 | Safety BMC 增加不变式 |
| US-106 | Parallel 块因果链检查修复 | P1 | 中 | 修复 Causality 准确性 |
| US-107 | `if/else` 条件分支 | P1 | 中 | Safety BMC 处理条件守卫 |
| US-108 | `goto` 支持跳到 task 内指定 step | P1 | 小 | 无 |
| US-109 | 自定义设备状态 | P2 | 中 | BMC 状态空间增大但仍有限 |

### 13.3 各需求详细设计

#### US-101: `delay` 延时原语

**动机**: 工程师经常需要"延时N秒"（传送带送走、电机定时运转、保压），当前只能用"等待对立条件 + timeout"模拟，语义不清晰。

**语法设计**:

```plc
step wait_dry:
    delay: 2000ms
```

**编译语义**: `delay: Nms` 等价于编译器自动生成一个内部 wait + timeout 组合：
- 创建一个不可满足的内部守卫条件
- 设置 timeout = N ms，超时后转移到下一个 step（而非 fault_handler）
- Liveness 检查器识别 delay 为合法的有界等待，不报死锁

**与 timeout 的区别**:
- `delay: 2000ms` — 正常流程，2秒后继续下一步
- `timeout: 2000ms -> goto fault_handler` — 异常保护，2秒后跳到故障处理

**delay 也可以与 timeout 组合使用**（delay 期间如果发生外部异常）:
```plc
step wait_dry:
    delay: 2000ms
    timeout: 5000ms -> goto fault_handler   # 保护性超时（可选）
```

**验证影响**:
- Liveness: 识别 delay 为有界等待，通过
- Timing: 将 delay 值加入关键路径计算
- Safety / Causality: 无影响

**验收标准**:
- `delay: 2000ms` 编译通过，Liveness 不报错
- Timing 关键路径正确包含 delay 值
- 现有测试不受影响

---

#### US-102: `repeat N` 有界循环

**动机**: 工程师说"涂3遍"、"冲压2次"时，手动展开为 N 份步骤导致代码膨胀且难维护。

**语法设计**:

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

**编译语义**: `repeat N:` 在 AST → IR 阶段展开为 N 份顺序步骤，步骤名自动加后缀 `_1`, `_2`, ..., `_N`。展开后的 IR 与手动编写完全等价。

**展开示例**:
```
repeat 3: { body }
→ step glue_cycle_1: { body }
  step glue_cycle_2: { body }
  step glue_cycle_3: { body }
```

**约束**:
- N 必须是编译期常量正整数，范围 2..100
- repeat 块内不能嵌套 repeat（避免指数膨胀）
- repeat 块内的 goto 跳出循环是合法的（如 timeout -> goto fault_handler）

**验证影响**: 无。展开后的 IR 与手动编写等价，所有验证引擎无需修改。

**验收标准**:
- `repeat 3: { ... }` 编译通过，展开为 3 份步骤
- 展开后的步骤名正确（`_1`, `_2`, `_3`）
- 验证结果与手动展开一致
- `repeat 1:` 报错（无意义）
- `repeat 0:` 报错
- 嵌套 repeat 报错

---

#### US-103: `wait` 支持 AND/OR 组合条件

**动机**: 装配工位"两个零件都到位才继续"需要 AND 条件；分拣工位"任一传感器触发"需要 OR 条件。当前只能拆成多个顺序 step。

**语法设计**:

```plc
# AND: 两个条件都满足
step wait_both:
    wait: sensor_left == true AND sensor_right == true
    timeout: 5000ms -> goto fault_handler

# OR: 任一条件满足
step wait_either:
    wait: sensor_A == true OR sensor_B == true
    timeout: 3000ms -> goto fault_handler
```

**语法规则**:
- 支持 `AND` 和 `OR` 关键词（大写）
- 同一个 wait 中只能用一种连接词（不支持混合 AND/OR，避免优先级歧义）
- 如需混合逻辑，拆成多个 step 或用 race

**编译语义**:
- AND: 状态机转移守卫为合取条件 `guard = cond_A ∧ cond_B`
- OR: 状态机转移守卫为析取条件 `guard = cond_A ∨ cond_B`

**验证影响**:
- Safety: BMC 已追踪多设备状态，组合守卫不增加理论复杂度
- Liveness: 组合条件的 wait 仍需 timeout，检查逻辑不变
- Timing: 无影响
- Causality: 对 AND 中的每个传感器分别检查因果链；对 OR 中的每个传感器分别检查因果链

**验收标准**:
- `wait: A == true AND B == true` 编译通过
- `wait: A == true OR B == true` 编译通过
- `wait: A == true AND B == true OR C == true` 报语法错误（不支持混合）
- Causality 对组合条件中的每个传感器分别验证因果链
- 现有单条件 wait 不受影响

---

#### US-104: Timing 关键路径计算修复

**动机**: 当前 timing 检查器对 task 级别的 `must_complete_within` 把每个 step 的 `max(action_time, timeout)` 累加。timeout 值（安全裕量）远大于实际动作时间，导致计算结果严重偏大。

**当前行为**（有问题）:
```
step clamp:        action=300ms, timeout=500ms → worst_case=500ms
step stamp_down:   action=200ms, timeout=400ms → worst_case=400ms
step stamp_up:     action=200ms, timeout=400ms → worst_case=400ms
task total = 500+400+400 = 1300ms  ← 用的是 timeout 值
```

**修复方案**: 区分两种路径：

1. **正常路径时间**（action 实际时间）: 只累加 action 的设备时间（stroke_time + response_time 链）
2. **最坏路径时间**（timeout 累加）: 保持当前行为

`must_complete_within` 默认检查正常路径时间。新增 `must_complete_within_worst_case` 检查最坏路径。

```
正常路径: 300+200+200 = 700ms   ← action 实际时间
最坏路径: 500+400+400 = 1300ms  ← timeout 值
```

**语法变更**:
```plc
# 检查正常路径（默认，改后行为）
timing: task.cycle must_complete_within 8000ms

# 检查最坏路径（新增，等价于改前行为）
timing: task.cycle must_complete_within_worst_case 20000ms
```

**验收标准**:
- `must_complete_within` 使用 action 实际时间计算
- `must_complete_within_worst_case` 使用 timeout 值计算
- 现有 `must_complete_within` 的语义变更需要更新现有示例
- 无 timeout 的 step（纯 action）两种计算结果一致

---

#### US-105: Safety 验证器增加 `requires` 检查

**动机**: `safety: cyl_press.extended requires cyl_clamp.extended` 声明了"压装时夹紧缸必须保持伸出"，但当前 Safety 验证器跳过 requires，只检查 conflicts_with。

**当前行为**: `src/verification/safety.rs:149` — `if !matches!(rule.relation, SafetyRelation::ConflictsWith) { continue; }`

**修复方案**: 在 BMC 中增加 requires 不变式检查：

- `A requires B` 等价于 `A ∧ ¬B` 是不可达的
- 即：在所有可达状态中，如果 A 为真，则 B 必须为真
- 实现：在 BMC 的每一步检查 `state[A] == true && state[B] == false` 是否可达
- 如果可达，报错并给出违反路径

**错误报告格式**:
```
ERROR [safety] requires 约束违反
  位置: <input>:12:1
  约束: cyl_press.extended requires cyl_clamp.extended
  违反路径:
    1. cycle.clamp -> extend cyl_clamp (cyl_clamp.extended = true)
    2. cycle.clamp -> timeout -> fault_handler (cyl_clamp 未确认缩回)
    3. fault_handler -> cycle.press_down -> extend cyl_press
    4. 此时 cyl_press.extended = true 但 cyl_clamp.extended 状态未知
  建议: 在 fault_handler 中确保 cyl_clamp 缩回后再允许重新进入 cycle
```

**验收标准**:
- `requires` 约束被 Safety 验证器检查
- 违反 requires 时报错，包含违反路径和修复建议
- 现有通过验证的示例仍然通过（它们的顺序控制隐式满足 requires）
- 构造一个违反 requires 的测试用例，验证能正确检出

---

#### US-106: Parallel 块因果链检查修复

**动机**: parallel 块中不同分支操作不同设备，但编译器把所有分支的 action 与 step 级别的 wait 做因果检查，导致误报。

**当前行为**:
```plc
step start_both:
    parallel:
        branch_left:
            action: set motor_left on
        branch_right:
            action: set motor_right on
    wait: sensor_left_arrive == true
# → 编译器检查 motor_right -> sensor_left_arrive，报因果链断裂（误报）
```

**修复方案**: 因果链推断时，对 parallel 块中的 action-wait 对，只检查"同一因果链上"的关联：

1. 对 step 级别的 wait 中的每个传感器 S，在拓扑图中反向追溯找到其上游设备集合 U(S)
2. 对 parallel 块中的每个分支 B 的 action 目标设备 D(B)
3. 只有当 D(B) ∈ U(S) 时，才检查 D(B) → S 的因果链
4. 如果 D(B) ∉ U(S)，跳过该分支的因果检查

**验收标准**:
- 上述 parallel 场景不再误报
- 同一设备的 parallel action + wait 仍然正确检查因果链
- 现有测试不受影响

---

#### US-107: `if/else` 条件分支

**动机**: 根据设备状态选择不同路径（如选择开关决定加工模式），当前只能用 race 模拟，语义不匹配。

**语法设计**:

```plc
step check_mode:
    if: switch_A == true
        goto grind_coarse
    else:
        goto grind_fine
```

**编译语义**: 编译为两条带互补守卫的状态转移：
- 转移1: guard = `switch_A == true` → goto grind_coarse
- 转移2: guard = `switch_A == false` → goto grind_fine

**与 race 的区别**:
- `race`: 多个条件竞争等待，适合"不知道哪个先发生"的场景
- `if/else`: 检测已知状态，适合"根据当前状态选择路径"的场景
- `if/else` 不需要 timeout（条件是即时检测的，不是等待未来事件）

**约束**:
- if 条件使用与 wait 相同的条件语法（`device == true/false`）
- 必须有 else 分支（确保完备性，避免死路径）
- if/else 内只能是 goto，不能是 action（action 应放在目标 task 中）

**验证影响**:
- Safety: BMC 需处理条件分支的两条转移路径
- Liveness: if/else 天然有两条出边，不会死锁
- Timing: if/else 是瞬时判断，不贡献时间
- Causality: 无影响（if/else 不涉及 action-wait 对）

**验收标准**:
- `if: ... goto X else: goto Y` 编译通过
- 缺少 else 报错
- if 内放 action 报错
- Safety BMC 正确探索两条分支路径

---

#### US-108: `goto` 支持跳到 task 内指定 step

**动机**: 从 fault_handler 恢复时，可能需要跳到 task 中间的某个步骤而非从头开始。

**语法设计**:

```plc
# 当前语法（保持兼容）
timeout: 500ms -> goto fault_handler          # 跳到 fault_handler 的第一个 step

# 新增语法
timeout: 500ms -> goto cycle.press_down       # 跳到 cycle 任务的 press_down 步骤
```

**编译语义**: `resolve_task_target()` 支持 `task.step` 格式，返回指定的 (task_name, step_name) 状态。

**约束**:
- 目标 step 必须存在于目标 task 中
- 不能跳到 parallel/race 块内部的合成步骤

**验证影响**: 无。状态机已用 (task_name, step_name) 标识状态，所有验证引擎已处理任意状态间转移。

**验收标准**:
- `goto cycle.press_down` 编译通过
- 目标 step 不存在时报语义错误
- 跳到 parallel 内部合成步骤时报错
- 现有 `goto task_name` 语法不受影响

---

#### US-109: 自定义设备状态

**动机**: 三位电磁阀（伸出/中位/缩回）、多档位开关等设备需要超过 2 个状态。

**语法设计**:

```plc
device valve_3pos: solenoid_valve {
    connected_to: Y0
    response_time: 20ms
    states: [extend, neutral, retract]
}

device mode_switch: digital_input {
    connected_to: X5
    states: [auto, manual, maintenance]
}
```

**编译语义**:
- 如果设备声明了 `states: [...]`，使用自定义状态集替代默认状态
- 如果未声明，保持默认行为（cylinder = {extended, retracted}，其他 = {on, off}）
- 自定义状态可用于 safety 约束和 wait 条件

**验证影响**:
- Safety BMC: 状态域从 2 扩展到 N，状态空间增大但仍有限
- 需要评估对 BMC 收敛深度和验证时间的影响
- 建议限制单设备最大状态数（如 ≤ 8）

**验收标准**:
- `states: [a, b, c]` 编译通过
- `safety: device.a conflicts_with device.b` 使用自定义状态
- `wait: device == a` 使用自定义状态
- 未声明 states 时保持默认行为
- 状态数超过上限时报警告

---

### 13.4 实施优先级与依赖关系

```
P0（必须完成，阻塞后续迭代）:
  US-101 delay          ← 独立，可先做
  US-102 repeat         ← 独立，可先做
  US-104 timing 修复    ← 独立，可先做
  US-105 requires 验证  ← 独立，可先做
  US-103 wait AND/OR    ← 依赖 US-106（因果链修复后 AND/OR 的因果检查更准确）

P1（重要改进）:
  US-106 parallel 因果链 ← 独立，可先做
  US-107 if/else         ← 独立，可先做
  US-108 goto step       ← 独立，可先做

P2（锦上添花）:
  US-109 自定义状态      ← 独立，但需评估对 BMC 性能的影响
```

建议实施顺序: US-101 → US-102 → US-104 → US-105 → US-106 → US-103 → US-107 → US-108 → US-109

### 13.5 不在本迭代范围内

以下特性因形式验证可判定性约束，不纳入本迭代：

| 特性 | 排除原因 |
|------|---------|
| 无界循环 (while/loop) | 状态空间无限，Safety 验证不可判定 |
| 运行时整数变量/计数器 | 状态空间爆炸，BMC 无法收敛 |
| 度量时序逻辑 (MTL) | 需要 timed automata，完全不同的验证框架 |
| 概率验证 | 需要概率模型检查器 (PRISM) |
| 模拟量 I/O | 工作量大，需要 Z3 实数算术，留待 Phase 2.2 |
| PID 控制回路 | 需要混成自动机理论，可达性分析不可判定 |

详细分析见 `docs/dsl-defects-analysis.md`。

### 13.6 验收标准（整体）

1. 所有 P0 需求（US-101 ~ US-105）实现并通过测试
2. 现有 52 个测试全部通过（向后兼容）
3. 现有 7 个示例 .plc 文件全部验证通过
4. 为每个新特性至少新增 1 个示例 .plc 文件
5. plc-gen skill 更新以使用新语法特性
6. 错误报告保持中文，包含行号和修复建议
