# Topology Abstraction: PLS / Angle / Distance (Draft)

Date: 2026-02-18

This is a repo-local Wiki draft, aligned with:
- `docs/已实现/stepper_ab_encoder.md`
- `docs/已实现/scenario_playbook.md`

## Problem Statement

A single motion chain often exposes multiple coordinates:

- Pulse/count (`axis_count`)
- Angle (`axis_theta`)
- Linear position (`axis_pos_mm`)
- Speed (`axis_speed`)

If all coordinates are treated as equal “truth,” constraints become contradictory and hard to verify.

## Recommended Model: Primary + Derived Coordinates

Use exactly one primary truth coordinate for safety/control closure:

- Typical primary: `axis_count` or `axis_pos_mm`.
- Other coordinates are derived observations for display/diagnostics/signalization.

In RustPLC DSL:
- Prefer safety and interlocks against primary coordinate or derived discrete safety signals.
- Avoid multi-coordinate arithmetic as decision truth in DSL.

## Conversion Boundary

Keep conversion and kinematic logic in the driver/board layer:

- `theta_deg = f(count, ppr, gear_ratio, ...)`
- `pos_mm = g(theta_deg, lead, linkage, LUT, ...)`
- Nonlinear mechanisms should use LUT/piecewise fits in the driver layer.

DSL should consume discrete semantic output signals, not implement conversion math or model raw analog channels as process devices.

## Standard Signal Set

Recommended interface signals for this topology abstraction:

- Driver-internal engineering values: `axis_count`, `axis_theta`, `axis_pos_mm`, `axis_speed`
- DSL-facing discrete signals: `range_valid`, `pos_consistent`, `inpos`, `alarm`
- DSL-facing collision signal: `zone_code` (`off=safe`, `on=collision window`)

These signals let DSL stay verifiable while preserving engineering semantics.

## Consistency Strategy (Encoder vs Distance Sensor)

When both encoder-derived and external distance estimates exist:

- Compare in driver layer (`abs(pos_encoder - pos_laser) <= tol` plus persistence/hysteresis).
- Publish simple DSL-friendly results:
  - `pos_consistent` (bool)
  - optional encoded fault/health code (`sensor_fault_code` / `safety_mode`)

Then in DSL:
- If inconsistent in risk-related context, go `fault`.
- If inconsistent but posture is safe, optionally degrade (restrict motion) and recover only after consistency returns.

## Fault vs Degrade Policy

Suggested policy:

- Go `fault` for alarm conditions, prolonged invalid range data, or inconsistency during dangerous-window operations.
- Allow degrade for transient validity/consistency loss while still in safe posture; restrict to safe retreat/recovery actions.

## Parseable DSL Skeleton

```plc
[topology]
device range_valid: sensor { purpose: "驱动层导出的量程有效信号" }
device pos_consistent: sensor { purpose: "驱动层导出的位置一致性信号" }
device zone_code: sensor { purpose: "驱动层导出的碰撞窗口信号，on 表示危险窗口" }
device move_cmd: digital_output
device cyl_clamp: cylinder

[constraints]
safety: zone_code.on conflicts_with cyl_clamp.extended
safety: move_cmd.on conflicts_with cyl_clamp.extended
safety: move_cmd.on requires range_valid.on
safety: move_cmd.on requires pos_consistent.on

[tasks]
task cycle:
    step hold:
```

## Cross References

- Safety and rule templates: `docs/已实现/stepper_ab_encoder.md`
- Scenario authoring and regression loop: `docs/已实现/scenario_playbook.md`
