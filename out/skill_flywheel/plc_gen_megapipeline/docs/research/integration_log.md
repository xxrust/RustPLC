# Integration Log

## Scope
- Build a line-level RustPLC project for a six-station battery module pack line.
- Keep workpiece semantics first-class.
- Keep station ownership explicit so later parallel refinement does not degenerate into one flat PLC.

## Main-Thread Difficulties
- The current scaffold creates one line asset cleanly, but station assets still need to be authored manually inside the same project tree.
- For a >100-actuator line, the practical compile surface is much larger than the minimal runtime proof path; the integration strategy is to keep the full inventory in topology while only exercising representative actuators in the first executable route.
- The worker-authored station docs revealed a missing shared contract for canonical workpiece naming and per-station quick-check launchers.

## Resolutions Applied
- The line compile surface was rewritten around six station-owned tasks plus one line supervisor, instead of leaving the default generic scaffold in place.
- Representative cylinders were closed through explicit `relation { from: valve.out, to: cyl.cmd, via: driven_by }` links so scenario validation could resolve a unique physical output path.
- The line start wait now uses a timeout route into `line_fault.line_start_timeout`, which closed the liveness violation.
- The intent contract was moved from placeholder bindings to trace-derived transition bindings after a real `sim-plc` run and `intent-doctor` pass.

## Final Validation Snapshot
- `cargo run --bin rust_plc -- out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --no-print-ir`
- `cargo run --bin rust_plc -- scenario-validate out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --output human`
- `cargo run --bin rust_plc -- sim-plc out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl`
- `cargo run --bin rust_plc -- intent-doctor out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --trace out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --output human`
- `cargo run --bin rust_plc -- project-check out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out-dir out/skill_flywheel/plc_gen_megapipeline/out/project_check/line_with_intent --intent-contract out/skill_flywheel/plc_gen_megapipeline/plc/deliveries/line/plc_gen_megapipeline/docs/line.intent_alignment.contract.json --intent-evidence out/skill_flywheel/plc_gen_megapipeline/out/sim/megapipeline_trace.jsonl --output human`

## Open Follow-Up
- Add a second-cycle nominal scenario so `intent-doctor` can evaluate cross-cycle readiness with stronger evidence.
- Normalize station workpiece names to the same canonical line contract.
- Export a public `plc-gen` helper for station asset scaffolding and multi-station routing examples so future workers do not need to infer the same structure manually.
