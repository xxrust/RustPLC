#!/usr/bin/env bash
# Gate SIL vs OpenPLC variable traces with ±1 tick tolerance and >=95% pass rate.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SIL=""
OPENPLC=""
OUT=""
VARS="_state,valve_a,valve_b"
TICK_TOL=1
MIN_PASS=0.95

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sil) SIL="${2:-}"; shift 2 ;;
    --openplc) OPENPLC="${2:-}"; shift 2 ;;
    --out) OUT="${2:-}"; shift 2 ;;
    --vars) VARS="${2:-}"; shift 2 ;;
    --tick-tolerance) TICK_TOL="${2:-}"; shift 2 ;;
    --min-pass-rate) MIN_PASS="${2:-}"; shift 2 ;;
    -h|--help)
      cat <<USAGE
Usage: scripts/openplc_trace_gate.sh --sil <sil_normalized.jsonl> --openplc <openplc_normalized.jsonl> --out <report.json>
       [--vars _state,valve_a,valve_b] [--tick-tolerance 1] [--min-pass-rate 0.95]
USAGE
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SIL" || -z "$OPENPLC" || -z "$OUT" ]]; then
  echo "Missing required args: --sil/--openplc/--out" >&2
  exit 2
fi

python3 "$REPO_ROOT/scripts/openplc_trace.py" compare \
  --sil "$SIL" \
  --openplc "$OPENPLC" \
  --vars "$VARS" \
  --tick-tolerance "$TICK_TOL" \
  --min-pass-rate "$MIN_PASS" \
  --out "$OUT"
