# 操作者边界与 Front-Door 契约

## 1. 定位

本文定义 RustPLC 中“人参与控制闭环”的建模边界。

核心裁决：

- 操作者不是普通 `device`，不进入设备语义库。
- 按钮、选择开关、HMI 命令是操作者进入 PLC 的输入接口。
- 指示灯、蜂鸣器、HMI 状态、报警文本是 PLC 返回给操作者的输出接口。
- 操作者闭环属于 I/O 边界和操作契约层，而不是机械设备拓扑闭环。

因此下面这种关系仍然是合法且必要的底层 I/O 映射：

```plc
relation { from: start_button.out, to: plc_main.X0, via: reports_to }
```

复杂项目可先在 `controller_io` 中为 `X0` 定义业务别名，再在关系中使用该别名：

```plc
controller_io plc_main {
    input start_cycle_cmd: X0 { purpose: "启动按钮输入" }
}

relation { from: start_button.out, to: plc_main.start_cycle_cmd, via: reports_to }
```

但它只说明“按钮电信号报告给 PLC”。它没有说明这个按钮代表谁的意图、什么状态下可用、如何触发、是否需要反馈、以及异常状态下是否允许继续影响自动流程。

这些缺失语义由 `operator_boundary` / `operator_panel` front-door 契约承载。

## 2. 不把人建成设备的原因

设备拓扑解决的是确定性物理连接：

- 哪个设备接到哪个控制器端口
- 哪个执行器由哪个输出驱动
- 哪个传感器报告哪个反馈
- 哪个设备动作可以通过拓扑闭合成可判定结果

操作者不是可由 runtime 合成反馈的设备，也不是可由 verification 枚举状态机内部状态的执行器。

把人建成 `device operator` 会带来三个错误：

1. 让设备库承担行为、权限、确认、误操作等上层语义。
2. 让拓扑闭环假装可以验证人的真实反应。
3. 让按钮这种输入接口看起来必须存在反向物理反馈，从而扭曲 I/O 模型。

正确模型是：

```text
operator intent
    -> button / selector / hmi_command
    -> plc input / internal command
    -> task / mode / fault policy
    -> hmi_status / lamp / buzzer / alarm_message
    -> operator decision
```

这是系统边界闭环，不是设备闭环。

## 3. 语义层级

推荐分层：

| 层 | 职责 | 示例 |
|---|---|---|
| `device` / `relation` | 物理 I/O 与拓扑映射 | `start_button.out -> plc_main.start_cycle_cmd` |
| `operator_boundary` | 操作者、权限、命令入口、反馈义务 | `operator main_operator` |
| `operator_panel` | 一组按钮、选择开关、HMI 命令和指示输出 | `main_panel` |
| `front-door contract` | 命令触发方式、允许状态、确认/反馈要求 | `start_cycle` 只能在 `ready.wait_start` 生效 |
| `task` | 消费已经建模的命令语义 | `wait: start_button == true` 或后续 `wait command start_cycle` |
| `verification` | 验证命令可达性、禁止状态、反馈覆盖、人工确认闭环 | 故障后必须给出可见报警并等待复位 |

## 4. DSL 方向

第一阶段不要求立即替换已有 `wait: start_button == true`。

推荐新增可向后兼容的声明语义：

```plc
operator main_operator {
    role: operator
}

operator_panel main_panel {
    actor: main_operator

    command start_cycle {
        source: start_button.out
        trigger: rising_edge
        debounce: 20ms
        allowed_when: ready.wait_start
        rejects_when: [fault_alarm.message, manual_mode.active]
        requires_feedback: cycle_started_lamp.coil
    }

    command reset_fault {
        source: reset_button.out
        trigger: rising_edge
        allowed_when: fault_alarm.message
        requires_feedback: alarm_lamp.coil
    }

    indication alarm_visible {
        source: alarm_lamp.coil
        required_for: reset_fault
    }
}
```

这里的 `source` 可以指向按钮输出、选择开关输出、HMI 命令变量或控制器输入端口。

保留现有底层关系：

```plc
device start_button: sensor {
    purpose: "启动按钮"
    subtype: "push_button"
    debounce: 20ms
}

relation { from: start_button.out, to: plc_main.X0, via: reports_to }
```

如果项目已声明 `controller_io`，这里应优先写 `plc_main.start_cycle_cmd` 这类业务别名，物理点位仍由 preprocess 降级到 `X0`。

`operator_panel.command` 只解释该输入的操作语义，不替代电气拓扑。

## 5. 默认值

为了不让普通项目写很多样板，front-door 契约应有保守默认值：

| 字段 | 默认值 | 说明 |
|---|---|---|
| `actor` | `operator` | 如果项目未声明具体操作者，使用默认操作者角色 |
| `trigger` | `rising_edge` | 按钮类输入默认按上升沿处理 |
| `debounce` | 从设备 `debounce` 继承，否则 `20ms` | 避免重复触发 |
| `allowed_when` | 必须显式声明，或由 system contract 标记 blocker | 启动、复位、手动动作不应默认全局可用 |
| `reject_policy` | `ignore_with_diagnostic` | 禁止状态下触发应被诊断记录，不推进任务 |
| `requires_feedback` | 对 `start/reset/mode_change` 类命令必须显式声明或说明免除原因 | 人机闭环必须可见 |
| `ack_timeout` | `0ms` | 默认不要求操作者在时间内响应；若要求确认必须显式声明 |

