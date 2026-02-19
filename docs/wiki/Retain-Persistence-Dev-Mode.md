# Retain Persistence (Dev Mode)

Date: 2026-02-19

## What It Does

`sim-plc` can persist configured DI/AI/DO/AO channels across restarts with:

- config-defined defaults,
- JSON state + SHA256 checksum,
- corruption fallback to defaults.

## Quick Use

```bash
cargo run --release -- sim-plc examples/force_override_demo.plc \
  --scenario scenarios/force_override_demo/force.yaml \
  --out out/trace.jsonl \
  --retain-config out/retain.toml \
  --retain-state out/retain_state.json
```

## Notes

- `--retain-state` without `--retain-config` is rejected.
- Missing/corrupted state does not fail run; it emits `[RET-201]` and uses defaults.
- Outputs are restored with one-tick bootstrap force, then auto-cleared for normal runtime ownership.
