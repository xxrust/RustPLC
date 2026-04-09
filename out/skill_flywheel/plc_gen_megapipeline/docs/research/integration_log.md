# Integration Log

## Scope
- Build a line-level RustPLC project for a six-station battery module pack line.
- Keep workpiece semantics first-class.
- Keep station ownership explicit so later parallel refinement does not degenerate into one flat PLC.

## Main-Thread Difficulties
- The current scaffold creates one line asset cleanly, but station assets still need to be authored manually inside the same project tree.
- For a >100-actuator line, the practical compile surface is much larger than the minimal runtime proof path; the integration strategy is to keep the full inventory in topology while only exercising representative actuators in the first executable route.
- The worker-authored station docs revealed a missing shared contract for canonical workpiece naming and per-station quick-check launchers.
- The first attempt to re-review the tree with three fresh subagents failed entirely due to stream disconnects before any result returned, so orchestration reliability itself became part of the experiment data.
- The first station validation sweep showed a hidden quality split: S01-S04 were only thin wrappers over shared line fragments, while S05-S06 were outright invalid because raw DSL had been placed inside bundle TOML manifests.

## Resolutions Applied
- The line compile surface was rewritten around six station-owned tasks plus one line supervisor, instead of leaving the default generic scaffold in place.
- Representative cylinders were closed through explicit `relation { from: valve.out, to: cyl.cmd, via: driven_by }` links so scenario validation could resolve a unique physical output path.
- The line start wait now uses a timeout route into `line_fault.line_start_timeout`, which closed the liveness violation.
- The intent contract was moved from placeholder bindings to trace-derived transition bindings after a real `sim-plc` run and `intent-doctor` pass.
- Each station bundle now owns a local `fragments/topology.plcfrag`, `fragments/constraints.plcfrag`, and `fragments/tasks.plcfrag` entry so station `project-check` no longer depends on shared line fragments.
- The station-local canaries were normalized onto the canonical `battery_module_pack` workpiece vocabulary and retain high-level cylinder semantics instead of hand-written sensor choreography.
- The two-cycle line canary now uses a real second ingress source plus a `reserve_loader` task, not a fake duplicated trace fixture.
- The first two-cycle attempt failed because `if: ... else: ...` lowered to `condition + NOT(condition)`, while the runtime bridge only accepts `condition + timeout`, `condition + always`, or `always + timeout`. The fix was to rewrite each station gate as `wait: counter >= 2.0` plus `timeout -> goto entry_window`.
- The second blocker after the gate rewrite was evidence quality, not route logic: the first two-cycle trace window ended before the second `packout_complete`, so intent alignment still only saw one complete cycle. Extending the nominal scenario window and rerunning `sim-plc` produced two real `line_started -> weld_complete -> packout_complete` sequences.
- Overlapping trace-anchor cycle handling was re-checked with dedicated intent-alignment regressions in the source tree so the line canary is now backed by an explicit test, not by assumption.

## Final Validation Snapshot
- `cargo run --bin rust_plc -- out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --no-print-ir`
- `cargo run --bin rust_plc -- scenario-validate out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --output human`
- `cargo run --bin rust_plc -- sim-plc out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl`
- `cargo run --bin rust_plc -- intent-doctor out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --trace out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/line_with_intent --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --intent-evidence out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s01_tray_infeed_buffer/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s01_tray_infeed_buffer/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s01_local --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s02_cell_loading_alignment/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s02_cell_loading_alignment/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s02_local --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s03_busbar_tab_prep/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s03_busbar_tab_prep/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s03_local --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s04_laser_weld_cooling/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s04_laser_weld_cooling/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s04_local --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s05_leak_hipot_vision/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s05_leak_hipot_vision/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s05_local --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s06_label_packout_sort/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/station/s06_label_packout_sort/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/s06_local --output human`
- `cargo test --test intent_alignment_compare -- --nocapture`
- `cargo test overlapping_transition --tests`
- `cargo run --bin rust_plc -- sim-plc out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace_cycle2.jsonl`
- `cargo run --bin rust_plc -- intent-doctor out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --trace out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace_cycle2.jsonl --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/line_with_intent_cycle2 --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --intent-evidence out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace_cycle2.jsonl --output human`

## Open Follow-Up
- Export a public `plc-gen` helper for station asset scaffolding and multi-station routing examples so future workers do not need to infer the same structure manually.
- Add an orchestration reliability note to the flywheel public surface so agent stream disconnects are treated as experiment data instead of silent noise.
