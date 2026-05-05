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
- `00_topology/`
- `process_model/` for source-side process operation scheduling-intent TOML when the project has discrete workpiece flow or pipelined admission/resource policy
- `01_init/`
- `02_process/`
- `03_constraints/`
- `04_faults/`
- `05_supervision/`
- `06_manual/`
- `07_hmi/`

Reference example:
- `rust_plc new <project_dir> --layout structured-fragments`

Do not split by arbitrary line count or by temporary implementation convenience.
Do split by stable ownership and semantic responsibility so the project is suitable for parallel implementation and later review.

## Process operation model rule

For discrete workpiece flow, do not let the generated task sequence be the only place where scheduling intent exists.

Author or refresh before writing task/step:

```text
process_model/process_operation_model.toml
```

Then validate after task/step generation with:

```bash
rust_plc process-model-check <source.plc|source.bundle.toml> --model process_model/process_operation_model.toml
```

If migrating an existing task/step source, `operation-model` may bootstrap a review draft:

```bash
cargo run --release --bin rust_plc -- operation-model out/<project>/rustplc.bundle.toml --out out/<project>/process_model/process_operation_model.toml
```

Review requirements:
- `operation_classes` should normalize repeated physical slots, for example `storage_box.slot[0]` and `storage_box.slot[1]` should appear as one class like `storage_box.slot[*]`
- `admissions` should expose source availability, destination capacity, program/operator guards, and semantic-resource availability where applicable
- the model should be treated as source-side scheduling intent, not as a disposable verification artifact
- `process-model-check` must pass before delivery; `OP-002` means the task/step flow added unjustified same-task serialization
- generic program/operator guards must not be used as the reason to suppress OP-002; only shared endpoint/resource or an explicitly modeled predecessor relation should justify ordering

Do not place the default output at `out/process_operation_model.json`.
Use JSON only when a machine consumer explicitly needs it; prefer TOML for human review and project authoring.

## Task and step comment rule

Every generated or repaired `task` and `step` must have a concise Chinese `#` comment immediately before it.

The task comment should name the task responsibility and boundary.
The step comment should explain operational intent plus the exit/completion condition, such as wait satisfaction, timeout route, semantic device-action completion, workpiece transfer, or terminal logging.

Do not emit bare task/step blocks in project delivery sources.
Do not use comments that only repeat the identifier in Chinese; make the comment useful for a reviewer who does not already know the sequence.

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

Capacity requirements:
- use `capacity: 1` for true single-part positions such as pickup positions, process stations, sleeve entries, handoff points, and holders
- use `capacity > 1` for finite containers such as storage boxes, bins, racks, magazines, cassettes, trays, buffers, hoppers, reject bins, and scrap boxes
- if the system contract names a container but gives no number, choose a conservative finite capacity and record the assumption in `main.system.md`
- do not let a one-cycle nominal scenario collapse a real container to `capacity: 1`

Before running validation, classify the flow as single-shot, finite-batch, or repeating:
- for single-shot or patent/demo acceptance flows, make the successful path terminal unless the contract explicitly describes replenishment
- for repeating flows, the source site must be replenished by modeled ingress, scenario evidence, or another upstream task before the next cycle consumes it
- do not write a loop that reads the same finite ingress token again; the verifier should treat that as workpiece underflow, not as an implicit new part

For every normal or fault terminal path, enumerate the possible active workpiece locations first:
- if the workpiece may still be at the input site, finish or reject it from that site
- if it may be in a holder, transfer or finish it from that holder
- if it may already be at the output, close the terminal state there
- do not route all faults to one generic terminal handler unless that handler is proven valid for every possible workpiece stage
- treat `storage_box`, `reject_bin`, `tray`, `rack`, or `buffer` with `capacity: 1` as suspicious unless the system contract explicitly says it is a single-position station

Process-only exception:
- if the confirmed system is a valve station, thermal process, pressure loop, or other process-only asset with no discrete part ownership flow, do not invent workpiece semantics
- in scaffolded projects, explicitly switch `config/workpiece.toml` to a deliberate no-workpiece exception before claiming validation

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
- start/reset/acknowledge 等瞬时人工命令应生成 `wait: rising_edge(<alias_or_button>)`；只有明确等待释放时才使用 `falling_edge(...)`，不要再用额外的 `wait: <button> == false` 步骤模拟边沿触发
- 非 manual wait 默认应有 timeout 或可解释的收敛路径
- recovery / fault target 必须是实际 `task.step` 路径，不是抽象注释

## 3.1 timing budget 不得拍脑袋

- `must_complete_within` / `must_complete_within_worst_case` 必须和实际 authored 路径一致
- 不要写一个低于固定 `delay` 总和、显式 timeout 上界或重复次数展开总量的 narrative 数字
- 对 `repeat N` 或多段冷却/保压路径，要把每次重复都计入预算
- 如果当前还算不清 budget，先不写该 timing claim，或明确写成 assumption / blocker

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
- 复杂项目优先在 `controller.plc` 中用 `controller_io plc_main { ... }` 给 PLC 物理点位定义业务别名
- 复杂项目里，如果 `X0` / `Y0` 这类名字只是控制器通道，不要直接把它们建成 `digital_input` / `digital_output` 设备
- `device` 只用于真实硬件对象；不要把 mode bit、manual jog bit、vacuum command bit、alias signal 之类的名字直接建成 `device`
- 操作员按钮、模式选择开关、点动请求优先建模成语义输入设备，例如 `sensor` + `push_button` / `selector_switch`
- 优先把现场对象建模成 `sensor`、`solenoid_valve`、`lamp`、`motor`、`cylinder` 等语义设备，再用 `relation { from, to, via }` 接到 `plc_main.<alias>`；小型测试可临时使用 `plc_main.<port>`
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

## 6.1 raw AI/AO process-control boundary

- 当前 complex-project source path 不要默认把 controller AI/AO 通道当作工程量过程设备
- 压力、温度、比例阀、加热、夹爪、输送、泵、视觉等应优先进入过程设备语义
- 如果缺少对应过程设备契约，应标记为能力边界 / blocker

不要继续硬凑一个“已验证”的 PID、温控或压力交付。

## 7. 共享资源与模式切换

当 system contract 明确有共享资源、互锁或模式矩阵时：
- 资源占用优先显式建模到 `semantic_resource` / `claim`
- 依赖顺序优先用 `requires`
- 真实互斥优先用 `conflicts_with`
- mode / service / supervisor 逻辑优先拆为独立 task，而不是塞进单个大 step

## 8. 生成 fault path 的要求

不要用模糊的“异常处理”注释替代真实 task。fault handling 必须成为实际 task / step，可被 semantic lowering、runtime bridge 与 verification 消费。
