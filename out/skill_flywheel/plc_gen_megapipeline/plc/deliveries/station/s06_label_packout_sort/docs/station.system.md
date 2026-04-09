# S06 Label & Packout System

## Identity
- Station slug: `s06_label_packout_sort`
- Delivery layer: `station`
- Consumes validated modules from S05 and feeds the line outfeed.

## Workpiece Semantics
- `workpiece battery_module_pack` enters at `s06_infeed`, traverses label/packout positions, and leaves via `s06_outfeed`.
- The station owns the carrier `station_carrier` and uses `effect: transfer` and `effect: finish` to make the module state explicit.
- Every labeling step ends with `effect: finish workpiece at s06_outfeed as validated`, so a downstream station cannot skip the final release.

## Work Positions
1. **Alignment & Staging** – the inbound clamps (`cyl_align_1..4`) secure the module and align sensor references.
2. **Label Application** – label head cylinders (`cyl_label_5`, `cyl_label_6`) and motors (`motor_label_4`, `motor_label_5`) coordinate to print and place pack labels.
3. **UV Curing / Inspection** – UV lamp motor (`motor_uv_6`) and curing guards (`cyl_guard_7`) ensure adhesives cure before packout.
4. **Packout Sorting** – roller motors (`motor_sort_7`, `motor_sort_8`) and reject cylinder (`cyl_reject_9`) handle diverging good/reject modules.

## Architecture Intent
- Station isolates all label quality checks, deferring sorting decisions to this station alone.
- All motors and cylinders are claimed under `resource_label_station` to prevent upstream modules from preempting them.
