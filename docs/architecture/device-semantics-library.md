# Device Semantics Library

Related:
- Intent-alignment verification is defined in `docs/architecture/intent_alignment_verification.md`.

## 1. 背景

当前 RustPLC 中，`cylinder`、`axis`、`motor` 这类抽象设备的语义分散在多个层面：

- `parser` / `ast` 负责接受高层动作语法
- `semantic` 负责部分动作合法性检查
- `runtime_bridge` 负责大量设备闭环解析与拓扑推断
- `runtime-core` 负责动作执行结果与 pending 生命周期
- `verification` 负责 safety / timing / causality 的设备相关推断
- `codegen` 负责目标后端可承载性的裁决

问题不在于“文件多”，而在于“同一设备语义没有唯一归属点”。

以气缸为例，下面这些知识目前并没有集中归口：

- 气缸的动作语义是什么
- 闭环气缸需要哪些拓扑要素
- 哪些问题应在编译期拒绝
- 哪些问题应作为运行期动作结果
- verification 应如何认识这些结果
- codegen 若承载不了，应在什么层明确拒绝

这会导致三个长期问题：

1. DSL 主流程越来越长，设备细节不断泄漏到 task 代码
2. bridge / verification / codegen 对同一设备形成不同契约
3. 新增一个设备家族时，只能到处复制粘贴规则

因此需要引入一个明确的主线抽象：

`device_semantics`

它不是设备实例库，也不是运行时模拟器，而是“设备家族语义库”。

## 2. 目标

`device_semantics` 的目标不是让 DSL 更短，而是让“高层设备动作”拥有唯一、可验证、可复用的语义归属点。

长期目标：

- `task` 只表达设备动作意图
- 设备拓扑闭环要求在设备语义层定义
- 设备动作结果枚举在设备语义层定义
- semantic / runtime / verification / codegen 都消费同一份设备语义

这意味着：

- 不允许再把闭环气缸写成显式传感器 choreography
- 不允许 bridge 私下发明某个设备的完整执行契约
- 不允许 codegen 静默丢失设备动作结果语义

## 3. 与现有层的边界

### 3.1 `device_library`

`device_library` 解决的是：

- 某类设备有哪些端口
- 默认状态是什么
- 设备级通用约束是什么

它不应承担：

- 某个高层动作有哪些运行结果
- 某个动作如何闭环解析
- 某个动作如何进入 verification/runtime

### 3.2 `topology`

`topology` 解决的是：

- 这个项目里具体接了哪些设备
- 设备之间如何连线
- 物理 I/O 如何映射

它不应承担：

- 从这些连线中推导出某个设备动作的完整执行契约

### 3.3 `device_semantics`

`device_semantics` 解决的是：

- 某类设备的动作模型
- 该动作要求的最小闭环拓扑
- 该动作的编译期约束
- 该动作的结果枚举
- 该动作如何进入 IR / runtime / verification / codegen

### 3.4 `semantic`

`semantic` 的职责变为：

- 识别 DSL 中的高层设备动作
- 调用对应 `device_semantics::<family>` 做校验与 lowering
- 拒绝不完整或无意义的设备动作

### 3.5 `runtime_bridge`

`runtime_bridge` 的职责变为：

- 消费设备语义层给出的闭环契约
- 将其映射为 runtime-core 可执行结构

它不应继续独占：

- “什么叫闭环气缸”
- “两个磁性开关是否必须”
- “哪些反馈属于 stroke fault / safety fault”

### 3.6 `verification`

`verification` 的职责变为：

- 基于设备语义层定义的动作结果与状态影响做分析

它不应再依赖 scattered 的 axis/cylinder 特判来猜测设备行为。

### 3.7 `codegen`

`codegen` 的职责变为：

- 明确声明某后端是否承载某设备语义
- 能承载则生成
- 不能承载则显式拒绝

它不允许：

- 看起来生成成功，实际静默丢失设备语义

## 4. 建议目录

建议新增：

```text
crates/device-semantics/
  src/lib.rs
  src/cylinder.rs

src/device_semantics/
  mod.rs
  cylinder.rs
  axis.rs
  motor.rs
```

`crates/device-semantics` 只放不依赖分配、不依赖 AST/IR 的纯设备语义，例如常量、默认值、动作枚举和 runtime 可见 fault 类型。`src/device_semantics` 负责把这些纯语义接到主编译器的 AST、IR、semantic validation、topology lowering 和诊断上。

第一阶段只要求真正落地 `cylinder.rs`，其余文件可以后续补。

