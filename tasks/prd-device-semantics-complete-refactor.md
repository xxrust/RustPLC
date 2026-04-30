# RustPLC Device Semantics Complete Refactor Task List

Date: 2026-05-01
Owner: Codex + user

This task list replaces the earlier "one device at a time" trial plan with a full backlog.
The goal is to make every real field device, device capability, and PLC I/O channel have a
single semantic owner that can be consumed by semantic validation, IR, verification,
runtime bridge, and codegen.

## 0. Boundary Decisions

### 0.1 Move PLC I/O Points Out Of Device Semantics

`digital_input`, `digital_output`, `analog_input`, and `analog_output` are not process
devices. They are PLC ports, I/O channels, or signal binding endpoints.

Required outcome:

- Do not create `device_semantics::{digital_input,digital_output,analog_input,analog_output}`.
- Treat `plc_main.X0/Y0/AI0/AO0` as controller ports or channels.
- Field devices must own process meaning: `sensor`, `proportional_valve`, `vfd`,
  `servo_drive`, `pump`, `heater`, etc.
- Runtime still lowers to `DigitalInputId`, `DigitalOutputId`, `AnalogInputId`,
  `AnalogOutputId`.
- Verification tracks analog regions and digital states through signal/channel bindings,
  not as standalone field devices.

Must prove:

- Source DSL rejects new raw `device X0: digital_input`, `device Y0: digital_output`,
  `device AI0: analog_input`, `device AO0: analog_output` declarations.
- Legacy compatibility paths cannot bypass topology gate.
- `sensor.out -> plc_main.AI0` is accepted and verified as a sensor-to-channel binding.
- `plc_main.AO0 -> proportional_valve.cmd` is accepted and verified as a channel-to-device binding.

Primary files:

- `src/plc_port.rs`
- `src/io_map.rs`
- `src/topology_semantic_gate.rs`
- `src/runtime_bridge_guards.rs`
- `src/runtime_bridge_lowering.rs`
- `src/verification/safety_model_builder.rs`
- `src/verification/causality.rs`
- `src/codegen/st.rs`

### 0.2 Introduce Device / Capability / Port Layers

Every item must be classified into exactly one primary semantic layer:

- Task-level device semantics: user-facing process actions and result buckets.
- Capability semantics: implementation capability consumed by a task-level device or action.
- Port/channel semantics: PLC I/O and field signal binding.

Rules:

- Task code should express process intent, not hand-written sensor or coil choreography.
- Capabilities may constrain a task-level device but must not become duplicate task semantics.
- Ports/channels must never decide high-level DSL semantics.

Must prove:

- A high-level device action cannot silently lower to raw `set Y0` or raw coil writes.
- A backend that cannot carry a high-level device action rejects it explicitly.
- All source compile paths run the same topology/device semantics gate.

Primary files:

- `src/semantic/semantic_core.rs`
- `src/semantic/mod.rs`
- `src/ir/mod.rs`
- `src/runtime_bridge.rs`
- `src/runtime_bridge_transitions.rs`
- `src/codegen/st.rs`
- `src/verification/*`

## 1. Front-Door Safety Gates

### T1.1 Unify Compile Entrances

Problem:

`src/cli/shared/compile_pipeline.rs` runs `validate_topology_semantics`, but direct semantic
entry points can build state machines without the same gate.

Tasks:

- Audit all parser/semantic/IR builder entry points.
- Ensure source-level topology and device semantic gates run before IR consumers.
- Add a shared helper so tests and CLI do not drift.

Must prove:

- Direct `build_state_machine` style calls cannot accept raw I/O device declarations.
- Bundle, CLI compile, scenario tools, geometry export, runtime bridge, and codegen all see
  the same rejected source.

Tests:

- Unit test for direct semantic entry.
- CLI/bundle regression using raw I/O device declarations.

### T1.2 Block Raw I/O Bypass Of High-Level Device Actions

Tasks:

- Detect `set plc_main.Y*`, `set valve.coil*`, `set axis.enable/pulse/direction`, and raw
  sensor waits when they replace a declared high-level device action.
