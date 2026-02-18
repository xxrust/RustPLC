# OpenPLC_v3 Learnings Integration (Repo-local Wiki Draft)

This is a repo-local Wiki draft meant to be readable offline.

It summarizes how we **translated OpenPLC_v3-style engineering learnings** into RustPLC deliverables, and where to look in the repository.

Aligned with:
- `docs/openplc_v3_analysis.md`
- `docs/prd_openplc_v3_learnings_notes.md`

Date: 2026-02-18

---

## 1) Board Log -> Standard Artifacts (`board-parse`)

Goal: turn mixed board logs (`TRACE ...`, `TIMING ...`, plus other noise) into a **standard artifact set** that downstream tools can consume.

Command:

```bash
cargo run --release -- board-parse --in <board.log> --out-dir out/board_artifacts
```

Outputs:
- `out/board_artifacts/board_trace.jsonl` (one structured trace row per line)
- `out/board_artifacts/tick_timing.jsonl` (one `TickTimingSample` per line)

Why this shape:
- downstream gates/tools can stay format-stable (`trace-diff`, `timing-report`, dashboards)
- parsing logic is single-sourced in `rust_plc::board_log` (TRACE + TIMING together)

Related code/docs:
- Parser composition: `src/board_log.rs`
- TRACE line parser: `src/board_trace.rs`
- TIMING line parser: `src/tick_timing.rs`
- RP2040 gate script: `scripts/rp2040_trace_gate.sh`

---

## 2) FORCE / Override (SIL-first, scenario-driven)

Goal: provide OpenPLC-like FORCE capability in a **regression-friendly** way.

RustPLC approach:
- Implement FORCE semantics in SIL IO (`SimIo`)
- Drive FORCE through `scenario.yaml` so it is **replayable** in CI

Where:
- Semantics: `crates/sim/src/lib.rs` (`SimIo` forced DI/AI/DO/AO)
- Scenario schema: `crates/sim/src/scenario.rs` (`forces:` list; `null` clears)
- Doc: `docs/force_override.md`
- Minimal demo:
  - `examples/force_override_demo.plc`
  - `scenarios/force_override_demo/force.yaml`

Key rules (mental model):
- inputs: `force > plant > scheduled`
- outputs: `force > program writes`
- edges record **final observable outputs** (not “attempted program writes”)

---

## 3) IEC Address Aliases + IoMap Normalization

Goal: accept IEC-style addressing as a **migration alias layer**, while keeping core IoMap canonical (`di/do/ai/ao` only).

Pieces:
- Minimal IEC parser (tool-chain only):
  - Supports `%IXn.m`, `%QXn.m`, `%IWn`, `%QWn`
  - Mapping doc: `docs/iec_address_aliases.md`
  - Implementation: `src/iec_address.rs`

- Normalization command:

```bash
cargo run --release -- io-map-normalize --in io_map.toml --out io_map.normalized.toml
```

What it does:
- converts quoted IEC keys (e.g. `"%IX0.0"`) into native keys (e.g. `di0`)
- merges native + IEC keys
- errors on conflicts for the same logical channel id
- preserves other sections like `[safe_state]`

Tests:
- `tests/io_map_normalize.rs`

---

## 4) RP2040 Firmware: Minimal HAL Shape

Goal: converge towards an OpenPLC-like “small HAL surface” so board ports are easier to add later.

Current shape (internal module):
- `crates/board-rp2040/src/firmware/hal.rs`
  - `initialize`
  - `update_in`
  - `update_out`
  - `finalize_on_error`

This is a structural refactor: it keeps the same external observability records (`TICK/TRACE/LOG/TIMING`) and safe-state behavior.

---

## Suggested Next Step (After This Integration)

If we continue following the same “board-evidence + replayable control-plane” direction, the next most valuable step is to make **real-board timing evidence** first-class in HIL gates (so it matches the virtual-board / no-board evidence chain).

This is now supported in the RP2040 gate scripts:

- `scripts/rp2040_trace_gate.sh` can generate `timing_report.json` from board `tick_timing.jsonl`
- Optional timing gate thresholds:
  - `--max-p99-exec-us <us>`
  - `--max-overrun-count <n>`

This makes “realtime evidence” symmetric between virtual-board and real-board runs.
