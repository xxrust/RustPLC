# PID KPI Simulation (US-010)

`sim-pid-kpi` runs a no-board closed-loop PID simulation and exports KPI JSON.
The command is still available, but the historical `examples/pid_loop.plc` fixture is no longer the canonical topology example for current semantic-gate rules. Treat the help contract below as authoritative.

## Command

```bash
cargo run --release -- help sim-pid-kpi
```

Current command contract:

```text
sim-pid-kpi <source.plc|source.bundle.toml> --scenario <pid_scenario.yaml> --out <kpi.json>
```

## Scenario YAML

```yaml
tick_ms: 100
duration_ms: 10000
loop_index: 0
initial_pv: 0.0
model:
  kind: first_order
  gain: 1.0
  tau_ms: 1200
```

Supported models:
- `first_order`: `gain`, `tau_ms`
- `dead_time_first_order`: `gain`, `tau_ms`, `dead_time_ms`

## Output (`kpi.json`)

Fields:
- `schema_version`
- `tick_ms`, `duration_ms`
- `loop_index`
- `model` (echo from scenario)
- `setpoint`
- `samples`
- `kpi`:
  - `overshoot_percent`
  - `settling_time_ms` (`null` means not settled in horizon)
  - `steady_state_error`
