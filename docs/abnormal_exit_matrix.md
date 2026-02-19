# RP2040 Abnormal-Exit Safety Matrix (A/B/C/D)

This document defines abnormal-exit classes for RP2040 safety validation and the evidence contract used by HIL audits.

## Matrix

Source of truth: `scenarios/rp2040_hil_gate/abnormal_exit/matrix.json`

| Class | Trigger method | Expected IO behavior | Acceptance checks | Verification mode |
| --- | --- | --- | --- | --- |
| A | `ui_stop_or_sigterm` | Software safe-state profile applies to critical outputs. | `safe_state_applied`, `ordering_brake_before_enable`, `critical_outputs_safe` | Automated |
| B | `recoverable_runtime_error` | Fault-handling path applies safe-state profile. | `fault_path_reached`, `safe_state_applied`, `critical_outputs_safe` | Automated |
| C | `panic_abort_or_hardfault` | Software cleanup is best-effort; hardware safety chain must force safe outputs. | `panic_detected`, `hardware_chain_opened`, `critical_outputs_safe` | Automated evidence check (hardware-chain driven outputs) |
| D | `kill9_power_loss_kernel_hang` | No software callback is guaranteed; rely on independent hardware safety chain. | `manual_electrical_checklist_completed`, `hardware_chain_validated` | Manual hardware validation only |

## Evidence schema and artifacts

Evidence files live in:

- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/A.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/B.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/C.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/D.json`

JSON schema contract:

- `scenarios/rp2040_hil_gate/abnormal_exit/evidence_schema.json`

Each evidence file must include these key fields used by audits:

- `trigger`
- `observed_outputs`
- `verdict`
- `artifacts.trigger_log` and `artifacts.output_log`

## Automated verification (A/B/C)

Run verifier:

```bash
python3 scripts/abnormal_exit_matrix_verify.py \
  --matrix scenarios/rp2040_hil_gate/abnormal_exit/matrix.json \
  --evidence-dir scenarios/rp2040_hil_gate/abnormal_exit/evidence \
  --out out/rp2040_hil_daily_gate/abnormal_exit_report.json
```

Default required classes are `A,B,C`. Class `D` is marked `hardware_only` and is intentionally excluded from auto-pass criteria.

If `D` is added to `--require-classes`, the verifier returns non-zero and reports `manual_hardware_chain` to make the boundary explicit.

## Electrical checklist notes (critical actuators)

For vertical-axis systems (example channels: `do2` brake, `do1` enable):

1. Verify brake de-energize timing is before (or at worst simultaneous with) enable drop during controlled safe-state paths.
2. Verify crash/power-loss paths de-energize brake and enable via independent hardware safety chain (relay/STO), without software cleanup.
3. Attach instrument capture IDs and measured timestamps in class-D evidence artifacts.
