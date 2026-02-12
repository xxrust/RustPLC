# RustPLC DSL 缺陷分析报告

**来源**: 6 轮三方角色迭代测试（工程师 × 智能体 × 程序员）
**日期**: 2026-02-12
**测试场景**: 钻孔工位、贴标工位、装配工位、打磨工位、冲压折弯产线、涂胶工位

---

## 1. 迭代中暴露的缺陷总表

| # | 缺陷 | 暴露场景 | 严重程度 | 当前 Workaround |
|---|------|---------|---------|----------------|
| D1 | 无延时原语 | 钻孔"传送带送走2秒"、打磨"低速转3秒" | 高 | 等待对立条件 + timeout 跳转 |
| D2 | 无循环原语 | 涂胶"来回涂3遍" | 高 | 手动展开为 N 份顺序步骤 |
| D3 | 无模拟量 I/O | 打磨"低速/高速"无法区分转速 | 高 | 只能 on/off，速度由外部变频器控制 |
| D4 | 设备只有 2 个状态 | 电机只有 on/off，无法表达档位 | 中 | 用多个 digital_output 模拟 |
| D5 | wait 只支持单条件 | 装配"两个零件都到位" | 中 | 拆成两个顺序 wait step |
| D6 | parallel 块因果链误报 | 装配工位两传送带同时启动 | 中 | 改用顺序 step 分别启动 |
| D7 | timing 关键路径计算不准 | 装配 8s 约束 vs 15.7s timeout 总和 | 中 | 去掉 timing 约束或放宽值 |
| D8 | 无条件分支 (if/else) | 打磨工位根据开关选模式 | 低 | 用 race 检测开关状态模拟 |
| D9 | goto 只能跳到 task 首步 | — | 低 | 拆分 task 使目标 step 成为首步 |
| D10 | safety 不检查 requires | requires 声明了但验证器跳过 | 低 | 依赖顺序控制隐式保证 |

---

## 2. 缺陷详细分析

### D1: 无延时原语

**问题**: DSL 没有 `delay` / `sleep` / `timer` 原语。工程师经常需要"延时N秒"（如传送带送走工件、涂胶保压、电机运转固定时间）。

**当前 Workaround**:
```plc
# 方案1: 等待物理传感器状态变化（优选）
step wait_leave:
    wait: sensor_arrive == false    # 工件离开传感器
    timeout: 3000ms -> goto fault_handler

# 方案2: 等待对立条件 + timeout 模拟延时
step wait_coarse:
    wait: switch_B == true          # 选择开关在A位时B一定为false
    timeout: 3000ms -> goto stop_grind  # 3秒后跳转 = 延时3秒
```

**问题所在**: 方案1 依赖有传感器可用；方案2 语义不清晰，需要找一个"已知为 false"的条件来等待，可读性差。

**影响范围**: 所有涉及定时动作的场景（传送带送料、电机定时运转、保压、冷却等待）。

---

### D2: 无循环原语

**问题**: DSL 没有 `for` / `while` / `repeat` 循环。工程师说"涂3遍"、"冲压2次"时，必须手动展开。

**当前 Workaround**:
```plc
# "涂胶3遍" → 展开为6个步骤
step glue_1_out:
    action: extend cyl_glue
    wait: sensor_glue_ext == true
    timeout: 300ms -> goto fault_handler
step glue_1_back:
    action: retract cyl_glue
    wait: sensor_glue_ret == true
    timeout: 300ms -> goto fault_handler
step glue_2_out:
    ...（重复）
step glue_2_back:
    ...
step glue_3_out:
    ...
step glue_3_back:
    ...
```

**问题所在**: 代码膨胀，3遍涂胶需要6个步骤，10遍就是20个步骤。修改涂胶逻辑时需要改N处。

---

### D3: 无模拟量 I/O

**问题**: 所有设备都是二值的（on/off、extended/retracted）。无法表达：
- 模拟量传感器（压力、温度、位置反馈）
- 变速电机（PWM/DAC 控制）
- 比例阀（开度控制）
- PID 控制回路

**当前 Workaround**: 电机速度由外部变频器控制，PLC 只管 on/off。压力、温度等由独立仪表监控。

**影响范围**: 所有需要连续量控制的场景（调速、调压、温控、位置伺服）。

---

### D4: 设备只有 2 个状态

**问题**: `default_states_for_kind()` 硬编码：气缸 = {extended, retracted}，其他 = {on, off}。无法表达：
- 三位电磁阀（伸出/中位/缩回）
- 多档位开关
- 步进电机离散位置

**源码位置**: `src/semantic/mod.rs:796-804`

