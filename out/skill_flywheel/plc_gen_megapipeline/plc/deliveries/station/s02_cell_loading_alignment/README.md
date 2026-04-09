# S02 Cell Loading & Alignment Station

- Delivery layer: `station`
- Identity: `s02_cell_loading_alignment`
- Role: takes buffered `battery_module_pack` inputs, positions modules, aligns edges, and confirms clamp readiness for welding.
- Work positions: 1) Tray intake, 2) Module preload, 3) Alignment table, 4) Vision alignment lane, 5) Safety release.
- Actuator inventory: 10 cylinders (cam lift, preload clamps, alignment jacks, sight flippers) and 9 motors (servo aligner, vision stage, preload jaws, belt, sensor sweep).

The station exposes `alignment_ready` and `cell_loaded` as workpiece routing hooks for the downstream station, maintaining explicit workpiece semantics instead of sensor choreography.
