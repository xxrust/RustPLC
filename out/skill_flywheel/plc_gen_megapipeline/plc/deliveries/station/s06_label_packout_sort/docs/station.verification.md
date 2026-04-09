# S06 Label & Packout Verification

## Required Checks
1. Compile the station bundle to ensure label motors and cylinders are enumerated.
2. Validate the nominal scenario to confirm transitions between alignment, label, UV, and sorting positions exist.
3. Simulate the station to cover both `packout_good` and `packout_reject` flows.
4. Run `intent-doctor` using this station's intent contract and the simulation trace to anchor packing milestones.

## Regression Focus
- Assert that sorting divergence happens only after UV curing passes and `sensor_packout_ready` asserts.
- Confirm rejects release only through `cyl_reject_9` and the carrier returns to `s06_outfeed` with `finish`.
