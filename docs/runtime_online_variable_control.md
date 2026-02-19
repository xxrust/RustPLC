# Runtime Online Variable Control Plane (Dev Mode)

Date: 2026-02-19

## Goal

Provide a development-only control plane for runtime variables (BOOL/REAL) in `sim-plc` so commissioning/debug flows can replay variable mutations with full audit records.

## Safety Boundary

- This capability is **disabled by default**.
- It can only be enabled with `--enable-online-force-dev`.
- It is currently **SIL-only** (`sim-plc`) and does not bypass hardware fail-safe chains.
- Runtime variable operations are recorded as audit evidence and are intended for diagnosis/replay, not as production hardware control.
- Runtime effect is explicit: online variables are bound to runtime-evaluated input channels (DI/AI) and injected as deterministic force events.

## Command

```bash
cargo run --release -- sim-plc examples/force_override_demo.plc \
  --scenario scenarios/force_override_demo/force.yaml \
  --out out/force_trace.jsonl \
  --enable-online-force-dev \
  --online-var-script out/online_var.jsonl \
  --online-var-bindings out/online_var_bindings.toml \
  --online-var-audit-out out/online_var_audit.jsonl
```

## Script Format (JSONL)

Each line is one operation:

```json
{"at_ms":0,"actor":"commissioning","source":"panel","variable":"BOOL:diag_latch","value":true}
{"at_ms":20,"actor":"commissioning","source":"panel","variable":"REAL:gain_k","value":1.25}
{"at_ms":30,"actor":"commissioning","source":"panel","variable":"BOOL:diag_latch","value":null}
```

Rules:

- `variable`: `BOOL:<name>` or `REAL:<name>` (name chars: `A-Za-z0-9_.-`)
- `value`:
  - BOOL variable: `true`/`false` to set, `null` to clear
  - REAL variable: finite number to set, `null` to clear
- `at_ms` must align with scenario `tick_ms`
- lines are replayed in deterministic tick order (same script + same tick config -> same audit output)

## Binding Contract (`online_var_bindings.toml`)

Variables must be either:

1. explicitly bound via TOML (`--online-var-bindings`), or
2. auto-mappable by name (`BOOL:DI<n>` / `REAL:AI<n>`).

Example bindings file:

```toml
schema_version = 1

[bool]
diag_latch = "DI0"

[real]
gain_k = "AI0"
```

Runtime behavior:

- BOOL variable -> DI force channel
- REAL variable -> AI force channel
- set (`value`) persists until clear (`null`)

## Audit Contract (`online_var_audit.jsonl`)

Each row includes:

- `at_ms`
- `tick`
- `actor`
- `source`
- `variable`
- `variable_kind`
- `bound_channel` (resolved runtime channel, e.g. `di0` / `ai0`)
- `operation` (`set` / `clear`)
- `from`
- `to`

This satisfies traceability for who changed what, when, and how values evolved.
