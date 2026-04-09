# Knowledge Accumulation

## Purpose
This file accumulates integration pain points from the main thread and the station workers.

## Worker-Derived Knowledge
- Worker A hit a `skill-gap` because there is still no exported station doc template with actuator-count sections.
- Worker B hit a `public-surface-gap` because delivery-layer-specific quick-check commands are not exported as a stable helper artifact.
- Worker C hit a `skill-gap` around station directory seeding and a `public-surface-gap` around cross-station workpiece routing examples.

## Main-Thread Integration Notes
- The line was intentionally planned as one serial route before station authoring to avoid station-level workpiece drift.
- Compile-surface fragments keep ownership in file names so the project does not collapse back into spaghetti PLC.
- Worker station docs currently use inconsistent workpiece labels (`tray_module_pack`, `battery_module`, and line-level `battery_module_pack`), which is now a documented alignment gap to fix in the next refinement round.
- Only a subset of the >100 actuators is exercised in the first line-level runtime proof path; the full actuator inventory is still declared and documented for delivery realism.

## Toolchain Findings
- `cargo run --bin rust_plc -- ...main.bundle.toml --no-print-ir` initially failed because `wait` without timeout is rejected by liveness on autonomous starts; the fix was to add an explicit timeout route for the line start wait.
- `scenario-validate` initially failed because the representative cylinders had no unique physical output path; the correct closed-loop topology is `plc_main.Y -> valve.coil` and `valve.out -> cyl.cmd` via `relation`, not the deprecated `driven_by:` property.
- The first intent contract draft failed schema validation because `postconditions` only accept `postcondition_id` and `description`; carrying a free-form `label` field is rejected by the parser.
- The first intent-alignment run was blocked even after schema fix because the contract still used placeholder bindings. `intent-doctor` produced concrete transition anchors, and binding those real transitions allowed `project-check` to pass with `intent_alignment`.

## Current Evidence
- Line bundle compile/verify: pass.
- Line scenario validation: pass.
- Line `project-check` without intent evidence: pass.
- Line `intent-doctor`: pass, with stable bindings for `line_started`, `weld_complete`, and `packout_complete`.
- Line `project-check` with intent evidence: pass.

## Remaining Gaps
- Cross-cycle diagnosis remains weak because the current nominal scenario only covers one complete cycle.
- Station asset docs exist, but the line and station layers still need a shared canonical workpiece vocabulary so the delivery tree stops drifting on names.
- The current skill still lacks a dedicated public artifact for multi-station routing and station directory seeding; those were exposed by all three workers independently, so they are now strong candidates for the next `plc-gen` flywheel round.
