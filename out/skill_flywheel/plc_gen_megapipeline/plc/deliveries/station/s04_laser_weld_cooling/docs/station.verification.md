# S04 Laser Weld & Cooling Verification

## Required Checks
1. Compile `plc/main.bundle.toml` ensuring the clamp cylinders and servo motors exist in topology without scattering sensor waits.
2. Run `scenario-validate` using `scenarios/nominal/normal.yaml`.
3. Execute `sim-plc` and confirm `weld_ready` and `cooling_ready` results emit sequential transitions, not sensor choreography.
4. Run `intent-doctor` targeting `weld_cooled` as the anchor milestone and freeze bindings before marking the asset aligned.
5. Include the asset in `no-board-gate` once the cooling trace is stable.

## Monitoring Focus
- Observe that `cooling_ready` cannot assert true until the fan speed motor and conveyor motors pass their semantic checks.
- Confirm that the clamp module releases before transfer, avoiding false sensor gating.
