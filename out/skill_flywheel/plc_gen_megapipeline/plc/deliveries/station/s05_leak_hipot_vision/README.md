# S05 Leak / Hipot / Vision Station

## Quick Facts
- Entry PLC: `plc/main.bundle.toml`
- Nominal scenario: `scenarios/nominal/normal.yaml`
- 9 cylinders: clamp_1..4, seal_5..6, release_7, guard_8, curtain_9.
- 8 motors: rotation_1..2, seal_3..4, hipot_5..6, vision_carousel_7..8.
- Validates `battery_module_pack` pressure, hipot, and vision before handing to S06.

## Validation Snippets
```bash
cargo run --bin rust_plc -- project-check plc/main.bundle.toml --scenario scenarios/nominal/normal.yaml --output json
```
