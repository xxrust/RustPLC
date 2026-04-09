# S01 Tray Infeed Buffer System

## Identity
- Station slug: `s01_tray_infeed_buffer`
- Delivery layer: `station`
- Workpiece context: `battery_module_pack`

## Workpiece semantics
- `workpiece battery_module_pack: workpiece_type`
- Ingress site: `tray_pickup`
- Normal egress: `buffer_ready`
- Abnormal egress: `tray_reject`
- Holder `tray_bay`: capacity 2 for interchange buffering

## Flow outline
1. The operator or upstream buffer loads a `battery_module_pack` carrier at `tray_pickup`; `acquire tray_bay battery_module_pack from tray_pickup`.
2. Dual clamp cylinders (C01–C04) seat the tray and engage sensors; motors M01/M02 index the tray to the safe transfer window.
3. A transfer arm (cylinders C05–C08, motor M03) moves the tray into the buffer lane where buffer pistons (C09–C10) align the tray.
4. The clamp readiness actuator suite (C11–C12, motors M04–M06) primes the tray for handoff; `effect: transfer from tray_bay to buffer_ready` once sensors confirm readiness.
5. The station routes faults (timeout, clamp failure) into `fault.reject_tray`.

## Tasks
- `task ingress`: holds trays, runs gate/motor combos, monitors clamp sensors, and forwards ready buffers.
- `task fault`: handles clamp latch failures and timer expiries, transfers trays to `tray_reject`, signals downstream to avoid misfeeds.

## Constraints
- Timeout for clampready is 150ms -> goto `fault.reject_tray`.
- Resource claims: clamp cylinders, transfer motors, buffer sensors, tray picker force.