## 5. 每个设备语义模块应包含什么

以 `cylinder.rs` 为例，长期应包含 5 类内容。

### 5.1 家族级固定语义

- 设备家族名
- 关键动作名
- 默认端口/状态命名约定

例如：

- `cmd`
- `extended`
- `retracted`

### 5.2 拓扑闭环契约

定义某个动作是否属于：

- 开环动作
- 闭环动作

定义闭环动作的最小要求，例如：

- 伸出确认反馈
- 缩回确认反馈
- 两者必须成对存在

### 5.3 编译期错误分类

例如气缸：

- 目标不是气缸
- 动作声明了闭环结果分流，但拓扑不具备闭环条件
- 只声明了部分结果分流
- 闭环动作缺少互补反馈

### 5.4 运行期动作结果枚举

例如气缸闭环动作：

- `done`
- `timeout`
- `motion_fault`
- `safety_fault`

更细的设备内部原因仍可保留，例如：

- `motion_fault: opposite_feedback_reasserted`
- `safety_fault: contradictory_feedback`

但 DSL 主桶应先保持稳定。

### 5.5 多后端消费接口

同一个设备语义模块，应同时服务：

- semantic lowering
- runtime bridge
- verification
- codegen capability check

## 6. 气缸 first slice 的最终形态

### 6.1 DSL 层

`task` 里只写高层动作，不写传感器脚本：

```plc
task clamp:
    step close:
        action: extend cyl_clamp
        timeout: 500ms -> fault.timeout
        on_motion_fault -> fault.motion_fault
        on_safety_fault -> fault.safety_fault
```

这里表达的是：

- 一个气缸动作
- 一个完整结果集分流

不是：

- 手动 wait 伸出磁开
- 手动判断另一侧磁开是否释放
- 手动判断两个磁开是否同时触发

这些都必须属于 `cylinder` 语义库与编译器。

### 6.2 语义层

`semantic` 应通过 `device_semantics::cylinder` 回答：

- 目标是不是气缸
- 这个动作是不是闭环动作
- 若要求闭环，当前 DSL 分流是否完整
- 若要求闭环，当前拓扑是否具备互补反馈

气缸 stroke 动作契约应集中表达默认参数，而不是让各层各自硬编码：

- `feedback_debounce_ms` 默认 `20`
- `stroke_timeout_ms` 默认 `3000`
- `allow_extend` 默认 `true`
- `allow_retract` 默认 `true`
- `simulation_mode` 默认 `false`

这些默认值属于设备语义契约的基线。项目 manifest / topology / HMI 参数以后可以覆盖它们，但覆盖后的值仍必须通过 `device_semantics::cylinder` 进入 IR、runtime、verification 和 codegen。runtime 不应在缺少 `timeout -> <target>` 时自行猜测默认超时跳转目标。

轴的第一阶段语义集中点与气缸不同：它不是端位反馈闭环，而是长时运动命令的生命周期、故障分类和策略分流。

- `crates/device-semantics::axis` 定义 runtime 可见的纯语义类型：`AxisFaultKind`、`AxisFaultCategory`、`AxisMotionResult`、`AxisMoveKind`、`AxisFaultRouteKind`、`AxisFaultPolicy`、停机状态与审计消息 ID。
- `src/device_semantics::axis` 定义主编译器侧动作契约：`axis.move_relative` / `axis.move_absolute` 默认端口为 `self`，`move_absolute` 默认要求 homed，运动动作必须具备 `timeout`、`on_reject`、`on_motion_fault`、`on_safety_fault` 四类分支。
- fault route bucket 也属于轴动作契约：`on_reject` 只接受 `reject/vendor`，`on_motion_fault` 只接受 `motion/vendor`，`on_safety_fault` 只接受 `safety/vendor`。
- runtime-core 只重新导出并消费这些类型；它不再作为轴 fault 分类、停机审计 ID 或 motion result 的定义源。

### 6.3 IR 层

长期理想形态不是不断给 `TransitionAction::Extend/Retract` 追加字段。

更理想的方向是：

- 保留 DSL 的高层动作名
- 同时挂接一个“设备动作契约”

例如概念上：

```text
TransitionAction::DeviceStroke {
  family: "cylinder",
  verb: "extend",
  target: "cyl_clamp",
  contract: closed_loop_cylinder_dual_feedback_v1,
  routes: { timeout, on_motion_fault, on_safety_fault }
}
```