- Allow raw port control only under explicit low-level mode or fixture-only compatibility.
- Add stable diagnostics with fix suggestions.

Must prove:

- A cylinder with closed-loop topology cannot be driven by manually setting its valve coil
  in a normal task.
- An axis cannot be moved by manually pulsing `pulse` in a normal task.
- A conveyor/pump/heater task cannot bypass its device action by writing only the drive output.

## 2. Existing Task-Level Devices

### T2.1 Cylinder

Status:

Partially complete. Shared semantics crate exists and runtime-core consumes shared types.

Remaining tasks:

- Replace old IR `Extend/Retract + optional fields` with a first-class device action contract.
- Remove runtime bridge fallback from closed-loop cylinder to raw `Action::Extend/Retract`.
- Ensure raw feedback waits are rejected or converted into device action result handling.
- Keep codegen explicit: implement full cylinder backend or reject.

Must prove:

- Complete dual-feedback topology lowers to `CylinderMotion`.
- Missing feedback fails before runtime.
- Opposite feedback and contradictory feedback route to explicit fault buckets.
- ST backend cannot generate false-success coil-only code for closed-loop cylinder.

### T2.2 Axis

Status:

Task-level axis semantics exists, but target selection is still tied to concrete device kinds.

Remaining tasks:

- Introduce `MotionAxisCapability`.
- Make `axis.move_*` validate against capability, not hard-coded `stepper_motor | servo_drive`.
- Move `acc/dec` into IR and runtime command, not only semantic validation.
- Stop safety verification from reducing `axis.move_*` to `pulse.active`.
- Represent pending axis action as a first-class verification state.

Must prove:

- A non-stepper/servo device with declared axis capability can be used by `axis.move_*`.
- A stepper/servo device missing axis capability or profile is rejected.
- Motion actions carry speed, acceleration, deceleration, timeout, and fault routes to runtime.
- Safety/liveness/timing reason about pending axis action, not only pulse state.

## 3. Existing Capability Devices

### T3.1 Stepper Motor

Layer:

Device identity plus axis capability provider.

Tasks:

- Keep `stepper_motor` as a real device identity with ports and feedback.
- Move axis eligibility to `MotionAxisCapability`.
- Keep low-level pulse/direction mapping in runtime bridge or board profile only.
- Define stepper-specific fault mappings: drive fault, limit fault, optional lost-step policy.

Must prove:

- Stepper port model cannot be used directly as task-level motion semantics.
- Stepper can satisfy axis capability only when model/config/profile are complete.
- Board targets can still consume pulse/direction mapping without owning DSL semantics.

### T3.2 Servo Drive

Layer:

Device identity plus axis capability provider.

Tasks:

- Keep `servo_drive` as a real device identity with ready, in_position, fault, clear_fault,
  zero_speed, and optional feedback ports.
- Map servo status into axis result buckets.
- Model clear-fault/reset behavior explicitly.
- Keep position feedback, torque feedback, and alarm code as capability fields.

Must prove:

- `ready=false` rejects or faults a motion action before fake completion.
- `in_position` participates in completion when the axis profile requires it.
- `fault=true` routes through axis fault buckets.

### T3.3 Motor

Layer:

Drive capability by default, not the primary process device for conveyor, pump, fan, or mixer.

Tasks:

- Keep `motor` as drive identity/capability: run, direction, running, fault.
- Reject legacy `set motor on/off` task syntax outside low-level mode.
- Define when `motor` may be used directly as a task-level device, such as a standalone fan.
- Add drive capability interface consumed by conveyor, pump, and other process devices.

Must prove:

- Conveyor/pump/heater semantics cannot be replaced by raw `motor.run`.
- Direct motor use requires explicit direct-drive mode and fault handling.
- Direction/run conflicts are verified.

### T3.4 VFD

Layer:

Variable-speed drive capability, sometimes direct task-level device only with explicit mode.

Tasks:

