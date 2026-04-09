# S06 Label & Packout Architecture

## Responsibility
- Guarantees that only modules with passed leak/hipot/vision move to final labeling and sorting, preventing downstream rework.
- Publishes `line_label_ready` and `line_packout_ok` to the line supervise.

## Composition
- Four major work positions are explicitly modeled so they can run in parallel if needed:
  1. Align clamps for inbound accuracy.
  2. Label placement and verification using dedicated servos.
  3. UV curing/inspection to ensure adhesives and inks set.
  4. Sorting diverter with reject logic before the final sink.
- Each position is mapped to distinct resource groups for cylinders/motors.

## Interlocks
- Label motors (`motor_label_4`, `motor_label_5`) wait for `sensor_label_ready`.
- UV curing cannot start until `sensor_uv_ready` and label signals clear.
- Packout rejects (when `sensor_reject` trips) use `cyl_reject_9` to divert mod into `reject_lane`.
