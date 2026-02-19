# Commissioning Playbook

Date: 2026-02-19

Canonical document: `docs/commissioning_playbook.md`

Covers one chained workflow across:

- `scenario-doctor`
- `sim-plc` with retain and online control planes
- `no-board-gate`

Two rehearsal paths are included:

1. Nominal startup
2. Fault-injection debug

Each step in the canonical playbook includes explicit artifact path + pass/fail checkpoint.
