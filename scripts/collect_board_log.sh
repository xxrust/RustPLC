#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Collect board runtime logs into a stable file for trace parsing/diff.

Usage:
  scripts/collect_board_log.sh --mode serial --out <board.log> [--duration <sec>] --port <tty> [--baud <n>]
  scripts/collect_board_log.sh --mode cmd    --out <board.log> [--duration <sec>] --cmd "<producer command>"

Modes:
  serial  Read from UART TTY (Linux), e.g. /dev/ttyACM0
  cmd     Capture stdout of a custom log producer command (RTT/other tools)

Examples:
  scripts/collect_board_log.sh --mode serial --port /dev/ttyACM0 --baud 115200 --duration 20 --out out/board.log
  scripts/collect_board_log.sh --mode cmd --duration 20 --out out/board.log \
    --cmd "probe-rs attach --chip RP2040 --elf target/thumbv6m-none-eabi/release/board-rp2040"
USAGE
}

MODE=""
OUT=""
DURATION=10
PORT=""
BAUD=115200
CMD=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="${2:-}"; shift 2;;
    --out) OUT="${2:-}"; shift 2;;
    --duration) DURATION="${2:-}"; shift 2;;
    --port) PORT="${2:-}"; shift 2;;
    --baud) BAUD="${2:-}"; shift 2;;
    --cmd) CMD="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$MODE" || -z "$OUT" ]]; then
  echo "Missing required args --mode/--out" >&2
  usage
  exit 2
fi

mkdir -p "$(dirname "$OUT")"

case "$MODE" in
  serial)
    if [[ -z "$PORT" ]]; then
      echo "--port is required for --mode serial" >&2
      exit 2
    fi
    if [[ ! -e "$PORT" ]]; then
      echo "Serial port does not exist: $PORT" >&2
      exit 1
    fi
    stty -F "$PORT" "$BAUD" raw -echo -echoe -echok
    # Keep line buffering so LOG/TRACE lines are flushed deterministically.
    timeout "${DURATION}s" stdbuf -oL cat "$PORT" > "$OUT" || true
    ;;
  cmd)
    if [[ -z "$CMD" ]]; then
      echo "--cmd is required for --mode cmd" >&2
      exit 2
    fi
    timeout "${DURATION}s" bash -lc "$CMD" > "$OUT" || true
    ;;
  *)
    echo "Unsupported mode: $MODE" >&2
    exit 2
    ;;
esac

echo "board log captured: $OUT"
