# Class-D Abnormal Exit Evidence Workflow

Date: 2026-02-19

## Goal

Standardize Class-D (`hardware_only`) manual electrical evidence so hardware-chain validation is reviewable and machine-checkable.

## Source Assets

- `scenarios/rp2040_hil_gate/abnormal_exit/matrix.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence_schema.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/class_d_checklist_template.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/D.json`

## Required Class-D Checklist Fields

- `trigger`
- `wiring_state`
- `measured_result`
- `verdict`
- `operator`
- `attachments[]`

## Attachment Provenance Contract

Each `attachments[]` item must provide provenance metadata:

- Required: `provenance.source_path`
- Optional: `provenance.digest_sha256`, `provenance.captured_by`, `provenance.captured_at`

Purpose: make Class-D manual files traceable to their original capture/export source during audits.

## Verifier Behavior

`python3 scripts/abnormal_exit_matrix_verify.py` now validates Class-D manual evidence attachments even when D stays `hardware_only`.

- valid D manual artifact: `manual_hardware_chain_validated`
- invalid D manual artifact: `manual_hardware_chain_invalid`
- if D is added to `--require-classes`, verifier returns non-zero because hardware-only classes are not auto-passable.

## Related Docs

- `docs/已实现/abnormal_exit_matrix.md`
- `docs/已实现/board_rp2040.md`
