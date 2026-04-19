# Diagnostics Backend Methodology

See the full Chinese methodology doc:

- `docs/已实现/diagnostics_backend_methodology.md`

Quick entrypoints:

- `trace-doctor` (offline diagnosis contract)
- `no-board-gate --output json` (gate + diagnosis artifact on fail)
- `sim-plc --io-snapshot-out <io_snapshot.json>` (optional tick-level evidence artifact)
