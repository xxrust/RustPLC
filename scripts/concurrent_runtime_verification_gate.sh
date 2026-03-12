#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[concurrent-gate] Running blocking axis.move_* runtime regressions"
cargo test --test runtime_bridge_us006 \
  axis_move_blocking_baseline_example_blocks_without_explicit_wait_until_done \
  -- --exact --nocapture

echo "[concurrent-gate] Running multi-task concurrent runtime regressions"
cargo test --test runtime_bridge_us006 \
  load_unload_concurrent_example_keeps_load_blocked_while_unload_advances \
  -- --exact --nocapture

echo "[concurrent-gate] Running concurrent example compile + verification regressions"
cargo test --test examples_integration \
  parses_axis_move_blocking_baseline_example_without_explicit_wait \
  -- --exact --nocapture
cargo test --test examples_integration \
  parses_load_unload_concurrent_tasks_example_into_verified_ir_json \
  -- --exact --nocapture

echo "[concurrent-gate] Running verification engine concurrent regressions"
cargo test --lib \
  verification::safety::tests::reports_conflict_when_independent_tasks_overlap_on_conflicting_outputs \
  -- --exact --nocapture
cargo test --lib \
  verification::liveness::tests::reports_deadlock_when_two_tasks_only_wait_each_other_resource_release \
  -- --exact --nocapture
cargo test --lib \
  verification::timing::tests::concurrent_worst_case_analysis_distinguishes_task_local_and_global_completion \
  -- --exact --nocapture
cargo test --lib \
  verification::causality::tests::accepts_cross_task_variable_chain_with_compute_dataflow \
  -- --exact --nocapture

echo "[concurrent-gate] PASS"
