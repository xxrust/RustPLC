# RustPLC 步进轴动作抽象报告（Step/Dir 场景）

日期：2026-03-03

## 1. 背景与问题

当前在 `step` 里使用如下写法控制旋臂步进轴：

- `action: set motor_arm.run on`
- `delay: xxxms`
- `action: set motor_arm.run off`

该模式存在三个工业风险：

1. **语义不完整**：只表达“通电时长”，未表达“目标位置/角度”。
2. **可维护性弱**：换电机细分、减速比后，`delay` 需要整线重调。
3. **故障诊断差**：无法统一区分“未到位”“跟随误差”“驱动报警”等运动异常。

## 2. 专业控制语言的共性做法

工业运动控制（PLCopen MC、TwinCAT、CODESYS、Rockwell、LinuxCNC）普遍采用“轴参数 + 运动指令”的两层模型：

- **轴配置层（静态）**：步距角、细分、传动比、单位换算、速度/加速度上限。
- **指令层（动态）**：`MoveAbsolute/MoveRelative`，并统一 `Done/Busy/Error` 结果。
- **顺控层（任务）**：等待 `Done`，对 `Error` 与超时做显式分流。

这比“run + delay”更接近工业真实控制链路，也更利于审计和验证。

## 3. 对 RustPLC 的抽象建议

### 3.1 设备定义（拓扑层）

`stepper_motor` 设备应承载静态参数（建议至少包含）：

- `steps_per_rev`（整步脉冲/转）
- `microstep`（细分）
- `gear_num/gear_den`（传动比）
- `max_speed`、`accel_time`、`decel_time`
- `unit`（建议统一工程单位：`deg` 或 `mm`）

### 3.2 Step 动作（任务层）

建议将步进轴动作抽象为“目标驱动语义”：

- `move_relative(axis, distance, vel, acc)`
- `move_absolute(axis, position, vel, acc)`

并强制配套：

- 显式 `timeout`
- 显式异常跳转（不可隐式默认）

### 3.3 运行时语义（执行层）

- `Busy=true` 时禁止同轴再次下发运动命令。
- 指令完成信号统一为 `Done`，驱动报警统一为 `Error + ErrorID`。
- 编译器仅负责单位换算与静态检查；高频脉冲生成留在驱动/板级层。

## 4. 建议示例（目标语法草案）

> 说明：以下是建议的高层语法示例，用于表达目标方向。

```plc
[topology]
device arm_axis: stepper_motor {
    purpose: "旋臂步进轴",
    steps_per_rev: 200,
    microstep: 16,
    max_speed: 720deg_s,
    accel_time: 80ms,
    decel_time: 80ms
}

device arm_inpos: sensor { purpose: "旋臂到位反馈" }
device arm_fault: sensor { purpose: "驱动器故障反馈" }

[constraints]
safety: arm_fault.on conflicts_with arm_axis.enable.on

[tasks]
task cycle:
    step arm_move_pick:
        action: stepper.move_relative(arm_axis, 170deg, vel: 360deg_s, acc: 1200deg_s2) timeout: 800ms -> goto arm_motion_timeout

    step arm_wait_inpos:
        wait: arm_inpos == true
        timeout: 200ms -> goto arm_motion_timeout

    on_complete: goto cycle

task arm_motion_timeout:
    step stop_axis:
        action: set arm_axis.enable off
    step report:
        action: log "旋臂运动超时，请检查步进驱动器与机械负载"
    on_complete: goto ready
```

## 5. 实施建议（分阶段）

1. **阶段 A（立即可做）**：在现有 DSL 中禁止 `stepper_motor` 采用纯 `run+delay` 作为定位动作模板。
2. **阶段 B（语义增强）**：增加 `stepper.move_relative/absolute` 动作与强制 timeout 语法检查。
3. **阶段 C（运行时对齐）**：桥接 `Busy/Done/ErrorID`，补齐故障码与追溯日志。

---

参考（主规范/主厂商文档）：

- PLCopen Motion Control: <https://www.plcopen.org/standards/motion-control/>
- Beckhoff MC_MoveAbsolute: <https://infosys.beckhoff.com/content/1033/tcplclibmc2/458411147.html>
- Schneider MC_MoveAbsolute: <https://product-help.schneider-electric.com/Machine%20Expert/V2.2/en/PLCO/PLCO/D-SE-0086558.html>
- Rockwell MC_MoveAbsolute: <https://www.rockwellautomation.com/en-ie/docs/factorytalk-design-workbench/1-00-00/ftdw-help-ditamap/micro800-controller/micro800-instruction-set/motion-move-instructions/mc_moveabsolute.html>
- LinuxCNC Stepconf: <https://www.linuxcnc.org/docs/2.6/html/config/stepconf.html>
