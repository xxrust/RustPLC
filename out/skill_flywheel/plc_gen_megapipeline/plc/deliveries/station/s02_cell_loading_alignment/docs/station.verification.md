# S02 Cell Loading & Alignment Verification

## Required checks
1. `project-check` with station bundle and `scenarios/nominal/normal.yaml`.
2. Scenario validation covering vision misalignment and skid fault (simulate misalignment sensor toggles).
3. Intent alignment verifying `alignment_ready` milestones.

## Assertions
- Vision alignment milestone should not fire unless the servo stage (M03) completes while preload clamps (C03/C04) hold.
- Fault path must cleanly route failed trays to `alignment_reject` without presenting them as ready to S03.
