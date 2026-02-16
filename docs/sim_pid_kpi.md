# PID KPI Simulation (US-010)

`sim-pid-kpi` runs a no-board closed-loop PID simulation and exports KPI JSON.

## Command

```bash
rust_plc sim-pid-kpi examples/pid_loop.plc \
  --scenario examples/pid_kpi_scenario.yaml \
  --out out/pid_kpi.json
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

