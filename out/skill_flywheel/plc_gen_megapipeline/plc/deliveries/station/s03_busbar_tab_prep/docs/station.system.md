# S03 Busbar Tab Preparation Station

## Identity
- Station slug: `s03_busbar_tab_prep`
- Delivery layer: `station`
- Workpiece focus: battery busbar tab subassembly for the pack line

## Process Intent
1. Receive subassemblies from the S02 Cell Loading Alignment station and buffer them on the transfer conveyor.
2. Feed each subassembly into the dual-tab rail with five pneumatic clamping zones and start swept servo tab feed.
3. Align the tab stock using the four-axis servo roll table while the laser height sensor confirms registration (semantic action: `align_tab`).
4. Close the tab clamp cluster, extend the pick cylinder to hold the tab head, and energize the precision tab gun to affix the tab semi-permanently.
5. Retract the clamp and hand off the prepared subassembly to the S04 Laser Weld & Cooling station carrier.

## Work Positions
- Receiving & Buffer (position 1): belt conveyor with two pneumatic diverters.
- Tab Stock Centering (position 2): dual clamps plus alignment servo roll.
- Automatic Tab Clamp (position 3): cluster of three cylinders that simultaneously clamp tab stock.
- Pick & Place Transfer (position 4): nose assembly with dedicated cylinder for blade insertion.
- Transfer Conveyor (position 5): servo-driven carrier that routes to S04.
- Operator Inspection Gate (position 6): manual confirmation before line release.

## Workpiece Semantics
- Workpiece `battery_module_pack: workpiece_type` remains the line-level part identity while S03 promotes it into the `tab_prepared` state.
- Locations: `infeed_s02`, `tab_prep_buffer`, `tab_prep_clamp`, `tab_prep_out`. Each location has `capacity: 2`.
- Handoffs: `effect: transfer from tab_prep_buffer to tab_prep_clamp` and `effect: transfer from tab_prep_out to s04_infeed`.
- The station owns `effect: acquire holder tab_grip from tab_prep_buffer` as part of the cycle semantics.

## Actuators
- 10 pneumatic cylinders (clamp zones, transfer pushers, diverters).
- 8 servo/globally synchronous motors (dual tab feed servos, transfer conveyors, alignment roll table, discharge servo).

## Fault Strategy
- Each pneumatic action is accompanied by a high-level `timeout` branch (e.g., `align_tab` -> `goto fault.tab_misalign`).
- If tab stock misaligns twice within a cycle, escalate to the Fault task and release the carrier for manual reset.
