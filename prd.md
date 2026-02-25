# PRD：Motor Control Enhancement v2.5（Ralph执行版）

日期：2026-02-25  
状态：Ready for Ralph  
来源：`docs/motor-control-enhancement.md`（v2.5）

## 1. Introduction / Overview

本需求将 RustPLC 的电机能力从单一 `motor on/off` 扩展到工业常见多设备模型（`motor`、`stepper_motor`、`vfd`、`servo_drive`），并补齐多端口语义在执行层与验证层的一致性。

本次采用破坏性迁移策略：旧写法 `motor.on` / `action: set motor on` 不再兼容，直接报错并提示迁移到显式端口写法（`motor.run.on` / `set motor.run on`）。

## 2. Goals

- 支持 `set` 动作的枚举状态输入，并在语义层做白名单校验。
- 支持新设备类型关键字和端口定义（motor/stepper/vfd/servo）。
- 完成 runtime_bridge 与 safety 的多端口语义闭环。
- 取消 motor 旧写法兼容层，统一显式端口建模。
- 形成可回归的测试矩阵，保证破坏性迁移行为被锁定。

## 3. User Stories（<=8）

### US-001: `set` 支持枚举状态并进行语义白名单校验
**Description:** As a DSL author, I want `set` to accept enum-like state tokens so that direction/active/idle can be expressed in control logic.

**Acceptance Criteria:**
- [ ] `src/parser/plc.pest` 中 `action_set` 改为接收 `state_value`（identifier）。
- [ ] `ActionStatement::Set.value` 从 `BinaryValue` 改为 `String`。
- [ ] 语义层新增 `validate_set_enum_values`，允许值仅为 `on/off/forward/reverse/active/idle`。
- [ ] 语义 lowering 仍输出二值 IR（保持 runtime 现状），非法值在 lowering 前被拦截。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-002: 扩展属性白名单并存储到 `extra_params`
**Description:** As a platform maintainer, I want new motor-related attributes parsed and preserved so that later phases can do typed validation.

**Acceptance Criteria:**
- [ ] `plc.pest` 的 `attribute_name` 增加电机参数字段（如 `steps_per_rev`、`accel_time` 等）。
- [ ] `apply_attribute` 新增对应分支，参数写入 `DeviceAttributes.extra_params`。
- [ ] `DeviceAttributes` 新增 `extra_params: HashMap<String, String>` 并提供默认值。
- [ ] 未在白名单内的属性继续报错，不引入静默吞参。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-003: 新增设备类型关键字与跨层枚举映射
**Description:** As a DSL user, I want `stepper_motor`/`vfd`/`servo_drive` to be first-class device types so that topology and semantics can recognize them.

**Acceptance Criteria:**
- [ ] parser/AST/IR/semantic/topology gate/device_subtype 中新增三类设备枚举分支。
- [ ] 所有 exhaustive match（含默认状态与名称映射）补齐分支并通过编译。
- [ ] 新类型可被 DSL 正常解析，并可进入语义阶段。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-004: 新增设备库定义（motor/stepper/vfd/servo）
**Description:** As a controls engineer, I want standard device TOML definitions so that constraints and ports are injected consistently.

**Acceptance Criteria:**
- [ ] 新增 `devices/motor.toml`、`devices/stepper_motor.toml`、`devices/vfd.toml`、`devices/servo_drive.toml`。
- [ ] 端口字段统一使用 `port_type`，并包含方向、默认态、状态集合。
- [ ] 设备约束可被 `inject_device_constraints` 注入并参与验证。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-005: Motor 旧写法破坏性迁移（不做兼容改写）
**Description:** As a maintainer, I want legacy motor shorthand rejected so that there is only one explicit-port modeling rule.

**Acceptance Criteria:**
- [ ] 不实现 `normalize_motor_compat` 或等效隐式改写。
- [ ] 对 `motor_x.on` / `motor_x.off` 给出语义错误并提示改写到 `motor_x.run.on/off`。
- [ ] 对 `action: set motor_x on/off` 给出语义错误并提示改写到 `set motor_x.run on/off`。
- [ ] 新增错误夹具锁定破坏性行为。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-006: runtime_bridge 完成多端口路由
**Description:** As a runtime engineer, I want output resolution keyed by `(device, port)` so that multi-port actuators do not alias to one channel.

**Acceptance Criteria:**
- [ ] `resolve_digital_output_id` 接口升级为接收 `device + port + state` 路由上下文。
- [ ] `convert_action` 调用链传递 `TransitionAction::Set.port`。
- [ ] `stepper.enable` 与 `stepper.direction` 能解析到不同物理输出通道。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-007: safety 引擎完成多端口状态索引
**Description:** As a verification engineer, I want safety checks indexed by `(device, port)` so that constraints evaluate per-port instead of per-device.

**Acceptance Criteria:**
- [ ] safety 侧 `device_index`（或等效结构）支持 `(device, port)` 维度。
- [ ] `action_effect`（或等效提取函数）返回包含 `port` 的动作效果。
- [ ] `enable.off conflicts_with pulse.active` 这类多端口约束可被正确验证。
- [ ] Typecheck passes.
- [ ] Tests pass.

### US-008: 回归测试与示例升级
**Description:** As a release owner, I want fixtures/examples/tests updated so that CI enforces new syntax and multi-port semantics.

**Acceptance Criteria:**
- [ ] 新增/更新解析、语义、verification 测试覆盖阶段 0~2 关键路径。
- [ ] 新增“旧 motor 写法报错”的回归用例并断言迁移提示。
- [ ] 新设备类型示例可编译（至少包含 stepper 单轴示例）。
- [ ] Typecheck passes.
- [ ] Tests pass.

## 4. Functional Requirements

- FR-1: `set` 指令必须支持枚举状态 token，并在语义层做白名单校验。
- FR-2: 设备参数属性必须可解析并保存到 `extra_params`。
- FR-3: DSL 必须支持 `stepper_motor`、`vfd`、`servo_drive` 设备类型。
- FR-4: 设备库必须提供四类电机设备 TOML 定义并可注入约束。
- FR-5: 旧 `motor` 简写语法必须报错，不提供兼容重写。
- FR-6: runtime_bridge 必须按 `(device, port)` 做输出路由。
- FR-7: safety 验证必须按 `(device, port)` 做状态验证。
- FR-8: CI 必须覆盖新语法、新设备、多端口语义与破坏性迁移行为。

## 5. Non-Goals (Out of Scope)

- 不实现旧 `motor` 语法自动迁移或兼容层。
- 不在本期实现多值状态 IR 全链路（仍保持二值 IR 映射）。
- 不实现高级运动控制（轨迹规划、插补、闭环伺服算法）。
- 不做 Web UI 能力扩展。

## 6. Technical Considerations

- 先完成阶段 0（语法与属性能力），再做阶段 1（新类型），最后完成阶段 2（多端口闭环）。
- 阶段 2 为高风险改动，必须以单元测试锁定 runtime_bridge 与 safety 的端口维度行为。
- 破坏性迁移策略需要明确错误文案，降低用户迁移成本。

## 7. Success Metrics

- 新增设备类型场景在 DSL 解析与语义阶段通过率为 100%。
- 多端口路由与多端口安全验证测试全部通过。
- 旧 `motor` 简写写法在 CI 中稳定失败并提供迁移提示。
- `cargo test --workspace` 与 `cargo check --workspace` 均通过。

## 8. Open Questions

- 阶段 3 的参数类型化（`extra_params` -> typed）是否拆分为独立 PRD。
- 多值状态 IR 何时从“语义映射”升级到“运行时原生支持”。
