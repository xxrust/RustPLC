# Runtime Online Variable Control Plane (Dev Mode)

Date: 2026-02-19

- Canonical doc: `docs/runtime_online_variable_control.md`
- Command surface (sim-plc):
  - `--enable-online-force-dev`
  - `--online-var-script <script.jsonl>`
  - `--online-var-audit-out <audit.jsonl>`
- Variable types:
  - `BOOL:<name>` set/clear with `true|false|null`
  - `REAL:<name>` set/clear with `number|null`
- Determinism:
  - `at_ms` must align with `tick_ms`
  - same script replay produces same audit rows
- Safety boundary:
  - dev-only, default-off, SIL-only; does not change hardware fail-safe chain semantics
