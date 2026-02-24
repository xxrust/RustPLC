# Topology Performance Baseline (500 Nodes / 2000 Edges)

This baseline guards topology scale regressions across three key paths:

- `parse_validate`: `component-topology-validate` latency on the large fixture
- `compile_simulate`: `component-sim` latency on the large fixture + scenario
- `render_transform`: frontend canvas-shape transformation latency (`toCanvasTopology` equivalent)

## Baseline Fixtures

- Topology: `examples/topology_perf_500.topology.json`
- Scenario: `examples/topology_perf_500.scenario.json`
- UI selector shim: `examples/topology_perf_500.plc` (project id anchor for Web UI)

The fixture is intentionally deterministic and fixed-size:

- `components = 500`
- `connections = 2000`

## Run Locally

```bash
python3 scripts/topology_perf_gate.py --output human
```

JSON output (for automation):

```bash
python3 scripts/topology_perf_gate.py --output json
```

## CI Thresholds

Threshold config:

- `scripts/perf/topology_perf_thresholds.json`

Current p95 guardrails (ms):

- `parse_validate <= 250`
- `compile_simulate <= 400`
- `render_transform <= 80`

If any p95 exceeds threshold, the script exits non-zero and emits GitHub warning annotations.
