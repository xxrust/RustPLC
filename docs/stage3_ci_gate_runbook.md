# Stage-3 CI Gate Runbook

Date: 2026-02-19

This runbook documents local reproduction commands for the Stage-3 runtime/dev gates.

## 1) One-shot local gate

Run the same contract gate bundle used by CI:

```bash
scripts/stage3_runtime_dev_gate.sh
```

This command covers:

- `online_force_control_plane` contract
- `online_variable_control_plane` contract
- `retain_persistent` contract
- `scenario_gen` contract (including `summary.json` dry-run schema)
- `sim_regress` contract (including `feedback.json` schema)
- `new_scaffold` bootstrap contract
- `abnormal_exit_matrix` + `abnormal_exit_matrix_doc` contracts (Class-D manual evidence workflow)

## 2) Manual breakdown (if one-shot fails)

### A. Focused tests

```bash
cargo test --test online_force_control_plane --test online_variable_control_plane --test retain_persistent --test scenario_gen --test sim_regress --test new_scaffold
cargo test --test abnormal_exit_matrix --test abnormal_exit_matrix_doc
```

### B. scenario-gen schema check

```bash
cargo run -- scenario-gen \
  --plc examples/assembly_station.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir out/stage3_scenario_gen \
  --coverage-mode boundary-first \
  --dry-run
```

Verify `out/stage3_scenario_gen/summary.json` contains:

- `coverage_mode`
- `dry_run`
- `template_library`
- `templates`
- `cases[*].template_id`

### C. sim-regress feedback check

```bash
cargo run -- sim-regress \
  --plc-dir <tmp_plc_dir> \
  --scenario-dir <tmp_scenario_dir> \
  --artifacts-dir out/stage3_sim_regress \
  --minimize-failure
```

Verify `out/stage3_sim_regress/feedback.json` contains:

- `schema_version`
- `feedback[*].plc`
- `feedback[*].scenario`
- `feedback[*].failure_kind`
- `feedback[*].template_hint`
- `feedback[*].parameter_hints`
