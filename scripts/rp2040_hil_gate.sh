#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run an end-to-end HIL gate for RP2040:
  - generate SIL trace from (.plc + scenario)
  - build/flash RP2040 UF2
  - collect board log
  - board-parse + trace-diff --fail-on-mismatch
  - (optional) bundle all artifacts for later inspection

Usage:
  scripts/rp2040_hil_gate.sh \
    --plc <file.plc> \
    --scenario <scenario.yaml> \
    --io-map <io_map.toml> \
    --mount <rp2040_mount> \
    --port <tty> \
    [--max-p99-exec-us <us>] \
    [--max-overrun-count <n>] \
    [--baud <n>] \
    [--duration <sec>] \
    [--out-dir <dir>] \
    [--bundle]

Notes:
  - This script is designed for self-hosted runners (a Pico is physically connected).
  - It writes everything into --out-dir (default: out/rp2040_hil_gate).
USAGE
}

PLC=""
SCENARIO=""
IO_MAP=""
MOUNT=""
PORT=""
BAUD="115200"
DURATION="20"
OUT_DIR="out/rp2040_hil_gate"
BUNDLE="0"
MAX_P99_EXEC_US=""
MAX_OVERRUN_COUNT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plc) PLC="${2:-}"; shift 2;;
    --scenario) SCENARIO="${2:-}"; shift 2;;
    --io-map) IO_MAP="${2:-}"; shift 2;;
    --mount) MOUNT="${2:-}"; shift 2;;
    --port) PORT="${2:-}"; shift 2;;
    --max-p99-exec-us) MAX_P99_EXEC_US="${2:-}"; shift 2;;
    --max-overrun-count) MAX_OVERRUN_COUNT="${2:-}"; shift 2;;
    --baud) BAUD="${2:-}"; shift 2;;
    --duration) DURATION="${2:-}"; shift 2;;
    --out-dir) OUT_DIR="${2:-}"; shift 2;;
    --bundle) BUNDLE="1"; shift 1;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

if [[ -z "$PLC" || -z "$SCENARIO" || -z "$IO_MAP" || -z "$MOUNT" || -z "$PORT" ]]; then
  echo "Missing required args: --plc/--scenario/--io-map/--mount/--port" >&2
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
SIL_TRACE="$OUT_DIR_ABS/sil_trace.jsonl"
META_JSON="$OUT_DIR_ABS/hil_meta.json"
SUMMARY_JSON="$OUT_DIR_ABS/hil_summary.json"
DIFF_REPORT="$OUT_DIR_ABS/diff_report.json"
DASHBOARD_HTML="$OUT_DIR_ABS/trace_diff_dashboard.html"
TIMING_REPORT="$OUT_DIR_ABS/timing_report.json"
TIMING_VERDICT="$OUT_DIR_ABS/timing_gate_verdict.json"

echo "[0/3] SIL trace (sim-plc)"
(
  cd "$REPO_ROOT"
  cargo run --release -- sim-plc "$PLC" --scenario "$SCENARIO" --out "$SIL_TRACE"
)

echo "[meta] write $META_JSON"
REPO_ROOT="$REPO_ROOT" PLC="$PLC" SCENARIO="$SCENARIO" IO_MAP="$IO_MAP" MOUNT="$MOUNT" \
PORT="$PORT" BAUD="$BAUD" DURATION="$DURATION" OUT_DIR="$OUT_DIR" SIL_TRACE="$SIL_TRACE" \
MAX_P99_EXEC_US="$MAX_P99_EXEC_US" MAX_OVERRUN_COUNT="$MAX_OVERRUN_COUNT" \
META_JSON="$META_JSON" python3 - <<'PY'
import json
import os
import shutil
import subprocess
import time

def sh(cmd):
  return subprocess.check_output(cmd, text=True).strip()

repo = os.environ["REPO_ROOT"]
out = {
  "ts_unix": int(time.time()),
  "git_commit": sh(["git", "-C", repo, "rev-parse", "HEAD"]),
  "git_status_clean": (sh(["git", "-C", repo, "status", "--porcelain"]) == ""),
  "rustc": sh(["rustc", "--version"]) if shutil.which("rustc") else None,
  "cargo": sh(["cargo", "--version"]) if shutil.which("cargo") else None,
  "inputs": {
    "plc": os.environ["PLC"],
    "scenario": os.environ["SCENARIO"],
    "io_map": os.environ["IO_MAP"],
    "mount": os.environ["MOUNT"],
    "port": os.environ["PORT"],
    "baud": os.environ["BAUD"],
    "duration_sec": os.environ["DURATION"],
    "out_dir": os.environ["OUT_DIR"],
    "max_p99_exec_us": os.environ.get("MAX_P99_EXEC_US") or None,
    "max_overrun_count": os.environ.get("MAX_OVERRUN_COUNT") or None,
  },
  "artifacts": {
    "sil_trace": os.path.abspath(os.environ["SIL_TRACE"]),
  },
}

