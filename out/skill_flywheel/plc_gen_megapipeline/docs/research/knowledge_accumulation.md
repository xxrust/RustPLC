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
- The first station authoring round exposed a real independence trap: S01-S04 passed because their bundle entries still pointed at shared line fragments, which is not the same thing as a station-local executable canary.
- Workpiece naming drift was real across worker assets; the corrective rule is to keep the line-level `battery_module_pack` type stable and express station progress via terminal states, not ad-hoc type renames.
- Only a subset of the >100 actuators is exercised in the first line-level runtime proof path; the full actuator inventory is still declared and documented for delivery realism.
- Three follow-up explorer subagents all failed with stream-disconnect before returning analysis. This is an orchestration-layer failure mode that must be recorded separately from PLC authoring quality.

## Toolchain Findings
- `cargo run --bin rust_plc -- ...main.bundle.toml --no-print-ir` initially failed because `wait` without timeout is rejected by liveness on autonomous starts; the fix was to add an explicit timeout route for the line start wait.
- `scenario-validate` initially failed because the representative cylinders had no unique physical output path; the correct closed-loop topology is `plc_main.Y -> valve.coil` and `valve.out -> cyl.cmd` via `relation`, not the deprecated `driven_by:` property.
- The first intent contract draft failed schema validation because `postconditions` only accept `postcondition_id` and `description`; carrying a free-form `label` field is rejected by the parser.
- The first intent-alignment run was blocked even after schema fix because the contract still used placeholder bindings. `intent-doctor` produced concrete transition anchors, and binding those real transitions allowed `project-check` to pass with `intent_alignment`.
- Raw DSL pasted directly into `main.bundle.toml` is rejected by the loader; delivery assets must keep PLC source in fragment files and use the bundle only as a manifest.
- Station-local nominal canaries can stay minimal, but they must still preserve workpiece semantics and high-level actuator actions. A thin local source set is acceptable; a fake wrapper over line fragments is not.
- For repeated production cycles, a real second workpiece source is better evidence than synthetic trace duplication. The executable fix was `line_infeed_reserve` plus `reserve_loader`, not contract-side cheating.
- In RustPLC today, task-step `if: ... else: ...` lowers to two guarded transitions: `condition` and `NOT(condition)`. That shape is valid semantically but is not runtime-lowerable in the current bridge path. For cycle gates, the stable authoring pattern is `wait: terminal_condition` plus `timeout -> goto retry_step`.
- For repeated production cycles, line intent evidence is only as good as the trace window. A real second ingress plus a short scenario still yields only one complete observed cycle if the second packout lands after the scenario ends.
- Overlapping transition-anchor cycle handling is now covered by dedicated intent-alignment regressions in `tests/intent_alignment_observed.rs` and `tests/intent_alignment_compare.rs`, so line-level canaries are no longer relying on implicit comparator behavior.

## Current Evidence
- Line bundle compile/verify: pass.
- Line scenario validation: pass.
- Line `project-check` without intent evidence: pass.
- Line `intent-doctor`: pass, with stable bindings for `line_started`, `weld_complete`, and `packout_complete`, `cycles=2`, `cross_cycle_ready=true`, and `trailing_partial_cycle=false`.
- Line `project-check` with intent evidence: pass for the two-cycle canary at `line_with_intent_cycle2`.
- `cargo test overlapping_transition --tests`: pass, covering both observed extraction and compare behavior for overlapping transition-anchored cycles.

## Remaining Gaps
- The current skill still lacks a dedicated public artifact for multi-station routing and station directory seeding; those were exposed by all three workers independently, so they are now strong candidates for the next `plc-gen` flywheel round.
- The line canary now proves two overlapping cycles, but it still relies on milestone occurrence order rather than explicit workpiece instance ids. If future lines allow overtaking or branch-specific per-piece divergence, intent alignment will need instance-level anchors instead of FIFO-by-occurrence inference.
