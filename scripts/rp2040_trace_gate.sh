#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run RP2040 build/flash/log/trace-diff as one reproducible pipeline.

Usage:
  scripts/rp2040_trace_gate.sh \
    --plc <file.plc> \
    --io-map <io_map.toml> \
    --sil-trace <trace.jsonl> \
    [--out-dir <dir>] \
    [--max-p99-exec-us <us>] \
    [--max-overrun-count <n>] \
    [--mount <rp2040_mount>] \
    [--board-log <board.log>] \
    [--collect-mode serial --port <tty> [--baud <n>] [--duration <sec>]] \
    [--collect-mode cmd --cmd "<producer>" [--duration <sec>]]

Notes:
  - Step 1 always runs: build-rp2040 + emit UF2
  - If --mount is provided, step 2 flashes UF2 (dry-run then actual copy)
  - If --collect-mode is provided, step 3 collects board log into --board-log (or default path)
  - If --collect-mode is omitted, --board-log must already exist
  - Step 4 always runs: board-parse + trace-diff --fail-on-mismatch
USAGE
}

PLC=""
IO_MAP=""
SIL_TRACE=""
OUT_DIR="out/rp2040_gate"
MOUNT=""
BOARD_LOG=""
COLLECT_MODE=""
PORT=""
BAUD="115200"
DURATION="20"
COLLECT_CMD=""
MAX_P99_EXEC_US=""
MAX_OVERRUN_COUNT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plc) PLC="${2:-}"; shift 2;;
    --io-map) IO_MAP="${2:-}"; shift 2;;
    --sil-trace) SIL_TRACE="${2:-}"; shift 2;;
    --out-dir) OUT_DIR="${2:-}"; shift 2;;
    --max-p99-exec-us) MAX_P99_EXEC_US="${2:-}"; shift 2;;
    --max-overrun-count) MAX_OVERRUN_COUNT="${2:-}"; shift 2;;
    --mount) MOUNT="${2:-}"; shift 2;;
    --board-log) BOARD_LOG="${2:-}"; shift 2;;
    --collect-mode) COLLECT_MODE="${2:-}"; shift 2;;
    --port) PORT="${2:-}"; shift 2;;
    --baud) BAUD="${2:-}"; shift 2;;
    --duration) DURATION="${2:-}"; shift 2;;
    --cmd) COLLECT_CMD="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$PLC" || -z "$IO_MAP" || -z "$SIL_TRACE" ]]; then
  echo "Missing required args: --plc/--io-map/--sil-trace" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$OUT_DIR" = /* ]]; then
  OUT_DIR_ABS="$OUT_DIR"
else
  OUT_DIR_ABS="$REPO_ROOT/$OUT_DIR"
fi
mkdir -p "$OUT_DIR_ABS"
UF2="$OUT_DIR_ABS/firmware.uf2"
RP2040_OUT="$OUT_DIR_ABS/rp2040"
BOARD_LOG_DEFAULT="$OUT_DIR_ABS/board.log"
BOARD_TRACE="$OUT_DIR_ABS/board_trace.jsonl"
TICK_TIMING="$OUT_DIR_ABS/tick_timing.jsonl"
TIMING_REPORT="$OUT_DIR_ABS/timing_report.json"
TIMING_VERDICT="$OUT_DIR_ABS/timing_gate_verdict.json"
DIFF_REPORT="$OUT_DIR_ABS/diff_report.json"
DASHBOARD_HTML="$OUT_DIR_ABS/trace_diff_dashboard.html"

if [[ -z "$BOARD_LOG" ]]; then
  BOARD_LOG="$BOARD_LOG_DEFAULT"
