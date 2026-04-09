# S04 Laser Weld & Cooling Station

## Quick Check

```bash
rust_plc project-check plc/deliveries/station/s04_laser_weld_cooling/plc/main.bundle.toml \
  --scenario plc/deliveries/station/s04_laser_weld_cooling/scenarios/nominal/normal.yaml \
  --out-dir out/project_check/s04 --output human
```

## Focus
- 4+ positions (laser load, clamps, laser motion, cooling, buffer, gate).
- 8 cylinders handling clamps and transfers, 10 motors controlling laser gantry, conveyors, cooling fans.
- Cylinder actions remain high-level (`clamp_banks`, `transfer_to_cooling`, `release_for_s05`).
