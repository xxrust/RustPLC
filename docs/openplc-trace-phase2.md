# OpenPLC Trace Phase-2 Plan

This document defines the phase-2 OpenPLC trace collection and comparison contract for ST codegen.

## Chosen collection path

Use **Modbus TCP polling** (not runtime API) so the pipeline is deterministic and easy to reproduce in CI/HIL labs.

- `_state` is read from a holding register
- Boolean outputs (`valve_a`, `valve_b`) are read from coils
- Sampling period uses the same `tick_ms` as SIL scenario for stable alignment

## Variable-to-address mapping rule

Mapping file: `scenarios/openplc_trace_map.two_cylinder.json`

Current required variables:

- `_state` -> `holding_register:4096` (int)
- `valve_a` -> `coil:0` (bool)
- `valve_b` -> `coil:1` (bool)

When adding a new variable, update mapping JSON with:

- `source`: `coil` or `holding_register`
- `address`: Modbus address
- `type`: `bool` / `int` / `real`

## Tooling

Script: `scripts/openplc_trace.py`

### 1) Normalize OpenPLC raw CSV trace

```bash
python3 scripts/openplc_trace.py normalize-modbus \
  --raw out/openplc_raw.csv \
  --mapping scenarios/openplc_trace_map.two_cylinder.json \
  --tick-ms 10 \
  --out out/openplc_trace.normalized.jsonl
```

### 2) Compare SIL vs OpenPLC trace

```bash
python3 scripts/openplc_trace.py compare \
  --sil out/sil_trace.normalized.jsonl \
  --openplc out/openplc_trace.normalized.jsonl \
  --vars _state,valve_a,valve_b \
  --tick-tolerance 1 \
  --min-pass-rate 0.95 \
  --out out/openplc_trace_compare.report.json
```

The compare command exits non-zero if pass rate is below `0.95`.

## Acceptance target

- Core scenarios: `two_cylinder.plc` and `assembly_station.plc`
- Required pass rate: **>= 95%**
- Allowed timing drift: **±1 tick**
