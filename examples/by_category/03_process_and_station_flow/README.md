# 03 Process And Station Flow

Station sequences, concurrent task flow, and structured project fixtures.

| Example | Kind | Source | Scenario | Purpose |
| --- | --- | --- | --- | --- |
| `three_station_assembly` | `plc` | [`examples/three_station_assembly.plc`](../../three_station_assembly.plc) |  | Multi-station assembly sequence. |
| `welding_station` | `plc` | [`examples/welding_station.plc`](../../welding_station.plc) |  | Welding station sequence and constraints. |
| `load_unload_concurrent_tasks` | `plc` | [`examples/load_unload_concurrent_tasks.plc`](../../load_unload_concurrent_tasks.plc) |  | Concurrent load/unload task fixture. |
| `realtime_stress` | `plc` | [`examples/realtime_stress/stress_case.plc`](../../realtime_stress/stress_case.plc) | [`examples/realtime_stress/scenarios/safe.yaml`](../../realtime_stress/scenarios/safe.yaml) | No-board gate and realtime stress playbook fixture. |
| `project_scaffold_demo` | `project` | [`examples/project_scaffold_demo/plc/main.plc`](../../project_scaffold_demo/plc/main.plc) | [`examples/project_scaffold_demo/scenarios/nominal/normal.yaml`](../../project_scaffold_demo/scenarios/nominal/normal.yaml) | Structured project scaffold reference used by scenario tools. |
