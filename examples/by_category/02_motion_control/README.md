# 02 Motion Control

Axis motion, board-oriented motion, and fault-routing fixtures.

| Example | Kind | Source | Scenario | Purpose |
| --- | --- | --- | --- | --- |
| `dual_axis_platform` | `plc` | [`examples/dual_axis_platform.plc`](../../dual_axis_platform.plc) |  | Canonical dual-axis motion example used by quickstart docs. |
| `rp2040_motion_minimal` | `plc` | [`examples/rp2040_motion_minimal.plc`](../../rp2040_motion_minimal.plc) |  | Board-oriented motion example paired with RP2040 scenarios and IO map. |
| `stepper_collision_guard` | `plc` | [`examples/stepper_collision_guard.plc`](../../stepper_collision_guard.plc) |  | Stepper safety and collision-guard scenario fixture. |
| `axis_move_blocking_baseline` | `plc` | [`examples/axis_move_blocking_baseline.plc`](../../axis_move_blocking_baseline.plc) |  | Blocking axis move baseline without hand-written wait steps. |
| `axis_servo_fault_routing` | `plc` | [`examples/axis_servo_fault_routing.plc`](../../axis_servo_fault_routing.plc) |  | Servo fault routing fixture. |
| `axis_stepper_fault_routing` | `plc` | [`examples/axis_stepper_fault_routing.plc`](../../axis_stepper_fault_routing.plc) |  | Stepper fault routing fixture. |
| `axis_fault_normal_path` | `plc` | [`examples/axis_fault_normal_path.plc`](../../axis_fault_normal_path.plc) |  | Axis normal-path fault policy fixture. |
| `axis_fault_recoverable_path` | `plc` | [`examples/axis_fault_recoverable_path.plc`](../../axis_fault_recoverable_path.plc) |  | Axis recoverable fault policy fixture. |
| `axis_fault_nonrecoverable_path` | `plc` | [`examples/axis_fault_nonrecoverable_path.plc`](../../axis_fault_nonrecoverable_path.plc) |  | Axis nonrecoverable fault policy fixture. |
| `axis_fault_safety_path` | `plc` | [`examples/axis_fault_safety_path.plc`](../../axis_fault_safety_path.plc) |  | Axis safety fault policy fixture. |
