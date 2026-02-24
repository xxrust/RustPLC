# PID Minimal Loop (US-009)

This repo supports declaring a minimal PID control loop in the PLC DSL without a physical board.

## DSL Declaration

Declare PID loops as `device <name>: pid { ... }` inside `[topology]`:

```plc
[topology]
device AI0: analog_input { range: 0..100, unit: "bar", external: true }
device AO0: analog_output { range: 0..100, unit: "%"}

device loop_pressure: pid {
    pv: AI0,
    sp: 50bar,
    kp: 2.0,
    ki: 0.4,
    kd: 0.05,
    out: AO0,
    period_ms: 100,
    limit: 0..100
}
```

Fields:
- `pv`: `analog_input` device name.
- `sp`: setpoint numeric literal (`number` or `measured_value` like `50bar`).
- `kp/ki/kd`: numeric gains.
- `out`: `analog_output` device name.
- `period_ms`: sampling period in milliseconds (must align to `tick_ms` at runtime).
- `limit`: output clamp range (`min..max`), and must be within the output device's declared `range`.

## Runtime Semantics

PID loops are executed deterministically once per tick (when the loop is due) before the state
machine steps are evaluated. Task actions may still override the output in the same tick.

Output is always clamped to `limit`.

### Anti-windup Strategy

Current implementation uses **conditional integration** ("integrator clamping"):
- If the controller output is saturated and the error would push it further into saturation,
  the integrator is not updated for that cycle.

This prevents integral windup while keeping the controller deterministic and low-overhead.