- Define analog speed/frequency command port with range and unit.
- Define running, fault, and freq_arrive feedback semantics.
- Bind `set_analog` to a specific port/channel, not to the device name.
- Model ramp/settle timeout and overrange rejection.

Must prove:

- Speed command outside configured range is rejected at compile time.
- Missing `freq_arrive` or equivalent feedback requires explicit open-loop policy.
- Pump/conveyor can consume VFD capability without duplicating VFD task semantics.

### T3.5 Solenoid Valve

Layer:

Valve device identity or pneumatic capability. It should only become task-level semantics when
the DSL expresses a valve action, not merely a cylinder action.

Tasks:

- Resolve current mismatch: TOML declares `coil_A/coil_B`; topology gate injects `coil/out`.
- Decide and support valve variants: single-solenoid, double-solenoid, 3-position.
- Define coil mutual exclusion and safe default.
- Define valve action result only if task-level valve actions are supported.
- Ensure cylinder actions consume valve capability without exposing coil choreography to tasks.

Must prove:

- Double coils cannot be energized together.
- Cylinder action cannot be bypassed by raw valve coil writes.
- A valve without required variant fields fails semantic validation.

## 4. Existing Controller / Algorithm Devices

### T4.1 Cam Coupling

Layer:

Motion controller semantics.

Tasks:

- Promote `cam_coupling` from target-type checks to a full device action contract.
- Define `engage`, `disengage`, `switch_table`, and `phase_shift` result buckets.
- Bind `in_sync`, `fault`, `following_error`, `master_pos`, and `slave_cmd`.
- Verify table existence, master/slave binding, following-error limits, and switch safety.

Must prove:

- Cam actions cannot target non-cam devices.
- Missing cam table or invalid switch table fails compile.
- `fault.on` conflicts with engaged state.
- Following error beyond limit routes to fault or verification failure.

### T4.2 PID

Layer:

Control algorithm block, not a field device.

Tasks:

- Decide whether `pid` remains a device type or moves to controller/function-block semantics.
- Bind PV and OUT through signal/channel bindings.
- Validate PV range, output range, period, and setpoint.
- Ensure anti-windup and output saturation semantics enter runtime/verification.

Must prove:

- PID cannot use bare `AI0/AO0` as fake devices in new source.
- PV/OUT bindings resolve uniquely through channels.
- Output saturation and range violations are reported.

## 5. New Task-Level Process Devices

### T5.1 Proportional Valve

Layer:

Task-level process actuator with analog command and optional feedback.

Tasks:

- Add parser/AST/IR/device library support for `proportional_valve`.
- Define actions: set_opening, open_to, close, hold, reset_fault.
- Define command port, optional feedback port, range, unit, ramp, settle tolerance.
- Define result buckets: done, timeout, reject, motion_fault, safety_fault.

Must prove:

- Command range and unit are checked.
- Missing feedback requires explicit open-loop policy.
- Saturation, feedback deviation, and fault feedback are visible to verification.

### T5.2 Gripper

Layer:

Task-level end-effector device.

Tasks:

- Add `gripper` device family.
- Define actions: grip, release, hold, reset_fault.
- Support pneumatic/electric capability providers.
- Integrate with workpiece possession: held, released, lost_part, no_part.
- Define feedback: gripped, released, part_present, fault.

Must prove:

- `grip` requires part presence or explicit empty-grip policy.
- A held workpiece cannot be simultaneously free at another holder.
- Release must transition workpiece state or report fault.
- Raw cylinder/valve choreography cannot replace gripper semantics in normal tasks.

### T5.3 Conveyor

Layer:

Task-level transport device.

Tasks:

- Add `conveyor` device family.
- Define actions: start, stop, move_until, index, reverse, clear_jam.
- Consume motor or VFD drive capability.
- Bind zone sensors, entry/exit sensors, jam/overload feedback.
- Integrate with workpiece flow and occupancy.

Must prove:

- Conveyor start without drive capability is rejected.
- Zone capacity and workpiece occupancy are verified.
- Jam/overload routes are explicit.
- Stop command has safe-state semantics.