with open(os.environ["META_JSON"], "w", encoding="utf-8") as f:
  json.dump(out, f, indent=2, ensure_ascii=False)
  f.write("\n")
PY

echo "[1/3] Board gate (build/flash/collect/trace-diff)"
set +e
(
  cd "$REPO_ROOT"
scripts/rp2040_trace_gate.sh \
    --plc "$PLC" \
    --io-map "$IO_MAP" \
    --sil-trace "$SIL_TRACE" \
    --out-dir "$OUT_DIR" \
    --mount "$MOUNT" \
    ${MAX_P99_EXEC_US:+--max-p99-exec-us "$MAX_P99_EXEC_US"} \
    ${MAX_OVERRUN_COUNT:+--max-overrun-count "$MAX_OVERRUN_COUNT"} \
    --collect-mode serial \
    --port "$PORT" \
    --baud "$BAUD" \
    --duration "$DURATION"
)
STATUS=$?
set -e

if [[ "$BUNDLE" == "1" ]]; then
  echo "[2/3] Bundle artifacts"
  BUNDLE_TGZ="$OUT_DIR_ABS/hil_bundle.tgz"
  # Tar is stable, cross-platform enough for CI artifact upload.
  tar -C "$OUT_DIR_ABS" -czf "$BUNDLE_TGZ" .
  echo "bundle: $BUNDLE_TGZ"
else
  echo "[2/3] Skip bundle (--bundle not set)"
fi

REPO_ROOT="$REPO_ROOT" OUT_DIR_ABS="$OUT_DIR_ABS" META_JSON="$META_JSON" \
SUMMARY_JSON="$SUMMARY_JSON" DIFF_REPORT="$DIFF_REPORT" SIL_TRACE="$SIL_TRACE" \
DASHBOARD_HTML="$DASHBOARD_HTML" STATUS="$STATUS" TIMING_REPORT="$TIMING_REPORT" \
TIMING_VERDICT="$TIMING_VERDICT" python3 - <<'PY'
import json
import os
from pathlib import Path

meta_path = Path(os.environ["META_JSON"])
summary_path = Path(os.environ["SUMMARY_JSON"])
diff_path = Path(os.environ["DIFF_REPORT"])
status = int(os.environ["STATUS"])
timing_verdict_path = Path(os.environ["TIMING_VERDICT"])

meta = {}
if meta_path.exists():
    meta = json.loads(meta_path.read_text(encoding="utf-8"))

timing_gate = None
if timing_verdict_path.exists():
    timing_gate = json.loads(timing_verdict_path.read_text(encoding="utf-8"))

summary = {
    "status_code": status,
    "status": "pass" if status == 0 else "fail",
    "artifacts": {
        "out_dir": os.environ["OUT_DIR_ABS"],
        "sil_trace": os.environ["SIL_TRACE"],
        "diff_report": str(diff_path),
        "dashboard_html": os.environ["DASHBOARD_HTML"],
        "timing_report": os.environ["TIMING_REPORT"],
        "timing_verdict": os.environ["TIMING_VERDICT"],
    },
    "timing_gate": timing_gate,
    "meta": meta,
}
summary_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
PY

if [[ -f "$DIFF_REPORT" ]] && command -v python3 >/dev/null 2>&1; then
  "$REPO_ROOT/scripts/trace_diff_dashboard.py" \
    --diff "$DIFF_REPORT" \
    --meta "$META_JSON" \
    --out "$DASHBOARD_HTML" \
    --title "RP2040 HIL Gate" \
    --sil-trace "$SIL_TRACE" \
    --board-trace "$OUT_DIR_ABS/board_trace.jsonl" \
    --board-log "$OUT_DIR_ABS/board.log"
fi

echo "[3/3] Done (exit=$STATUS)"
echo "summary: $SUMMARY_JSON"
exit "$STATUS"
