# Battery Module Pack Line System

## Identity
- Project: Plc Gen Megapipeline
- Delivery layer: `line`
- Product: `battery_module_pack`
- Station count: 6
- Work positions per station: 4
- Total cylinders: 58
- Total motors: 51
- Total cylinders and motors: 109

## Serial Manufacturing Route
1. S01 tray infeed and buffer
2. S02 cell loading and alignment
3. S03 busbar and tab preparation
4. S04 laser weld and cooling
5. S05 leak, hipot, and vision inspection
6. S06 label, packout, and sort

## Workpiece Contract
- The line always models real part flow with first-class workpiece semantics.
- The canonical ingress is `line_infeed`.
- The normal terminal site is `line_packout` with terminal state `packed`.
- The abnormal terminal site is `line_reject` with terminal state `rejected`.
- Each station owns one line-level handoff boundary and four internal work positions.

## Parallel Execution Direction
- The process route is serial.
- The authoring and delivery structure is parallel by station.
- The runtime proof path uses one task per station so ownership boundaries remain visible.
- Station internals stay station-local; the line only freezes handoff boundaries and escalation policy.

## Fault Direction
- Line start timeout routes into a dedicated line fault task.
- Station-local abnormal routes remain station-owned and should be refined in station assets rather than flattened into one global fault script.
