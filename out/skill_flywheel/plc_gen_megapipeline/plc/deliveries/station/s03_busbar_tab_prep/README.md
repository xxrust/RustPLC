# S03 Busbar Tab Prep Station

## Quick Check

```bash
rust_plc project-check plc/deliveries/station/s03_busbar_tab_prep/plc/main.bundle.toml \
  --scenario plc/deliveries/station/s03_busbar_tab_prep/scenarios/nominal/normal.yaml \
  --out-dir out/project_check/s03 --output human
```

## Focus
- 4+ work positions (buffer, alignment, clamp, transfer, operator gate).
- 10 cylinders (clamps, diverters, pushers) and 8 motors (servo conveyors, feed tables).
- Keeps the cylinder actions at the semantic level (`align_tab`, `tab_attach`, `transfer_out`).