### T5.4 Pump

Layer:

Task-level fluid actuator.

Tasks:

- Add `pump` device family.
- Define actions: start, stop, prime, hold_pressure, reset_fault.
- Consume motor or VFD drive capability.
- Bind pressure/flow/level feedback.
- Define dry-run, overpressure, no-flow, and low-level fault buckets.

Must prove:

- Start requires low-level and interlock checks when declared.
- Pressure/flow feedback closure is complete or open-loop policy is explicit.
- Overpressure conflicts with continued run.

### T5.5 Heater

Layer:

Task-level thermal actuator.

Tasks:

- Add `heater` device family.
- Define actions: heat_to, hold_temperature, stop_heat, reset_fault.
- Bind SSR/relay or analog power output capability.
- Bind temperature sensor feedback.
- Define overtemperature, sensor_fault, timeout, and thermal runaway buckets.

Must prove:

- Heating requires temperature feedback unless explicit open-loop policy exists.
- Overtemperature conflicts with heat output.
- Sensor fault prevents false completion.

### T5.6 Vision Sensor

Layer:

Task-level inspection sensor.

Tasks:

- Add `vision_sensor` device family.
- Define actions: trigger, acquire, inspect, reset_fault.
- Bind camera trigger output, exposure-ready input, result bits, optional score/position analog data.
- Define result buckets: pass, fail, timeout, reject, communication_fault, safety_fault.

Must prove:

- Inspection result cannot be consumed before acquire/inspect completes.
- Trigger channel conflicts and missing result binding are rejected.
- Fail and communication fault routes are explicit.

### T5.7 Camera Trigger

Layer:

Usually a port/action binding, not a standalone field device. It may be modeled as a capability
of `vision_sensor` or `camera`.

Tasks:

- Do not add `camera_trigger` as a bare process device unless it carries full trigger device semantics.
- Prefer modeling trigger as `vision_sensor.trigger` bound to `plc_main.Y*`.
- If standalone trigger controller is needed, define it as `trigger_controller`.

Must prove:

- Bare `digital_output` trigger scripts cannot replace vision action semantics.
- Trigger pulse width and busy/ready interlock are verified.

## 6. Shared Device Semantics Infrastructure

### T6.1 Extend `crates/device-semantics`

Tasks:

- Add shared no-std modules for retained device families and capability contracts.
- Keep AST/IR-free constants, result enums, default policy, fault categories, and audit IDs here.
- Do not put PLC I/O channel types here.

Families:

- `cylinder`
- `axis`
- `drive`
- `valve`
- `gripper`
- `conveyor`
- `pump`
- `heater`
- `proportional_valve`
- `vision`
- `cam`

### T6.2 Extend Compiler-Side `src/device_semantics`

Tasks:

- Add AST/IR/topology validation adapters per family.
- Provide one action contract API consumed by semantic, verification, runtime bridge, and codegen.
- Add diagnostics with stable codes.

### T6.3 Extend Device Library Schema

Tasks:

- Add `interface_contract`: command, status, parameter, feedback, fault, safety signals.
- Add `capabilities`: axis, drive, pneumatic_valve, analog_command, inspection, thermal, fluid.
- Add `defaults`: timeout, debounce, safe state, open-loop policy.
- Add `alarm_map`: fault kind, severity, stop scope, recovery action.
- Add `verification_contract`: required feedback, result buckets, forbidden raw ports.
- Add `codegen_support`: supported backends or explicit unsupported reason.

Must prove:

- Device library can parse old TOML unchanged.
- New fields can be parsed and validated.
- Missing required contract fields fail only for families that require them.

## 7. IR / Runtime / Verification / Codegen Contract

### T7.1 First-Class Device Actions In IR

Tasks:

- Replace scattered `Set/Extend/Retract/AxisMove/Cam*` semantics with a normalized device action
  contract where practical.
- Keep specialized payloads only when the semantics require them.
- Carry result buckets, timeout, topology contract, and capability references.

Must prove:

