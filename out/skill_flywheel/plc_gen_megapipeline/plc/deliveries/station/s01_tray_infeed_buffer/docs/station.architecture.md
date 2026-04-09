# S01 Tray Infeed Buffer Architecture

## Roles & Composition
- Module composition: tray conveyor handoff, clamp subsystem, transfer arm, buffer lane, safety gate.
- Work positions:
  1. `tray_gate_position`: transparent gate position with cylinder pair C01/C02 and gate motor M01.
  2. `buffer_lane`: pneumatically actuated microbuffer using cylinders C09–C10 plus motorized belt drive M02.
  3. `clamp_ready_zone`: clamp actuators C03–C04 and servo-indexer M03 keep tray faces aligned.
  4. `transfer_arm_center`: dual-arm cylinders C05–C08 plus motor M04 coordinate tray handoff to station S02.

## Actuator inventory
- Cylinders:
  - `C01/C02`: gate open/close
  - `C03/C04`: clamp deployment/relief
  - `C05/C06`: transfer arm lift/lower
  - `C07/C08`: transfer arm extend/retract
  - `C09/C10`: buffer align pistons
  - `C11/C12`: clamp readiness/unload
- Motors:
  - `M01`: gate servo
  - `M02`: buffer belt drive
  - `M03`: clamp indexer
  - `M04`: transfer arm swivel
  - `M05`: pickup head priming
  - `M06`: safety vent pump
  - `M07`: tray sensor drive
  - `M08`: data curtain

## Interfaces
- Consumers: Station S02 uses `buffer_ready` location as ingress.
- Provides: `battery_module_pack` holder state and explicit `effect: transfer` semantics.
- Shares topology fragments from `plc/target_semantics_fragments` for devices, constraints, and auto tasks.

## Validation zones
- Each work position runs in parallel tasks with soft interlocks (clamp/perimeter). Timeout results map to the fault domain and handshake output.
