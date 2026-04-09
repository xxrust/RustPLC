# Plc Gen Megapipeline

## Project Identity

- Project slug: `plc_gen_megapipeline`
- Manifest: `rustplc.project.toml`
- Source layout: `bundle + semantic fragments`
- Delivery layer: `line`

## Project Layout

- Authoritative asset system doc: `plc/deliveries/line/plc_gen_megapipeline/docs/line.system.md`
- Default asset PLC entry: `plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml`
- Default asset scenario: `plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml`
- `plc/main.target_semantics.bundle.toml`: aggregate compile surface
- `plc/target_semantics_fragments/`: semantic fragment tree
- `plc/deliveries/`: delivery-layer assets with their own docs, source entries, and scenarios
- `plc/target_semantics_fragments/io|manual|operator_interface|optimization|step/`: authored sidecar semantics kept outside the default compileable bundle when needed
- `config/workpiece.toml`: project workpiece policy
- `config/io_map.toml`: deployment I/O mapping
- `config/retain.toml`: retain baseline
- `out/`: generated artifacts

## Quick Start

```bash
cargo run --release --bin rust_plc -- project-check plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml --scenario plc/deliveries/line/plc_gen_megapipeline/scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
```
