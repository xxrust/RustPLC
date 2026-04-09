# S05 Leak / Hipot / Vision Architecture

## Responsibility
- Station `s05_leak_hipot_vision` isolates all high-voltage validation, so downstream stations do not need to repeat hipot or leak tests.
- It claims the `s05_infeed` slot and releases only after `verified` state.

## Work Positions
1. **Load & Clamp** – four leak-shield clamps (`cyl_clamp_1`..`cyl_clamp_4`) align the module and mount the carrier.
2. **Seal Chamber** – dual pneumatically actuated jaws (`cyl_seal_5`..`cyl_seal_6`) and two sealing servos ensure vacuum integrity.
3. **Hipot Excitation** – the hipot source uses motors `motor_hipot_1`..`motor_hipot_6` to ramp voltage and monitor `sensor_hipot_ready`.
4. **Vision / Release** – the vision carousel rotates with `motor_rotation_7`..`motor_rotation_8` while cylinder `cyl_release_7` handles egress alignment.

Each position is modeled as a sequential task step; concurrency between leak and vision is managed by explicit `on_complete` arrows.

## Interfaces
- Exposes `line_leak_gate` to receive modules and `line_out_gate` to pass validated modules to S06.
- Reports `status_ok`, `status_leak_fail`, and `status_vision_fail` signals at the station interface for the line supervisor.

## Fault Domains & Resource Claims
- Any hipot excursion over threshold transitions to `fault.hipot_trip`.
- Leak test failure uses `fault.leak_fault`.
- Cylinder and motor clusters extend the `resource_leak_station` claim to avoid double booking.
