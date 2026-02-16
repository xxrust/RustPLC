#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run PIL trace baseline suite (multiple deterministic cases).

Usage:
  scripts/pil_trace_baseline_suite.sh [--runner cat|renode] [--out-root <dir>]

Default runner:
  cat (fast, no simulator dependency)

When runner=renode:
  uses scripts/renode/run_trace_case.sh to execute per-case `.resc` scripts.
USAGE
}

RUNNER="cat"
OUT_ROOT="out/pil_baselines"
RENODE_BIN=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runner) RUNNER="${2:-}"; shift 2;;
    --out-root) OUT_ROOT="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ "$RUNNER" != "cat" && "$RUNNER" != "renode" ]]; then
  echo "--runner must be one of: cat, renode" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_DIR="$REPO_ROOT/examples/trace_baselines"

if [[ "$RUNNER" == "renode" ]]; then
  RENODE_BIN="$("$REPO_ROOT/scripts/renode/ensure_renode.sh")"
fi

if [[ ! -d "$BASE_DIR" ]]; then
  echo "Missing baseline dir: $BASE_DIR" >&2
  exit 1
fi

CASE_COUNT=0
for case_dir in "$BASE_DIR"/*; do
  [[ -d "$case_dir" ]] || continue
  case_name="$(basename "$case_dir")"
  sil="$case_dir/sil_trace.jsonl"
  if [[ ! -f "$sil" ]]; then
    echo "skip $case_name (missing sil_trace.jsonl)"
    continue
  fi

  out_dir="$OUT_ROOT/$case_name"
  if [[ "$RUNNER" == "cat" ]]; then
    board_log="$case_dir/board_log.txt"
    if [[ ! -f "$board_log" ]]; then
      echo "Missing board_log.txt for case: $case_name" >&2
      exit 1
    fi
    cmd="cat '$board_log'"
  else
    cmd="$REPO_ROOT/scripts/renode/run_trace_case.sh --case $case_name --renode-bin '$RENODE_BIN'"
  fi

  echo "[baseline] case=$case_name runner=$RUNNER"
  "$REPO_ROOT/scripts/pil_trace_gate.sh" \
    --sil "$sil" \
    --out-dir "$out_dir" \
    --runner-cmd "$cmd" \
    --duration 20

  CASE_COUNT=$((CASE_COUNT + 1))
done

if [[ $CASE_COUNT -eq 0 ]]; then
  echo "No baseline cases found under $BASE_DIR" >&2
  exit 1
fi

echo "PIL baseline suite passed: $CASE_COUNT case(s), runner=$RUNNER"
