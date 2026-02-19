# M6 Diagnostics: Error Codes, Output Modes, and Scenario Doctor

This document introduces the M6 diagnostics package for scenario/build/gate workflows.

## 1) Stable Error-Code Families

- `SCN-*`: scenario authoring/validation diagnostics
- `GATE-*`: no-board gate command failures
- `BLD-*`: RP2040 build-flow failures
- `DIAG-*`: deterministic trace diagnosis candidates/errors

Examples:

- `SCN-MAP-002`: referenced digital input id does not exist in PLC topology
- `SCN-TICK-001`: a scenario `at_ms` field is not aligned to `tick_ms`
- `SCN-RISK-001`: start input is held true without release (same-tick-loop risk)
- `DIAG-IN-001`: expected input never changed before timeout anchor
- `DIAG-ACT-001`: actuator command likely missing around mismatch anchor
- `DIAG-INT-001`: interlock/requires preconditions likely blocked
- `DIAG-MAP-001`: mapping or alias mismatch likely
- `DIAG-TIME-001`: timeout budget likely too short

## 2) Unified Output Mode (`--output human|json`)

Key commands now support machine-readable diagnostics:

```bash
rust_plc scenario-validate examples/assembly_station.plc \
  --scenario scenarios/normal.yaml \
  --output json

rust_plc no-board-gate examples/two_cylinder.plc \
  --scenario scenarios/two_cylinder.yaml \
  --out-dir out/no_board_gate \
  --output json

rust_plc build-rp2040 examples/two_cylinder.plc \
  --out out/build_rp2040 \
  --output json

rust_plc trace-doctor examples/two_cylinder.plc \
  --scenario scenarios/two_cylinder.yaml \
  --trace out/no_board_gate/sil_trace.jsonl \
  --diff out/no_board_gate/diff_report.json \
  --timing-report out/no_board_gate/timing_report.json \
  --evidence-source no_board \
  --output json
```

## 3) `scenario-doctor` MVP

Run focused diagnostics and optional fix preview:

```bash
rust_plc scenario-doctor examples/assembly_station.plc \
  --scenario scenarios/normal.yaml \
  --fix-preview \
  --output human
```

`scenario-doctor` checks common problems:

- path/source mismatch between scenario header and target PLC
- device-mapping issues (unknown DI/AI/DO/AO ids)
- tick-alignment issues (`at_ms` not aligned to `tick_ms`)
- same-tick-loop risk patterns (e.g., held start signal)

## 4) Troubleshooting Examples

### Unknown input mapping

Symptom (JSON issue code): `SCN-MAP-002`

Fix:

```bash
rust_plc scenario-init examples/assembly_station.plc \
  --out scenarios/assembly_station.fixed.yaml \
  --preset normal
```

### Tick alignment error

Symptom (JSON issue code): `SCN-TICK-001`

Fix: round all `at_ms` values to multiples of `tick_ms`.

### Same-tick risk

Symptom (JSON issue code): `SCN-RISK-001`

Fix pattern:

```yaml
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        10: true
  - at_ms: 10
    set:
      digital_inputs:
        10: false
```

## 5) `trace-doctor` JSON Contract

`trace-doctor --output json` returns deterministic machine-readable fields:

- `schema_version`
- `evidence_source` (`no_board|hil_board|runtime_live|mixed`)
- `anchors`
- `candidates` (stable `DIAG-*` issue codes)
- `summary`
- `artifacts` (PLC/scenario/trace/diff/timing paths)
