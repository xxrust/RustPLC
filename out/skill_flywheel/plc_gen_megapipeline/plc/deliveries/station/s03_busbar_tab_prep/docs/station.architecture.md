# S03 Busbar Tab Prep Architecture

## Role
- Station-level asset that finalizes tab geometry and pre-aligns subassemblies before laser welding.
- Primary consumers: S02 alignment outputs. Primary providers: S04 laser/weld cooling.

## Modules
- **Receiving Conveyor Module**: handles `transfer` from S02 via pneumatic diverters plus two intake cylinders.
- **Tab Alignment Module**: two servo roll tables and three clamp cylinders that work simultaneously to stabilize the busbar while tabs are driven.
- **Tab Attachment Module**: combines the pick & place cylinder module with the tab gun motor controller to complete the tab prep action while leaving the high-level result documented (`tab_ready`).
- **Transfer Conveyor Module**: a dual-axis servo motor pair with pneumatic discharger to route the prepared busbar to S04.

## Workpiece Flow
- Flow is linear: `infeed_s02 -> tab_prep_buffer -> tab_prep_clamp -> tab_prep_out`.
- `tab_prep_buffer` collects up to two carriers; `tab_prep_clamp` and `tab_prep_out` each capacity 1.
- Workpiece effect sequence: `acquire` from buffer, `transfer` through clamp, `transfer` to `tab_prep_out`.
- Workpiece terminates when `effect: finish workpiece at tab_prep_out as tab_ready`.

## Concurrency & Supervision
- Supervisor task ensures that buffer replenishment, clamp action, and transfer conveyor stay synchronized via `parallel` steps guarded by the `tab_ready` semantic result set.
- Shared resources: diverter cylinders and pick head cylinder must not operate when a tab is loaded (resource claim enforced by semantics).
- Fault handling isolates the `motion_timeout` branch for each cylinder action without serializing the rest of the station tasks.

## Higher-layer Contracts
- Exposes milestone `busbar_tab_ready` before handing off to S04.
- Requires S04 to observe `pre_weld_ready` before pulling the carrier.
