# S04 Laser Weld & Cooling Architecture

## Role
- Station-level asset that welds tabs, cools the assemblies, and presents them to S05 for inspection.

## Modules
- **Laser Gantry Module**: laser controller, gantry servo, and motion profile for `weld_tabs`.
- **Clamp Module**: eight cylinders arranged in two banks, claiming the `clamp_array` resource.
- **Cooling & Buffer Module**: cooling fans, servo conveyors, and transfer cylinders that protect the weld before handoff.

## Workpiece Flow
- Flow strictly sequences: `s04_infeed -> laser_chamber -> cooling_run -> s05_buffer`.
- Each workpiece acquires a `weld_ready` result from the Laser Gantry Module and releases `weld_cooled` to S05.
- Transfers are priced to avoid manual sensor choreography; each `effect` is high-level.

## Concurrency & Supervisory Guarantees
- Laser and cooling sequences are `parallel` guarded by `weld_ready` and `cooling_ready` milestones.
- Transfer cylinder cannot actuate until `cooling_ready` is true and `clamp_array` is released.
- Shared resources include clamp cylinders and the buffer servo; semantic claims ensure they never race.

## Upward Contracts
- Exposes `weld_cooled` milestone to the S05 station.
- Consumes `tab_ready` milestone from S03.