- Runtime bridge does not invent missing device semantics.
- Verification consumes the same result buckets as runtime.
- Codegen can decide support from the IR contract.

### T7.2 Formal Verification Per Device Family

Every task-level device family must include:

- Safety positive example.
- Safety violation example.
- Liveness/timeout example.
- Timing bound example.
- Causality chain example.
- Raw I/O bypass rejection example.
- Codegen unsupported or supported backend example.

### T7.3 Runtime Bridge No-Fallback Rule

Tasks:

- Remove fallback paths that turn high-level closed-loop actions into raw output toggles.
- Require explicit open-loop policy when no feedback exists.
- Preserve pending action lifecycle for long-running actions.

Must prove:

- Missing topology contract fails before runtime.
- Open-loop action is visible as open-loop in reports.
- Closed-loop action cannot silently become open-loop.

## 8. Parallel Work Packages

These work packages are designed to be implemented by separate agents in parallel after the
contracts above are frozen.

### WP-A: I/O Channel And Signal Binding

Scope:

- Remove IO points from device semantics.
- Migrate analog/digital verification to channel bindings.
- Harden topology gate.

Files:

- `src/plc_port.rs`
- `src/io_map.rs`
- `src/topology_semantic_gate.rs`
- `src/verification/safety_model_builder.rs`
- `src/verification/causality.rs`
- `src/runtime_bridge_*`
- `src/codegen/st.rs`

### WP-B: Axis Capability And Stepper/Servo Cleanup

Scope:

- Add `MotionAxisCapability`.
- Stop hard-coded stepper/servo target checks.
- Carry acceleration/deceleration into runtime.
- Fix safety model for axis pending actions.

Files:

- `src/axis_profile.rs`
- `src/device_semantics/axis.rs`
- `src/ir/mod.rs`
- `src/runtime_bridge_transitions.rs`
- `crates/runtime-core/src/lib.rs`
- `src/verification/*`

### WP-C: Drive, VFD, And Solenoid Valve Capability

Scope:

- Define drive and valve capabilities.
- Resolve solenoid valve port mismatch.
- Make VFD analog command a port binding.
- Prevent raw drive writes from replacing process device actions.

Files:

- `devices/motor.toml`
- `devices/vfd.toml`
- `devices/solenoid_valve.toml`
- `src/device_library.rs`
- `src/device_semantics/*`
- `src/topology_semantic_gate.rs`

### WP-D: Process Actuators

Scope:

- Add task-level `gripper`, `conveyor`, `pump`, `heater`, and `proportional_valve`.
- Integrate feedback, result buckets, and verification.

Files:

- `src/parser/plc.pest`
- `src/parser/topology.rs`
- `src/ast/mod.rs`
- `src/ir/mod.rs`
- `src/device_semantics/*`
- `devices/*.toml`
- `src/runtime_bridge*`
- `src/verification/*`
- `src/codegen/st.rs`

### WP-E: Vision Sensor And Trigger Semantics

Scope:

- Add `vision_sensor`.
- Model trigger as capability/port binding.
- Define acquire/inspect result buckets.

Files:

- Same parser/AST/IR/device semantics stack as WP-D.
- Add examples and verification tests for inspection pass/fail/fault.

### WP-F: Cam And PID Controller Semantics

Scope:

- Promote cam coupling action validation into full device action contract.
- Decide PID as controller/function-block semantics rather than field device.

Files:

- `src/device_semantics/cam.rs`
- `src/parser/tasks.rs`
- `src/ir/mod.rs`
- `src/runtime_bridge*`
- `src/verification/*`

## 9. Completion Criteria

The refactor is complete only when:

- PLC I/O points are not treated as process devices in new source.
- Every real process device has exactly one semantic owner.
- Every capability provider has a clear consumer and cannot define duplicate task semantics.
- All high-level actions enter IR with result buckets and verification contracts.
- Runtime bridge and codegen cannot silently degrade high-level semantics to raw I/O.
- Tests cover semantic rejection, runtime behavior, verification, and codegen support/unsupported paths.
