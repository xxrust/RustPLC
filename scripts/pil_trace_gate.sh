#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
PIL-style trace gate without requiring physical board flashing.

Usage:
  scripts/pil_trace_gate.sh \
    --sil <sil_trace.jsonl> \
    --out-dir <dir> \
    [--board-log <board.log>] \
    [--runner-cmd "<command producing board log on stdout>"] \
    [--duration <sec>]

Typical Renode usage:
  scripts/pil_trace_gate.sh \
    --sil out/trace.jsonl \
    --out-dir out/pil_gate \
    --runner-cmd "renode -e 'include @scripts/renode/run.resc'" \
    --duration 30

Notes:
  - If --runner-cmd is provided, this script captures its stdout into board.log.
  - If --runner-cmd is omitted, --board-log must point to an existing file.
  - Gate exits non-zero on mismatch via `trace-diff --fail-on-mismatch`.
USAGE
}

SIL=""
OUT_DIR=""
BOARD_LOG=""
RUNNER_CMD=""
DURATION="20"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sil) SIL="${2:-}"; shift 2;;
    --out-dir) OUT_DIR="${2:-}"; shift 2;;
    --board-log) BOARD_LOG="${2:-}"; shift 2;;
    --runner-cmd) RUNNER_CMD="${2:-}"; shift 2;;
    --duration) DURATION="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$SIL" || -z "$OUT_DIR" ]]; then
  echo "Missing required args: --sil/--out-dir" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$REPO_ROOT/$OUT_DIR"
BOARD_TRACE="$REPO_ROOT/$OUT_DIR/board_trace.jsonl"
DIFF_REPORT="$REPO_ROOT/$OUT_DIR/diff_report.json"

if [[ -z "$BOARD_LOG" ]]; then
  BOARD_LOG="$REPO_ROOT/$OUT_DIR/board.log"
elif [[ "$BOARD_LOG" != /* ]]; then
  BOARD_LOG="$REPO_ROOT/$BOARD_LOG"
fi

if [[ -n "$RUNNER_CMD" ]]; then
  echo "[1/3] runner command capture -> $BOARD_LOG"
  timeout "${DURATION}s" bash -lc "$RUNNER_CMD" > "$BOARD_LOG" || true
else
  echo "[1/3] skip runner capture (use existing --board-log)"
fi

if [[ ! -f "$BOARD_LOG" ]]; then
  echo "Board log not found: $BOARD_LOG" >&2
  exit 1
fi

echo "[2/3] trace-parse"
(
  cd "$REPO_ROOT"
  cargo run --release -- trace-parse --in "$BOARD_LOG" --out "$BOARD_TRACE"
)

echo "[3/3] trace-diff --fail-on-mismatch"
(
  cd "$REPO_ROOT"
  cargo run --release -- trace-diff \
    --sil "$SIL" \
    --board "$BOARD_TRACE" \
    --out "$DIFF_REPORT" \
    --fail-on-mismatch
)

echo "PIL gate passed."
echo "  board_log: $BOARD_LOG"
echo "  board_trace: $BOARD_TRACE"
echo "  diff_report: $DIFF_REPORT"
