# CI Runbook (Repo-local Wiki Draft)

This page explains how to reproduce the GitHub Actions gates locally, and what to check when CI fails.

Date: 2026-02-18

---

## What CI Runs

Workflow: `.github/workflows/rp2040_regression.yml`

Jobs:
- `workspace-test`: `cargo test --workspace`
- `topology-perf-gate`: `python3 scripts/topology_perf_gate.py --output human`
- `rp2040-cross-build`: `cargo build -p board-rp2040 --target thumbv6m-none-eabi --release`
- `trace-gate`: trace-diff + PIL gates against golden artifacts
- `pil-renode-runner`: PIL baseline suite with Renode runner (auto-download)

---

## Quick Local Repro (Recommended)

Run the same commands CI uses:

```bash
set -euo pipefail

cargo test --workspace

python3 scripts/topology_perf_gate.py --output human

cargo build -p board-rp2040 --target thumbv6m-none-eabi --release

cargo run --release -- trace-diff \
  --sil examples/trace_golden/sil_trace.jsonl \
  --board examples/trace_golden/board_trace_match.jsonl \
  --out out/ci_trace_match_report.json \
  --fail-on-mismatch

scripts/pil_trace_gate.sh \
  --sil examples/trace_golden/sil_trace.jsonl \
  --out-dir out/ci_pil_gate \
  --board-log examples/trace_golden/board_log_match.log

scripts/pil_trace_baseline_suite.sh \
  --runner cat \
  --out-root out/ci_pil_baselines

scripts/pil_semantic_baseline.sh \
  --cases-dir examples/pil_baselines \
  --out-root out/ci_pil_semantic_baselines

scripts/pil_trace_baseline_suite.sh \
  --runner renode \
  --out-root out/ci_pil_baselines_renode
```

Topology perf gate thresholds live in `scripts/perf/topology_perf_thresholds.json`.

---

## Motion Regression Notes (RP2040 stepper + AB encoder)

`cargo test --workspace` includes scenario-based regression tests for motion-related examples, such as:

- `examples/rp2040_motion_minimal.plc` + `scenarios/rp2040_motion_minimal/*.yaml`
- Test gate: `tests/rp2040_motion_minimal_scenarios.rs`

Fast local repro (just the motion regression):

```bash
cargo test -p rust_plc --test rp2040_motion_minimal_scenarios
```

If the motion regression fails, check:
- the `.plc` example and scenario YAML are in sync (`rust_plc scenario-validate ...`)
- the expected fault path is a timeout (trace event `reason == \"timeout\"`)

---

## `rp2040-cross-build` Notes

### Prerequisites
- Rust stable
- Target installed:

```bash
rustup target add thumbv6m-none-eabi
```

### Common Failure Patterns
- Missing target: install with `rustup target add ...`
- Linker script errors: check `.cargo/config.toml` and `link.x/defmt.x` availability
- Strict warning policies (`-D warnings`): prefer keeping `board-rp2040` warning-clean, because CI environments may vary

---

## Renode Runner Notes

`--runner renode` uses:
- `scripts/renode/ensure_renode.sh` (downloads Renode into `out/tools/renode/` if needed)
- `scripts/renode/run_trace_case.sh` to execute per-case `.resc`

If the Renode job fails:
- delete `out/tools/renode/` and retry (forces re-download)
- check that `python3`, `tar`, and outbound HTTPS access are available