第一阶段不要求立刻完成这类 IR 重构，但文档上必须把方向写清楚。

### 6.4 runtime 层

runtime-core 负责执行状态机，不负责重新定义什么叫气缸。

它只消费 bridge 已解析好的契约，例如：

- 输出口
- 确认反馈集合
- 对侧反馈集合
- timeout target
- stroke fault target
- safety fault target

### 6.5 verification 层

verification 应认识：

- 这是一个气缸闭环动作
- 它有 timeout / stroke_fault / safety_fault 分流
- 这些分流任务也是可达结构的一部分

不能只在 axis 上建模，到了 cylinder 又退回普通布尔输出。

### 6.6 ST/codegen 层

若某后端不能承载闭环气缸动作结果分流，应显式拒绝：

- 不是生成一个“只有线圈赋值”的假成功结果
- 而是返回明确的 backend unsupported 诊断

## 7. 推荐迁移顺序

### Phase 1

建立 `src/device_semantics/cylinder.rs`，先收拢纯语义常量与帮助函数：

- 端口命名
- 互补端口推导
- 端态端口识别
- 闭环结果桶定义

### Phase 2

把以下知识从 `runtime_bridge.rs` 上提到 `device_semantics::cylinder`：

- 互补反馈判定
- 闭环拓扑最小要求
- 气缸动作结果桶契约

### Phase 3

把 `semantic` 中的气缸门禁接入 `device_semantics::cylinder`：

- 非气缸目标拒绝
- 闭环结果分流完整性检查
- 相关 diagnostics 稳定化

### Phase 4

让 verification/codegen 消费同一份设备语义：

- safety / timing / causality 认识气缸动作分流
- ST backend 对不支持语义显式拒绝

### Phase 5

把同样模式推广到：

- `axis`
- `motor`
- 后续夹爪、真空、转台等复合设备

## 8. 当前这轮落地范围

本轮已经不止是 `runtime_bridge` 复用 helper，而是先把 `cylinder` 家族的核心解释点集中到可共享模块，再让多层开始消费同一份语义。

当前已经完成：

1. 增加 `src/device_semantics/` 目录与统一入口。
2. 建立 `src/device_semantics/cylinder.rs`，收敛气缸动作、端口、fault bucket 和目标类型门禁。
3. `semantic` 通过 `device_semantics::validate_task_action_semantics` 执行气缸动作校验。
4. `runtime_bridge` 复用气缸端态/helper，而不是在 bridge 内部重新定义。
5. `verification/causality` 通过共享的 `stroke_action_view` 读取气缸 fault 分流。
6. `codegen/st` 通过共享 helper 显式拒绝 ST 后端尚不支持的闭环气缸语义。

本轮还没有完成的，是把 safety / timing / runtime lowering / codegen rejection 全部进一步统一到更完整的设备家族接口上。也就是说，方向已经进入多层共享，但还没有达到“所有下游都只消费单一 device family contract”的终局。


## 9. 裁决原则

后续如果某个改动满足以下任意一种情况，就不应继续接受：

- 把设备闭环语义重新塞回 task 传感器脚本
- 让 bridge 单独发明完整设备契约
- 让 verification 与 runtime 消费不同设备契约
- 让 codegen 静默丢失设备语义

唯一允许的方向是：

- 高层设备动作留在 DSL
- 设备家族语义集中到 `device_semantics`
- semantic / IR / runtime / verification / codegen 共享这份语义

## 10. Current Public Contract

The current public device-semantics contract is broader than the original cylinder-first slice:

- `cylinder` keeps closed-loop stroke semantics at the device-action layer.
- `axis.move_relative` and `axis.move_absolute` are blocking long-running actions with explicit timeout/reject/motion-fault/safety-fault routes.
- Process families (`proportional_valve`, `gripper`, `conveyor`, `pump`, `heater`, `vision_sensor`) expose family-specific `DeviceAction` contracts.
- Process-device actions lower to IR and runtime as first-class actions. Runtime execution requires an explicit process-device handler through `tick_with_process_device`; plain `tick()` must reject them instead of guessing a hardware result.
- ST codegen must explicitly reject unsupported first-class device actions rather than silently lowering them to raw I/O writes.
- Device-library `[defaults.parameters]`, `[defaults.ports]`, and `[alarm_map]` are part of the semantic front door and must be kept in sync with runtime and verification consumers.

Analog input/output authoring is intentionally outside this public device-semantics contract for now. It remains a lower-level controller I/O and board-mapping concern, not a process-device source model.
