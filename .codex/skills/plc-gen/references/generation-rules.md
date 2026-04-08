# plc-gen Generation Rules

## Mandatory example: topology-closed cylinder actions

Do not generate this for a topology-closed cylinder:

```plc
step feed_forward:
    action: extend cyl_feed
    wait: sensor_feed_ext == true
    timeout: 800ms -> goto feed_warning.feed_cyl_warn
```

Do not generate this either:

```plc
step orient_home:
    action: retract cyl_orient_rotate
    wait: sensor_orient_ret == true
```

Generate this shape instead:

```plc
step feed_forward:
    action: extend cyl_feed
        timeout: 800ms -> goto feed_warning.feed_cyl_warn
```

```plc
step orient_home:
    action: retract cyl_orient_rotate
        timeout: 600ms -> goto orient_warning.orient_cyl_warn
```

Interpretation rule:
- If `sensor_*` is already connected from `cyl_x.extended` or `cyl_x.retracted` through `via: detects`, that feedback belongs to the device semantics layer.
- The generated task may route timeout or explicit fault buckets, but it must not restate the normal confirmation loop as `wait: sensor_* == true`.
- If the generated lowering still seems to require hand-written sensor waits for the normal endpoint, stop and report a blocker instead of downgrading the actuator semantics.

## Source-set structure rule

For complex projects, prefer a structured fragment layout over a monolithic PLC file.

Use semantic domains as the split boundary:
- `topology/`
- `constraints/`
- `architecture/`
- `auto/`
- `maintenance/`
- `manual/`
- `operator_interface/`

Reference example:
- `out/skill_flywheel/plc_gen_wafer_loader/plc/target_semantics_fragments`

Do not split by arbitrary line count or by temporary implementation convenience.
Do split by stable ownership and semantic responsibility so the project is suitable for parallel implementation and later review.

## Workpiece rule for physical part flow

If the system contract describes a real physical part moving through the station, do not leave that flow implicit in sensors and actuators only.

Treat the following as a mandatory signal that first-class workpiece modeling is required:
- one station picks a part from another location
- a holder or nozzle temporarily owns the part
- the part is handed to the next machine
- the part can be rejected, unloaded, scrapped, or otherwise reach an explicit terminal outcome

Minimum delivery requirements:
- declare a `workpiece_type`
- declare the participating `workpiece_location` / `workpiece_holder` / `workpiece_carrier`
- include the workpiece fragment in the main compileable bundle if the automatic flow depends on it
- place `effect: acquire`, `effect: transfer`, and `effect: finish` on the real task steps that change ownership or terminal status

Do not stop at a placeholder like:

```plc
# Sidecar workpiece-contract area.
```

That may still compile today, but it is structurally under-modeled because the compiler only activates workpiece semantic and safety checks when workpiece declarations or effects are actually present.

## Intent-alignment anchor rule for concurrent or pipelined flows

For a structured project that overlaps preparation and transfer tasks, do not bind a business milestone to a low-level prep transition that can repeat while the current part is still in flight.

Prefer milestone anchors that uniquely identify ownership handoff of the active workpiece, for example:
- `effect: acquire ...` confirmation that picks the active part from a shared site
- `effect: transfer ...` confirmation that moves the active part onto the next station
- `effect: finish ...` confirmation that closes the active part at its declared terminal site

Do not use a repeating prefetch or housekeeping transition as the only evidence for a required milestone when that step may fire again before the cycle completes.

If a milestone keeps comparing as `duplicated_required_step` in a real canary trace, first check whether the contract chose the wrong evidence anchor before weakening the comparator.

本文件记录生成 `.plc` 时不能偏离的硬约束。

## 1. task / step 基本约束

输出至少要满足：
- 至少一个 `task`
- 每个 task 至少一个 `step`
- task 名称唯一
- `goto` / `timeout` / `on_complete` 指向真实存在的目标

不要生成“逻辑上像流程，但结构上不闭合”的 DSL。

## 2. 并发与 blocking 语义