`allowed_when` 不建议默认放开。人的输入是外部非确定事件，默认全局可用会扩大 verification 状态空间，也会掩盖误操作风险。

## 6. IR 形态

长期 IR 应新增独立结构，而不是把字段挂到 `Device` 上：

```text
TopologyGraph {
  operator_boundaries: Vec<OperatorBoundaryDef>
}

OperatorBoundaryDef {
  operators: Vec<OperatorDef>
  panels: Vec<OperatorPanelDef>
  commands: Vec<OperatorCommandDef>
  indications: Vec<OperatorIndicationDef>
}

OperatorCommandDef {
  name
  actor
  source_ref
  trigger
  debounce_ms
  allowed_when: Vec<StateRef>
  rejects_when: Vec<StateRef>
  requires_feedback: Vec<SignalRef>
  reject_policy
}
```

状态引用必须指向 IR 中存在的 `task.step` 或后续模式状态；信号引用必须能通过拓扑关系解析到真实设备端口或内部变量。

## 7. Semantic 门禁

语义阶段至少检查：

1. `command.source` 必须能解析到已声明端口、设备输出或控制器输入。
2. 按钮/选择开关/HMI 命令不得被声明为执行器设备。
3. `allowed_when` 指向的 task/step 必须存在。
4. `requires_feedback` 指向的反馈输出必须存在，且方向必须是 PLC 到人可见界面。
5. `reset_fault`、`manual_ack`、`mode_change` 等命令必须有反馈或显式豁免。
6. 同一个物理按钮可映射多个命令，但这些命令的 `allowed_when` 不得重叠，除非声明优先级。
7. 禁止状态下的命令不能静默推进任务，只能按 `reject_policy` 记录、忽略或进入显式 fault route。

## 8. Verification 语义

verification 不证明“人一定会按按钮”，只证明：

- 如果命令发生，它只能在允许状态下产生语义效果。
- 命令被拒绝时不会破坏安全状态。
- 需要人工确认的 fault/reset 路径都有可见反馈。
- 启动、复位、手动点动、模式切换不会绕过 safety/liveness 约束。
- 操作者输入作为外部事件进入可达状态空间时，不会造成跨 task 资源冲突。

对 liveness 来说，等待人工命令的 step 默认可以是 indefinite wait，但必须标注为 operator-driven wait，而不是被误判为系统死锁。

对 causality 来说，`operator_command -> task transition -> indication` 是一条可验证链路。缺少 indication 时，不能默认认为人已获得反馈。

## 9. Runtime 与 Scenario

runtime 不模拟人的判断，只消费输入事件和已编译的 front-door 门禁：

- 合法命令：转换为内部 command event 或允许对应输入触发 wait。
- 禁止命令：按 `reject_policy` 记录诊断，不推进任务。
- 需要边沿触发的命令：runtime/scenario 层必须做 edge/debounce 处理。

scenario 应区分来源：

```yaml
inputs:
  - at_ms: 0
    actor: operator
    source: main_panel.start_cycle
    set:
      digital_inputs:
        0: true
  - at_ms: 30
    actor: operator
    source: main_panel.start_cycle
    set:
      digital_inputs:
        0: false
```

旧场景可以继续使用裸 `digital_inputs`，但复杂项目生成时应优先写出 `actor/source`，便于审计和 intent alignment。

## 10. Codegen 策略

ST/codegen 不需要生成“人”的对象。

它应生成或保留：

- 输入边沿检测
- debounce
- mode/fault 状态下的命令屏蔽
- HMI/报警/指示输出映射
- 非法命令诊断位或日志

如果目标后端不支持 front-door 契约中的某项能力，应显式拒绝，例如：

- 不支持 debounce
- 不支持 HMI command source
- 不支持 command reject diagnostic
- 不支持 feedback obligation

不能把 front-door 契约静默降级成裸输入触点。

## 11. 迁移路径

### Phase 1: 文档与生成约束

- 在 system contract 中显式列出 operator command、allowed state、feedback obligation。
- `plc-gen` 继续生成现有 `device sensor + relation + wait`，但必须在文档和 HMI/手动占位中记录 front-door 契约。
- scenario 对操作者输入添加 `actor/source`。

### Phase 2: Parser / AST / IR

- 增加 `operator`、`operator_panel`、`command`、`indication` 声明。
- IR 新增 `operator_boundaries`，不放入 `DeviceKind`。

### Phase 3: Semantic / Verification

- 校验命令来源、状态门禁、反馈义务、重叠 allowed_when。
- liveness 识别 operator-driven indefinite wait。
- causality 检查 command 到 indication 的链路。

### Phase 4: Runtime / Codegen

- runtime bridge 降级 command gate。
- runtime 记录非法命令诊断。
- ST 生成 edge/debounce/gate/feedback 或显式拒绝。

## 12. 对当前项目的裁决

对于：

```plc
relation { from: start_button.out, to: plc_main.X0, via: reports_to }
```

不应补一个 `relation { from: plc_main, to: start_button }`。

如果要提升可读性，应补的是 `controller_io` 别名，例如 `plc_main.start_cycle_cmd -> X0`，而不是把操作者建成设备。

应补的是 front-door 契约：

```text
operator main_operator
command start_cycle:
  source = start_button.out
  trigger = rising_edge
  allowed_when = ready.wait_start
  feedback = cycle started / running / alarm visible
```

这样既保留拓扑模型的物理真实性，也把人的输入纳入编译期验证和运行期审计。
