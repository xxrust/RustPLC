#!/usr/bin/env bash
# End-to-end phase-2 gate: normalize OpenPLC raw CSV + compare against SIL traces
# for core scenarios with ±1 tick tolerance and >=95% pass-rate threshold.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

FIXTURE_DIR="${1:-$REPO_ROOT/examples/openplc_trace_phase2}"
OUT_DIR="${2:-$REPO_ROOT/out/openplc_trace_phase2}"

mkdir -p "$OUT_DIR"

run_case() {
  local case_name="$1"
  local vars="$2"
  local mapping="$3"

  local raw_csv="$FIXTURE_DIR/${case_name}.openplc_raw.csv"
  local sil_norm="$FIXTURE_DIR/${case_name}.sil.normalized.jsonl"
  local openplc_norm="$OUT_DIR/${case_name}.openplc.normalized.jsonl"
  local report="$OUT_DIR/${case_name}.trace_compare.report.json"

  echo "[OpenPLC-Phase2] Normalize raw CSV: ${case_name}"
  python3 "$REPO_ROOT/scripts/openplc_trace.py" normalize-modbus \
    --raw "$raw_csv" \
    --mapping "$mapping" \
    --tick-ms 10 \
    --out "$openplc_norm"

  echo "[OpenPLC-Phase2] Compare SIL vs OpenPLC: ${case_name}"
  bash "$REPO_ROOT/scripts/openplc_trace_gate.sh" \
    --sil "$sil_norm" \
    --openplc "$openplc_norm" \
    --vars "$vars" \
    --tick-tolerance 1 \
    --min-pass-rate 0.95 \
    --out "$report"
}

run_case "two_cylinder" "_state,valve_a,valve_b" "$REPO_ROOT/scenarios/openplc_trace_map.two_cylinder.json"
run_case "assembly_station" "_state,motor_left,motor_right" "$REPO_ROOT/scenarios/openplc_trace_map.assembly_station.json"

echo "[OpenPLC-Phase2] OK - core scenario trace gates passed"
