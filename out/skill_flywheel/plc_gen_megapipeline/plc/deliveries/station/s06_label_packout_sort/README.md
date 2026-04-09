# S06 Label & Packout Station

## Quick Facts
- Entry PLC: `plc/main.bundle.toml`
- Scenario: `scenarios/nominal/normal.yaml`
- 9 cylinders across alignment, label head, UV guard, and reject diverter.
- 8 motors for conveyors, label head, UV lamp, and sorter.
- Station produces the final packout and handles rejects explicitly.

## Quick Command
```bash
cargo run --bin rust_plc -- project-check plc/main.bundle.toml --scenario scenarios/nominal/normal.yaml --output human
```
