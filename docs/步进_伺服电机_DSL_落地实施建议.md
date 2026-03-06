# 步进/伺服电机 DSL 落地实施建议（基于深度分析报告）

日期：2026-03-05

## 1. 结论（是否更适合、是否可实施）

结论：**方向更适合，且可实施**，但必须按“工业可落地 MVP -> 渐进增强”推进，不能一次性把完整对象模型全部塞进当前 DSL。

- 适合保留的核心思想：
  1. 统一轴语义（步进/伺服上层动作一致）
  2. 位置运动动作（`move_absolute/move_relative`）替代 `run + delay`
  3. 显式 timeout + 显式异常分流
  4. 开环/闭环语义显式化（避免“实际位置”误解）
- 需要收敛的部分：
  1. 报告中的 `Axis` 继承体系与 `Command` 对象返回，更像运行时 API，不适合直接照搬到当前声明式 DSL
  2. 完整 CiA402 模式与多轴高级同步应后置，不作为第一阶段门槛

## 2. 与当前代码基线的差距（事实）

1. 已有 `stepper_motor` 设备类型与基础参数白名单，但未覆盖报告建议的完整轴参数模型。
   - 参考：`src/parser/plc.pest:20`、`src/parser/plc.pest:77`
2. 当前 DSL action 没有“轴运动命令”，仅有 `set/extend/retract/op.*` 等。
   - 参考：`src/parser/plc.pest:248`
3. `op.motor.move_to` 当前被语义层禁用，不能作为运动抽象入口。
   - 参考：`src/semantic/mod.rs:1109`
4. `stepper_motor` 的隐式端口是 `enable/direction/pulse/fault`，与普通 `motor.run` 语义不同。
   - 参考：`src/semantic/mod.rs:430`
5. 运行时动作模型没有原生“轴运动”动作，若直接新增需跨层联动改造。
   - 参考：`src/ir/mod.rs:230`、`src/runtime_bridge.rs:1268`、`crates/runtime-core/src/lib.rs:118`

## 3. 落地策略（先做对，再做全）

### 3.1 总体策略

采用三阶段：

- **Phase A（MVP）**：先让 DSL 能写“轴运动意图 + 严格超时 + 错误去向”，并可编译、可测试、可回归。
- **Phase B（工业增强）**：补齐回零、停止、错误码、开环/闭环语义。
- **Phase C（高级功能）**：再做同步轴、齿轮/凸轮、多轴协同。

### 3.2 Phase A（MVP，建议 2~3 个迭代）

#### A1. 拓扑参数增强（不改运行时行为）

目标：补齐轴基础参数，先完成“配置可表达、可校验”。

建议新增参数（stepper/servo 共享子集）：

- `microstep`（步进细分）
- `gear_num` / `gear_den`（传动比）
- `lead_screw`（线性轴导程，可选）
- `position_unit`（`deg`/`mm`）
- `max_acceleration`（与 `max_speed` 配套）

改动点：

- 语法白名单：`src/parser/plc.pest`
- 设备库参数定义：`devices/stepper_motor.toml`、`devices/servo_drive.toml`（如存在）
- 预处理/类型校验：`src/semantic/mod.rs`

验收：

- 非法参数名/类型能报错
- 合法参数可通过编译

#### A2. 新增“轴运动动作”语法（强制 inline timeout）

目标：在 step 中直接表达运动意图。

建议最小语法：

```plc
action: axis.move_relative(arm_axis, 170deg, vel: 360deg_s, acc: 1200deg_s2) timeout: 800ms -> goto arm_motion_timeout
action: axis.move_absolute(arm_axis, 0deg, vel: 240deg_s, acc: 800deg_s2) timeout: 1000ms -> goto arm_motion_timeout
```

关键规则：

- `axis.move_*` 必须显式 timeout（无默认超时）
- timeout 必须同 action 同行（与现有 `op.*` 新规一致）
- axis 参数必须引用 `stepper_motor`/`servo_drive` 等合法轴设备

改动点：

- 语法解析：`src/parser/plc.pest`、`src/parser/mod.rs`
- AST：`src/ast/mod.rs`
- 语义降级：`src/semantic/mod.rs`

#### A3. 运行时落地路径（两选一，推荐方案 1）

方案 1（推荐，最稳）：新增 `TransitionAction` 轴动作并全链路打通。

- IR：`src/ir/mod.rs`
- 语义降级：`src/semantic/mod.rs`
- Runtime Bridge：`src/runtime_bridge.rs`
- Runtime Core：`crates/runtime-core/src/lib.rs`
- ST 代码生成：`src/codegen/st.rs`
- 安全/诊断匹配：`src/verification/safety.rs`

> 备注：该联动与仓库 AGENTS 约束一致，避免“只改语法不改执行”的半成品。

方案 2（过渡）：语义展开为 `call extern` + `wait` 组合，先跑通功能，再演进为原生 action。

### 3.3 Phase B（工业增强）

#### B1. 轴状态与错误语义

引入最小状态集合：`disabled / standstill / moving / homing / error_stop`。

- `move_*` 前置：非 `error_stop`
- `error_stop` 仅允许 `reset`
- 错误码统一映射到可观测变量（便于 DSL 分支）

#### B2. 开环/闭环语义明确化

- `encoder.type = none` 时：`actual_position` 定义为“估算/指令位置”
- 有反馈时：`actual_position` 定义为“反馈位置”
- 文档和诊断文案必须区分两者

#### B3. 基础回零动作

先支持 3 种高频模式：`active / absolute / set_position`，其余模式后续扩展。

### 3.4 Phase C（高级功能，后置）

- `SynchronousAxis`、`gear_in/out`、`cam_in/out`
- blending 连续轨迹
- 更完整 CiA402 operation mode 映射

## 4. 建议里程碑与验收标准

### Milestone 1（参数 + 语法）

- 能声明完整 stepper/servo 轴参数
- 能解析 `axis.move_relative/absolute ... timeout -> goto ...`
- 缺失 timeout 时报错（与 op 规则一致）

### Milestone 2（可执行）

- 运动命令可进入运行时并产生可追踪动作
- 超时跳转、错误跳转路径可复现
- `wafer_loader.plc` 中旋臂动作可替换掉 `run + delay + off`

### Milestone 3（验证闭环）

- 安全验证能识别轴错误状态与互锁冲突
- 诊断日志可区分：超时、命令拒绝、驱动错误
- 回归示例覆盖步进开环和伺服闭环各 1 套

## 5. 风险与控制

1. **一次性大改风险**：完整对象模型跨度过大。
   - 控制：先做 MVP 动作语义，再逐步补齐状态机。
2. **语义与执行脱节风险**：只加语法不加 runtime。
   - 控制：每个里程碑必须包含端到端测试。
3. **单位系统歧义风险**：`deg/mm` 与脉冲换算不一致。
   - 控制：统一单位声明与换算路径，编译期校验。

## 6. 推荐的第一步（本周可执行）

1. 完成 A1（参数增强）+ A2（语法定义）评审稿。
2. 先实现 `axis.move_relative` 单一动作链路（不同时做全部命令）。
3. 以 `docs/wafer_loader.plc` 的旋臂段作为首个真实迁移样例。

