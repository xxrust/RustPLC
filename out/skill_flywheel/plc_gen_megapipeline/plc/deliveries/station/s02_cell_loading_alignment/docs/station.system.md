# S02 Cell Loading & Alignment System

## Identity
- Station slug: `s02_cell_loading_alignment`
- Delivery layer: `station`
- Workpiece: `battery_module_pack`

## Workpiece semantics
- `workpiece battery_module_pack: workpiece_type`
- Ingress: `alignment_ready`
- Normal egress: `cell_loaded`
- Abnormal egress: `alignment_reject`
- Holder `alignment_table`: capacity 1 for focus alignment, `alignment_buffer`: capacity 2 for pre-load.

## Flow
1. Receives `battery_module_pack` at `alignment_ready` and runs `effect: acquire alignment_table battery_module_pack from alignment_ready`.
2. Preloads modules using preload clamps (C01/C02) and servo guidance (M01) to bring cells into reference positions.
3. Vision alignment stage runs (cylinders C03–C06, motors M02–M04) in parallel with skid actuators; only when both axis and normal results confirm readiness does the task emit `alignment_ready`.
4. Transfer to output table uses cylinders C07–C10 and motors M05–M06, finishing with `effect: transfer from alignment_table to cell_loaded`.
5. Faults include vision misalignment, release failure, or cycle timeout -> goto `fault.reject_alignment`.
