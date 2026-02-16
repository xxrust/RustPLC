#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run semantic PIL baselines from real .plc + scenario pairs.

For each case under examples/pil_baselines/<case>:
  1) Generate SIL trace via `sim-plc`
  2) Generate board-style log via `pil-run`
  3) Parse + diff gate via `pil_trace_gate.sh`

Usage:
  scripts/pil_semantic_baseline.sh [--cases-dir <dir>] [--out-root <dir>]
USAGE
}

CASES_DIR="examples/pil_baselines"
OUT_ROOT="out/pil_semantic_baselines"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cases-dir) CASES_DIR="${2:-}"; shift 2;;
    --out-root) OUT_ROOT="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$CASES_DIR" != /* ]]; then
  CASES_DIR="$REPO_ROOT/$CASES_DIR"
fi
if [[ "$OUT_ROOT" != /* ]]; then
  OUT_ROOT="$REPO_ROOT/$OUT_ROOT"
fi

if [[ ! -d "$CASES_DIR" ]]; then
  echo "Cases dir does not exist: $CASES_DIR" >&2
  exit 1
fi

COUNT=0
for case_dir in "$CASES_DIR"/*; do
  [[ -d "$case_dir" ]] || continue
  case_name="$(basename "$case_dir")"
  plc="$case_dir/case.plc"
  scenario="$case_dir/scenarios/base.yaml"
  if [[ ! -f "$plc" || ! -f "$scenario" ]]; then
    echo "skip $case_name (missing case.plc or scenarios/base.yaml)"
    continue
  fi

  case_out="$OUT_ROOT/$case_name"
  mkdir -p "$case_out"
  sil="$case_out/sil_trace.jsonl"

  echo "[semantic-baseline] case=$case_name"
  (
    cd "$REPO_ROOT"
    cargo run --release -- sim-plc "$plc" --scenario "$scenario" --out "$sil"
  )

  runner_cmd="cargo run --release -- pil-run '$plc' --scenario '$scenario'"
  "$REPO_ROOT/scripts/pil_trace_gate.sh" \
    --sil "$sil" \
    --out-dir "$case_out/gate" \
    --runner-cmd "$runner_cmd" \
    --duration 20

  COUNT=$((COUNT + 1))
done

if [[ $COUNT -eq 0 ]]; then
  echo "No valid semantic baseline cases found under $CASES_DIR" >&2
  exit 1
fi

echo "Semantic PIL baselines passed: $COUNT case(s)"