必须遵守当前产品语义：
- 并发 = 多个 active task 拥有独立 task context
- 不是“单执行点在 task.step 之间跳转”
- `wait`、`delay`、`timeout`、`axis.move_*` 默认都是 blocking
- 一个 task 被 blocking step 挡住，不得阻塞同 tick 其他 task

如果一个 station 阻塞时另一个 station 还要继续跑，就必须拆成独立 task，不要把多工位流程压成一个大 `cycle` task。

## 3. wait / timeout 规则

- manual wait 必须显式使用 `allow_indefinite_wait: true`
- 非 manual wait 默认应有 timeout 或可解释的收敛路径
- recovery / fault target 必须是实际 `task.step` 路径，不是抽象注释

## 4. 机构设备动作不得退化为传感器编排

- 对于拓扑已闭合的机构设备，task step 应表达设备动作，不应显式重写关联传感器的正常到位闭环
- “拓扑已闭合”的判定与最小结果集合要求，服从 `AGENTS.md` 中“task 中的设备动作必须保持高层语义”的定义
- 动作成功、超时以及关联反馈导出的异常结果，应作为设备动作语义进入 compiler / IR / runtime
- task 负责按这些结果分流，不负责手写 `wait sensor_a`、`if sensor_a and not sensor_b` 这类底层闭环
- 如果 DSL 还承载不了某个设备动作结果，先报告能力缺口与 blocker，不要为了补机构闭环而伪造中间变量或监控 task

## 5. axis 规则

当存在 axis motion 时：
- `axis.move_relative` / `axis.move_absolute` 默认 blocking
- 必须带 `timeout`
- 必须带 `on_reject`
- 必须带 `on_motion_fault`
- 必须带 `on_safety_fault`
- 依赖“动作完成”后的 effect，应拆到后续 step

不要把 axis move 写成本 step 内立即完成的普通即时 action。

## 6. topology / device 质量

- `plc` controller 优先使用 `model_ref` profile，而不是在业务 DSL 里内联 `ports: [...]`
- 复杂项目里，如果 `X0` / `Y0` 这类名字只是控制器通道，不要直接把它们建成 `digital_input` / `digital_output` 设备
- `device` 只用于真实硬件对象；不要把 mode bit、manual jog bit、vacuum command bit、alias signal 之类的名字直接建成 `device`
- 操作员按钮、模式选择开关、点动请求优先建模成语义输入设备，例如 `sensor` + `push_button` / `selector_switch`
- 优先把现场对象建模成 `sensor`、`solenoid_valve`、`lamp`、`motor`、`cylinder` 等语义设备，再用 `relation { from, to, via }` 接到 `plc_main.<port>`
- 如果 system contract 只给了原始 I/O 名称，把它们当 mapping hint，而不是最终业务 topology
- 一旦出现 `SEM-108` 或 `SCN-MAP-010`，先重写 controller / IO topology，再继续修 task 或 scenario

优先保证：
- 每个 device 都有非空 `purpose`
- 用显式 `relation { from, to, via }`
- 端口声明与 relation 真实闭合
- `requires` 用于依赖约束
- `conflicts_with` 只用于真实状态冲突

对 scaffold 或复杂项目，把下面模式视为硬失败：
- controller 内联 `ports: [...]`
- 大量 `device <name>: digital_input`
- 大量 `device <name>: digital_output`

不要用 `conflicts_with` 表达执行顺序。

## 7. 共享资源与模式切换

当 system contract 明确有共享资源、互锁或模式矩阵时：
- 资源占用优先显式建模到 `semantic_resource` / `claim`
- 依赖顺序优先用 `requires`
- 真实互斥优先用 `conflicts_with`
- mode / service / supervisor 逻辑优先拆为独立 task，而不是塞进单个大 step

## 8. 生成 fault path 的要求

不要用模糊的“异常处理”注释替代真实 task。fault handling 必须成为实际 task / step，可被 semantic lowering、runtime bridge 与 verification 消费。
