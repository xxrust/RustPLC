# Battery Module Pack Line Verification

## Minimum Checks
1. Compile the line asset bundle.
2. Validate the nominal line scenario.
3. Run `sim-plc` to produce a line trace.
4. Run `project-check` with the line intent contract when the station docs are integrated.
5. Use station-level bundles for independent station checks.

## Commands
```bash
cargo run --bin rust_plc -- plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --no-print-ir
cargo run --bin rust_plc -- scenario-validate plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --output human
cargo run --bin rust_plc -- sim-plc plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out out/sim/megapipeline_trace.jsonl
```
