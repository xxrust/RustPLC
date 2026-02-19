# Commissioning Playbook

Date: 2026-02-19

Canonical document: `docs/commissioning_playbook.md`

Covers one chained workflow across:

- `commissioning-run` (single command orchestration)
- `scenario-doctor`
- `sim-plc` with retain and online control planes
- `no-board-gate`

Two rehearsal paths are included:

1. Nominal startup
2. Fault-injection debug

`commissioning-run` emits `out/commissioning/commissioning_index.json` with per-step pass/fail status and artifact pointers; manual step-by-step commands remain documented for debugging.
