# Testing Inventory Matrix (US-013)

## Layer Coverage Matrix

| Layer | Representative suites | Main fixtures / samples | Coverage focus |
| --- | --- | --- | --- |
| Parser | `src/parser/mod.rs` unit tests, `tests/large_demo_coverage.rs` | inline DSL snippets, `examples/error_all_verifiers.plc` | grammar parsing, deprecated keyword migration, line/column diagnostics |
| Semantic | `src/semantic/mod.rs`, `tests/component_topology_validate.rs` | inline DSL MIMO graph, `examples/assembly_station.plc` | topology direction (`producer -> consumer`), constraints/causality checks, component contract validation |
| Runtime | `tests/runtime_bridge_us006.rs`, `tests/component_sim.rs`, `tests/sim_regress.rs` | `examples/assembly_station.plc`, runtime scenarios under `scenarios/` | runtime bridge behavior, tick execution semantics, regression traces |
| API / CLI contract | `crates/web-server/src/main.rs` tests, `tests/scenario_validate.rs`, `tests/scenario_init.rs` | `examples/two_cylinder.plc`, `examples/assembly_station.plc` | parse-plc payload schema, scenario init/validate contract, relation/port metadata consistency |
| UI | `web-ui` static checks (`npm run build`, `npm run lint`) + browser verification logs in `progress.txt` | topology demo canvas, tag/port workflows | type/lint gate for UI code + browser-based interaction regressions for topology UX |

## Key Regression Set Retained

| Regression set | Current coverage entry point |
| --- | --- |
| `two_cylinder` | `tests/examples_integration.rs` (`parses_two_cylinder_example_into_verified_ir_json`), `tests/scenario_init.rs`, `tests/scenario_validate.rs` |
| `assembly_station` | `tests/scenario_gen.rs`, `tests/scenario_expand.rs`, `tests/scenario_validate.rs`, `tests/large_demo_coverage.rs` |
| MIMO (multi-input/multi-output topology) | `src/semantic/mod.rs` (`supports_mimo_edges_in_producer_to_consumer_direction`) |

## Parameterization Refactor in This Iteration

- Consolidated duplicated `scenario-validate` failure tests into one table-driven case matrix in `tests/scenario_validate.rs`.
- Converted success-path validation into a table-driven loop over `assembly_station` and `two_cylinder` presets to reduce duplicated command wiring and keep fixture coverage explicit.
