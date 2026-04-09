# S05 Leak / Hipot / Vision Verification

## Required Checks
1. Compile the station bundle and ensure lookup of leak/hipot/vision devices succeeds.
2. Run `scenario-validate` with the nominal scenario to ensure all tasks reference defined sensors and actuators.
3. Run `sim-plc` to verify that the state machine has a clean `validated` path and identifies `hipot_trip`/`leak_fault` failures.
4. Run `intent-doctor` with the station intent contract against the scenario trace to demonstrate milestone anchoring.

## Regression Focus
- Verify that the carrier resource is acquired before any cylinder action and released only after both hipot and vision steps complete.
- Check that the `fault` task only activates when the hipot target is missed or a leak sensor trips.
