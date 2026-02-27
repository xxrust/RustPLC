#!/usr/bin/env bash
set -euo pipefail

run_case() {
  local name="$1"
  shift
  echo "[cam-gate] ${name}"
  if ! "$@"; then
    echo "[cam-gate] FAILED: ${name}" >&2
    echo "[cam-gate] Command: $*" >&2
    exit 1
  fi
}

run_case "parser" \
  cargo test -p rust_plc --lib parser::tests::parses_cam_table_declarations_in_topology -- --exact

run_case "semantic" \
  cargo test -p rust_plc --lib semantic::tests::periodic_cam_table_coeffs_are_c2_continuous_on_boundaries -- --exact

run_case "runtime-interpolation" \
  cargo test -p runtime-core binary_search_interval_covers_boundaries_exact_hits_and_inner_points

run_case "runtime-switch" \
  cargo test -p runtime-core cam_switch_keeps_continuity_with_ratio_phase_and_decay

run_case "runtime-guard" \
  cargo test -p runtime-core runtime_rejects_too_many_cam_couplings_at_init

run_case "runtime-bridge" \
  cargo test -p rust_plc --test runtime_bridge_us006 bridge_maps_cam_tables_configs_and_actions -- --exact

run_case "verification-safety" \
  cargo test -p rust_plc --lib verification::safety::tests::models_cam_following_error_threshold_on_port_domain -- --exact

run_case "verification-causality" \
  cargo test -p rust_plc --lib verification::causality::tests::accepts_encoder_cam_servo_chain_with_cam_actions -- --exact

echo "[cam-gate] PASS"