fi
if [[ "$BOARD_LOG" != /* ]]; then
  BOARD_LOG="$REPO_ROOT/$BOARD_LOG"
fi

echo "[1/4] build-rp2040 + emit UF2"
(
  cd "$REPO_ROOT"
  cargo run --release -- build-rp2040 "$PLC" \
    --out "$RP2040_OUT" \
    --io-map "$IO_MAP" \
    --emit-uf2 "$UF2"
)

if [[ -n "$MOUNT" ]]; then
  echo "[2/4] flash-rp2040 dry-run"
  (
    cd "$REPO_ROOT"
    cargo run --release -- flash-rp2040 --uf2 "$UF2" --mount "$MOUNT" --dry-run
  )
  echo "[2/4] flash-rp2040 actual copy"
  (
    cd "$REPO_ROOT"
    cargo run --release -- flash-rp2040 --uf2 "$UF2" --mount "$MOUNT"
  )
else
  echo "[2/4] skip flash (no --mount provided)"
fi

if [[ -n "$COLLECT_MODE" ]]; then
  echo "[3/4] collect board log ($COLLECT_MODE)"
  case "$COLLECT_MODE" in
    serial)
      if [[ -z "$PORT" ]]; then
        echo "--port is required when --collect-mode serial" >&2
        exit 2
      fi
      "$REPO_ROOT/scripts/collect_board_log.sh" \
        --mode serial \
        --port "$PORT" \
        --baud "$BAUD" \
        --duration "$DURATION" \
        --out "$BOARD_LOG"
      ;;
    cmd)
      if [[ -z "$COLLECT_CMD" ]]; then
        echo "--cmd is required when --collect-mode cmd" >&2
        exit 2
      fi
      "$REPO_ROOT/scripts/collect_board_log.sh" \
        --mode cmd \
        --duration "$DURATION" \
        --out "$BOARD_LOG" \
        --cmd "$COLLECT_CMD"
      ;;
    *)
      echo "Unsupported --collect-mode: $COLLECT_MODE (use serial|cmd)" >&2
      exit 2
      ;;
  esac
else
  echo "[3/4] skip log collection (no --collect-mode provided)"
fi

if [[ ! -f "$BOARD_LOG" ]]; then
  echo "Board log not found: $BOARD_LOG" >&2
  echo "Provide --collect-mode to collect one, or pass an existing file via --board-log." >&2
  exit 1
fi

echo "[4/4] board-parse + trace-diff --fail-on-mismatch"
(
  cd "$REPO_ROOT"
  cargo run --release -- board-parse --in "$BOARD_LOG" --out-dir "$OUT_DIR_ABS"
  if [[ -s "$TICK_TIMING" ]]; then
    cargo run --release -- timing-report --in "$TICK_TIMING" --out "$TIMING_REPORT"
  else
    echo "WARN: tick_timing.jsonl is empty; skip timing-report (no TIMING records in board log?)" >&2
  fi
  cargo run --release -- trace-diff \
    --sil "$SIL_TRACE" \
    --board "$BOARD_TRACE" \
    --out "$DIFF_REPORT" \
    --fail-on-mismatch
)

timing_gate_args=(
  --timing-report "$TIMING_REPORT"
  --out "$TIMING_VERDICT"
)
if [[ -n "$MAX_P99_EXEC_US" ]]; then
  timing_gate_args+=(--max-p99-exec-us "$MAX_P99_EXEC_US")
fi
if [[ -n "$MAX_OVERRUN_COUNT" ]]; then
  timing_gate_args+=(--max-overrun-count "$MAX_OVERRUN_COUNT")
fi

if command -v python3 >/dev/null 2>&1; then
  set +e
  python3 "$REPO_ROOT/scripts/timing_gate_verdict.py" "${timing_gate_args[@]}"
  timing_gate_status=$?
  set -e
  if [[ -f "$TIMING_VERDICT" ]]; then
    python3 - "$TIMING_VERDICT" <<'PY'
import json
import sys

obj = json.load(open(sys.argv[1], "r", encoding="utf-8"))
status = obj.get("status", "unknown")
message = obj.get("message", "")
observed = obj.get("observed") or {}
thresholds = obj.get("thresholds") or {}
print(
    "[timing-gate] status={status} p99={p99} overrun={overrun} "
    "max_p99={max_p99} max_overrun={max_overrun} message={message}".format(
        status=status,
        p99=observed.get("exec_us_p99"),
        overrun=observed.get("overrun_count"),
        max_p99=thresholds.get("max_p99_exec_us"),
        max_overrun=thresholds.get("max_overrun_count"),
        message=message,
    )
)
PY
  fi
  if [[ "$timing_gate_status" -ne 0 ]]; then
    exit "$timing_gate_status"
  fi
else
  echo "WARN: python3 not found; skip timing gate verdict generation" >&2
  if [[ -n "$MAX_P99_EXEC_US" || -n "$MAX_OVERRUN_COUNT" ]]; then
    echo "timing gate thresholds were requested but cannot be evaluated without python3" >&2
    exit 2
  fi
fi

if command -v python3 >/dev/null 2>&1; then
  "$REPO_ROOT/scripts/trace_diff_dashboard.py" \
    --diff "$DIFF_REPORT" \
    --out "$DASHBOARD_HTML" \
    --title "RP2040 Trace Gate" \
    --sil-trace "$SIL_TRACE" \
    --board-trace "$BOARD_TRACE" \
    --board-log "$BOARD_LOG"
fi

echo "Pipeline done."
echo "  UF2: $UF2"
echo "  Board log: $BOARD_LOG"
echo "  Board trace: $BOARD_TRACE"
echo "  Tick timing: $TICK_TIMING"
echo "  Timing report: $TIMING_REPORT"
echo "  Timing verdict: $TIMING_VERDICT"
echo "  Diff report: $DIFF_REPORT"
echo "  Dashboard: $DASHBOARD_HTML"
