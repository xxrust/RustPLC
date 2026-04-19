#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run a local STM32F4 Discovery firmware ELF in Renode and print UART output to stdout.

Usage:
  scripts/renode/run_firmware_trace.sh --elf <firmware.elf> [--renode-bin <path>]
USAGE
}

ELF=""
RENODE_BIN_ARG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --elf) ELF="${2:-}"; shift 2;;
    --renode-bin) RENODE_BIN_ARG="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$ELF" ]]; then
  echo "Missing --elf <firmware.elf>" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ "$ELF" != /* ]]; then
  ELF="$REPO_ROOT/$ELF"
fi
if [[ ! -f "$ELF" ]]; then
  echo "Firmware ELF not found: $ELF" >&2
  exit 1
fi

RENODE_BIN="$RENODE_BIN_ARG"
if [[ -z "$RENODE_BIN" ]]; then
  if [[ -n "${RENODE_BIN:-}" && -x "${RENODE_BIN}" ]]; then
    :
  elif command -v renode >/dev/null 2>&1; then
    RENODE_BIN="$(command -v renode)"
  else
    RENODE_BIN="$($REPO_ROOT/scripts/renode/ensure_renode.sh)"
  fi
fi

RESC="$(mktemp)"
LOG_FILE="$(mktemp)"
cleanup() {
  rm -f "$RESC" "$LOG_FILE"
}
trap cleanup EXIT

cat > "$RESC" <<EOF
mach create
machine LoadPlatformDescription @platforms/boards/stm32f4_discovery.repl
sysbus LoadELF @$ELF
showAnalyzer sysbus.usart2 LoggingUartAnalyzer
logFile @$LOG_FILE
start
emulation RunFor "0.05"
quit
EOF

"$RENODE_BIN" --disable-xwt --console -e "include @$RESC" >/dev/null
cat "$LOG_FILE"
