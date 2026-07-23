# 06 Performance And Deployment Fixtures

Large topology, component topology, and PIL baseline fixtures.

| Example | Kind | Source | Scenario | Purpose |
| --- | --- | --- | --- | --- |
| `topology_perf_500` | `plc` | [`examples/topology_perf_500.plc`](../../topology_perf_500.plc) | [`examples/topology_perf_500.scenario.json`](../../topology_perf_500.scenario.json) | Large topology performance fixture. |
| `pil_case_timeout` | `plc` | [`examples/pil_baselines/case_timeout/case.plc`](../../pil_baselines/case_timeout/case.plc) | [`examples/pil_baselines/case_timeout/scenarios/base.yaml`](../../pil_baselines/case_timeout/scenarios/base.yaml) | PIL/Renode timeout baseline. |
| `component_model` | `component_topology` | [`examples/component_model/topology.json`](../../component_model/topology.json) | [`examples/component_model/scenario_normal.json`](../../component_model/scenario_normal.json) | Component topology and scenario fixture set. |
