# S01 Tray Infeed Buffer Verification

## Required checks
1. `cargo run --release --bin rust_plc -- project-check plc/deliveries/station/s01_tray_infeed_buffer/plc/main.bundle.toml --scenario plc/deliveries/station/s01_tray_infeed_buffer/scenarios/nominal/normal.yaml --out-dir out/project_check/s01 --output human`
2. Scenario validation (`scenario-validate`) with nominal inputs and buffer/clamper fault cases.
3. Intent alignment run targeting `docs/station.intent_alignment.contract.json`.
4. Safety and liveness checks must ensure clamp resource claims hold while transfer arm actions run in parallel.

## Observables to assert
- Buffer_precharge state never overlaps with clamp_release while `battery_module_pack` is mounted.
- Fault path covers clamp timeout and transfer motor fault, routing to `tray_reject`.
