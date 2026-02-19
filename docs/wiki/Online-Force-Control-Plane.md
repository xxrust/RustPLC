# Online Force Control Plane (Dev Mode)

Date: 2026-02-19

## Goal

Provide a development-only runtime force control plane for `sim-plc` while keeping:

- default-off safety,
- deterministic replay,
- full audit trail.

## Command

```bash
cargo run --release -- sim-plc examples/force_override_demo.plc \
  --scenario scenarios/force_override_demo/force.yaml \
  --out out/force_trace.jsonl \
  --enable-online-force-dev \
  --online-force-script out/online_force.jsonl \
  --online-force-audit-out out/online_force_audit.jsonl
```

## Script Format (JSONL)

```json
{"at_ms":0,"actor":"commissioning","source":"panel","channel":"DI0","value":true}
{"at_ms":20,"actor":"commissioning","source":"panel","channel":"DI0","value":null}
{"at_ms":30,"actor":"commissioning","source":"panel","channel":"AO0","value":1.25}
```

- `channel`: `DI<n>/AI<n>/DO<n>/AO<n>`
- `value`: bool/number=set, `null`=clear
- `at_ms`: must align with scenario `tick_ms`

## Safety Boundary

- The control plane is disabled unless `--enable-online-force-dev` is set.
- This capability is SIL-only and does not change hardware fail-safe chains.
- Runtime audit records every operation (`actor/source/time/channel/from/to`).