---

### D5: wait 只支持单条件

**问题**: `wait: sensor == true` 只能等待一个条件。无法表达 `wait: sensor_A == true AND sensor_B == true`。

**当前 Workaround**: 拆成两个顺序 step 分别等待。但这改变了语义——顺序等待意味着"A先到位，然后B到位"，而非"A和B同时到位"。

**语法层面**: pest 文法中 `wait_statement` 只接受单个 `condition_operand comparison_operator condition_value`。

---

### D6: parallel 块因果链误报

**问题**: 编译器把 parallel 块中所有分支的 action 与 step 级别的 wait 做因果检查。不同设备的分支被错误关联。

**复现**:
```plc
step start_both:
    parallel:
        branch_left:
            action: set motor_left on     # 操作 motor_left
        branch_right:
            action: set motor_right on    # 操作 motor_right
    wait: sensor_left_arrive == true      # 只等 motor_left 的传感器
    # → 编译器检查 motor_right -> sensor_left_arrive，报因果链断裂
```

**源码位置**: `src/verification/causality.rs` — 因果链推断逻辑未区分 parallel 分支归属。

---

### D7: timing 关键路径计算不准

**问题**: task 级别的 `must_complete_within` 把每个 step 的 `max(action_time, timeout)` 累加。由于 timeout 值（安全裕量）通常远大于实际动作时间，导致计算出的"最坏路径"严重偏大。

**复现**: 装配工位 12 个 step，每个 timeout 500~5000ms，累加得 15700ms，远超工程师要求的 8000ms。但实际动作时间只有约 2320ms。

**源码位置**: `src/verification/timing.rs:284-302` — `build_task_worst_case` 直接累加所有 step 的 worst_case。

---

### D8: 无条件分支

**问题**: 没有 `if/else` 语法。根据设备状态选择不同路径只能用 `race` 模拟。

**当前 Workaround**:
```plc
# 用 race 模拟 if/else
step check_mode:
    race:
        branch_coarse:
            wait: switch_A == true
            then: goto grind_coarse
        branch_fine:
            wait: switch_B == true
            then: goto grind_fine
    timeout: 1000ms -> goto fault_handler
```

**问题所在**: race 的语义是"竞争等待"，用来做"检测已知状态"语义不匹配。且需要额外的 timeout 保护（万一两个开关都没信号）。

---

### D9: goto 只能跳到 task 首步

**源码位置**: `src/semantic/mod.rs` — `resolve_task_target()` 只返回 task 的第一个 step。

**影响**: 无法从 fault_handler 恢复到 task 中间的某个步骤，必须从头开始。

---

### D10: safety 不检查 requires

**问题**: `safety: A requires B` 声明了依赖关系，但 Safety 验证器只检查 `conflicts_with`，跳过 `requires`。

**源码位置**: `src/verification/safety.rs:149` — `if !matches!(rule.relation, SafetyRelation::ConflictsWith) { continue; }`

**影响**: `requires` 约束目前只是文档性质的声明，编译器不做验证。如果控制逻辑违反了 requires（如压装时夹紧缸未伸出），编译器不会报错。

---

## 3. 可修改 vs 不可修改分类

### 3.1 可修改（不破坏形式验证可判定性）

| 缺陷 | 修改方案 | 工作量 | 对验证引擎的影响 |
|------|---------|--------|----------------|
| D1 delay | 添加 `delay: Nms` 语法，编译为带固定时间守卫的转移 | 小 | 无，语法糖 |
| D2 repeat | 添加 `repeat N:` 块，编译期展开为 N 份步骤 | 小 | 无，编译期展开 |
| D5 AND/OR | 扩展 wait 支持 `AND` / `OR` 组合条件 | 中 | Causality 需对每个子条件分别检查 |
| D6 parallel 因果链 | 因果链推断时区分 parallel 分支归属 | 中 | 修复 Causality 检查准确性 |
| D7 timing 计算 | 区分正常路径（action 时间）和最坏路径（timeout 值） | 中 | 改善 Timing 检查准确性 |
| D8 if/else | 添加 `if condition: goto X else: goto Y` 语法 | 中 | Safety BMC 需处理条件分支 |
| D9 goto step | 扩展 goto 支持 `task.step` 格式 | 小 | 无，状态机已用 (task, step) 标识 |
| D10 requires 验证 | Safety 检查器增加 requires 约束的验证逻辑 | 中 | BMC 增加 requires 不变式检查 |
| D4 自定义状态 | 设备声明中允许 `states: [...]` | 中 | BMC 状态空间增大但仍有限 |
| D3 模拟量（基础） | 添加 analog_input/output 类型 + 数值比较 wait | 大 | 需要 Z3 实数算术支持 |

