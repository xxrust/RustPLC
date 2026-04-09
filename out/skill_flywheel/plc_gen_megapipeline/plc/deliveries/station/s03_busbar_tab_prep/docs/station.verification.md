# S03 Busbar Tab Prep Verification

## Required Checks
1. Compile `plc/main.bundle.toml` and ensure semantic fragments declare the 10 cylinders / 8 motors without lowering to raw sensors.
2. Run `scenario-validate` against `scenarios/nominal/normal.yaml`.
3. Execute `sim-plc` over the same scenario for three cycles, observing that `tab_ready` transitions from `main_cycle` to `transfer_out`.
4. Run `intent-doctor` from the station bundle plus scenario trace and freeze `busbar_tab_ready` anchors before declaring the asset aligned.
5. Push the asset through `no-board-gate` once the intent alignment evidence is stable.

## Monitoring Focus
- Check that the servo motors never run concurrently with diverter cylinder retractions (resource conflict).
- Validate that pipeline `transfer` effects always sequence `tab_prep_buffer -> tab_prep_clamp -> tab_prep_out`.
