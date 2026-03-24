# Axis Motion 异常处理采纳意见与 PRD 修订基线

日期：2026-03-06

## 1. 目标

基于《轴异常类型穷举与处理策略：面向可形式验证 PLC 的设计参考》，提炼可落地到 RustPLC 当前 DSL 的采纳项，明确 MVP 边界，并作为 `prd.json` 修订依据。

## 2. 关键判断

1. 第三方报告方向正确：异常处理不能只靠 `timeout`，必须区分拒绝类、运动故障类与安全类故障。
2. 第三方报告中 ADT/Result/match 方案适合语言理论层，但不宜直接照搬到当前 step DSL。
3. `op.motor.move_to` 保持禁用是正确决策：
   - 历史问题是把“运动目标”耦合为传感器目标，语义失真；
   - 该模板难以覆盖步进/伺服统一语义与完整异常分流；
   - 后续应由 `axis.move_*` 取代。

## 3. 采纳项（MVP 必做）

### 3.1 异常分层（采纳）

MVP 采用 4 类故障结果模型（避免一次性上 70+ 明细）：

- `timeout`：动作在限定时间内未完成
- `reject`：命令被前置条件或参数检查拒绝
- `motion_fault`：驱动/限位/跟随误差等运动层故障
- `safety_fault`：E-Stop/STO 等安全相关故障（可感知，不可由普通动作消解）

### 3.2 动作级显式分支（采纳）

`axis.move_relative` / `axis.move_absolute` 必须显式声明异常处理，不允许隐式默认去向。

建议 MVP 规则：

- 必填：`timeout: ... -> goto ...`
- 必填：`on_reject: goto ...`
- 必填：`on_motion_fault: goto ...`
- 必填：`on_safety_fault: goto ...`

`on_done` 可选：

- 不写时按现有 step 顺序流转到下一 step。
- 如需跨任务跳转可显式声明 `on_done: goto ...`。

### 3.3 编译期完备性检查（采纳）

新增 AXIS 规则建议：

- `AXIS-001`：`axis.move_*` 缺失 `timeout` 编译失败
- `AXIS-002`：`axis.move_*` 缺失 `on_reject` 编译失败
- `AXIS-003`：`axis.move_*` 缺失 `on_motion_fault` 编译失败
- `AXIS-004`：`axis.move_*` 缺失 `on_safety_fault` 编译失败
- `AXIS-005`：目标必须是轴设备（`stepper_motor`/`servo_drive`），禁止传感器/普通 motor

### 3.4 安全故障处理边界（采纳）

- `safety_fault` 必须进入明确安全处理任务（用户命名，不使用泛化名如 `fault_handler`）。
- 安全相关故障不允许在同一步做自动复位闭环（例如直接 `reset` 后继续生产）。

### 3.5 停机策略分级（采纳）

MVP 保留第三方报告中对停机方式的三分法语义，并在运行时动作语义中预留映射：

- `controlled_stop`
- `quick_stop`
- `immediate_disable`

对于垂直轴约束（先抱闸再断使能）列为 Phase-B 增强项。

## 4. 暂不采纳项（后续阶段）

1. 完整 ADT（几十个故障变体）与 DSL 内 `match` 穷举语法。
2. 全量 CiA402 操作模式映射。
3. 多轴传播矩阵自动推导（主从轴、插补组）的一次性落地。
4. 垂直轴制动顺序的编译器硬约束（Phase-B 再引入）。

## 5. 对 `op.motor.move_to` 的结论

- 继续保持禁用，不恢复。
- 禁用原因写入 Axis Motion 规范：
  - 旧模板将运动目标绑定到传感器语义，无法表达工程单位目标；
  - 异常处理能力不足（仅模板化等待，缺少分层故障语义）；
  - 与统一轴抽象方向冲突。

## 6. 建议语法草案（供 PRD 实施）

```plc
step arm_move_pick:
    action: axis.move_relative(arm_axis, 170deg, vel: 360deg_s, acc: 1200deg_s2)
    timeout: 800ms -> goto arm_motion_timeout
    on_reject: goto arm_command_rejected
    on_motion_fault: goto arm_motion_fault
    on_safety_fault: goto arm_safety_stop
```

说明：

- 先采用“step 内多行显式分支”保证可读性和可解析性；
- 若后续确定要一行式语法，可作为 parser 糖层，不改变语义规则。

## 7. 对 PRD 的直接影响

1. PRD 必须新增“异常分层模型 + 完备性编译检查”故事。
2. `axis.move_*` 语法故事必须从“仅 timeout”升级为“多故障分支必填”。
3. Runtime/Bridge/ST/Verification 故事均需覆盖 `reject/motion_fault/safety_fault` 语义，不仅覆盖 timeout。
4. `wafer_loader` 迁移故事需落地明确故障任务命名，不使用泛化故障任务名。