### 3.2 不可修改（受形式验证可判定性约束）

| 特性 | 不可行原因 |
|------|-----------|
| 无界循环 (while/loop) | 状态空间无限，BMC k-induction 无法收敛，Safety 验证变为不可判定（等价于停机问题） |
| 运行时整数变量/计数器 | 状态空间从 \|tasks\|×\|steps\| 爆炸为 \|tasks\|×\|steps\|×\|int_domain\|，BMC 深度和时间急剧增长 |
| 度量时序逻辑 (MTL/TCTL) | "A发生后B必须在100ms内发生"类约束的模型检查是 PSPACE-complete 到 undecidable，需要 timed automata + UPPAAL 等完全不同的验证框架 |
| 概率验证 | "碰撞概率<0.1%"需要马尔可夫决策过程和 PRISM 等概率模型检查器，当前确定性状态机无法表达 |
| 组合验证 | 两个独立安全的子系统组合后不一定安全（状态空间是笛卡尔积），组合验证是形式验证领域的开放问题 |
| PID 控制回路 | 连续控制系统的验证需要混成自动机（hybrid automata）理论，可达性分析一般不可判定 |

### 3.3 判断边界说明

**有界小范围计数器**（如 0..15）理论上可以通过状态展开实现，但本质上等价于 `repeat N` 的编译期展开，不是真正的运行时变量。如果限制范围足够小（如 4 bit），状态空间增长可控（×16），BMC 仍可收敛。但这需要仔细评估对验证时间的影响。

**模拟量**的可行性取决于精度要求。如果只需要阈值比较（`pressure >= 50bar`），可以离散化为有限个区间，状态空间仍有限。如果需要精确数值推理，则需要 Z3 的实数算术理论，验证时间可能显著增长。

---

## 4. 迭代中发现的编译器 Bug / 改进点

| # | 类型 | 描述 | 位置 |
|---|------|------|------|
| B1 | Bug | parallel 块因果链检查不区分分支归属 | `src/verification/causality.rs` |
| B2 | 改进 | timing 关键路径应区分 action 时间和 timeout 值 | `src/verification/timing.rs:284-302` |
| B3 | 缺失 | safety 验证器不检查 requires 约束 | `src/verification/safety.rs:149` |
| B4 | 改进 | 设备状态硬编码，应支持从 AST 读取 | `src/semantic/mod.rs:796-804` |
| B5 | 改进 | 连接类型硬编码，应支持扩展 | `src/semantic/mod.rs:1271-1280` |

---

## 5. plc-gen Skill 迭代改进记录

六轮迭代中对 `.claude/skills/plc-gen/SKILL.md` 的改进：

| 轮次 | 发现的问题 | 修改内容 |
|------|-----------|---------|
| 1 | 安全约束遗漏、缩回传感器因果链缺失、fault_handler 缺安全提示 | 添加约束转化检查清单、因果链完整性要求、fault_handler log 提示、延时替代方案 |
| 2 | 电机状态不明确、示例不足 | 补充设备状态列表（含 motor 的 on/off）、引用 examples 目录 |
| 3 | conflicts_with vs requires 混淆、parallel 因果链陷阱、timing 计算逻辑不清 | 添加判断指引表、parallel 陷阱警告 + 替代方案、timing 计算说明 |
| 4 | 延时模拟技巧（对立条件等待） | 更新延时方案优先级，增加"等待对立条件"方案 |
| 5 | 无新缺陷（多工位串联一次通过） | 仅更新 examples 引用 |
| 6 | 循环展开模式 | 添加循环展开指导和命名规范 |

---

## 6. 测试场景覆盖矩阵

| 示例文件 | 设备类型 | 控制模式 | 约束类型 | 验证结果 |
|---------|---------|---------|---------|---------|
| `drill_station.plc` | motor + cylinder | 顺序 + 传感器 false 等待 | requires | 通过 |
| `label_station.plc` | motor + cylinder | 顺序 | conflicts_with(cyl vs motor) | 通过 |
| `assembly_station.plc` | 2×motor + 4×cylinder | 顺序（替代 parallel） | requires + conflicts_with | 通过（修复后） |
| `grind_station.plc` | motor + cylinder + switch | race 模式选择 + timeout 延时 | requires | 通过 |
| `stamp_bend_line.plc` | motor + 4×cylinder | 多 task 串联 | 4×conflicts_with + 2×requires | 通过 |
| `glue_station.plc` | motor + 2×cylinder | 循环展开（3遍） | requires + conflicts_with | 通过 |
