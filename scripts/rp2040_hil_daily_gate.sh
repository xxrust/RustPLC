#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run RP2040 daily HIL gate with bundled motion + fail-safe cases.

Usage:
  scripts/rp2040_hil_daily_gate.sh \
    --mount <rp2040_mount> \
    --port <tty> \
    [--cases <cases.json>] \
    [--baud <n>] \
    [--duration <sec>] \
    [--max-p99-exec-us <us>] \
    [--max-overrun-count <n>] \
    [--out-root <dir>] \
    [--bundle]

Defaults:
  --cases scenarios/rp2040_hil_gate/cases.json
  --out-root out/rp2040_hil_daily_gate
  --baud 115200
  --duration 20

Per-case artifacts are written under:
  <out-root>/<case-id>/

Aggregate outputs:
  <out-root>/hil_daily_summary.json
USAGE
}

MOUNT=""
PORT=""
CASES_JSON="scenarios/rp2040_hil_gate/cases.json"
BAUD="115200"
DURATION="20"
OUT_ROOT="out/rp2040_hil_daily_gate"
BUNDLE="0"
MAX_P99_EXEC_US=""
MAX_OVERRUN_COUNT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mount) MOUNT="${2:-}"; shift 2 ;;
    --port) PORT="${2:-}"; shift 2 ;;
    --cases) CASES_JSON="${2:-}"; shift 2 ;;
    --baud) BAUD="${2:-}"; shift 2 ;;
    --duration) DURATION="${2:-}"; shift 2 ;;
    --out-root) OUT_ROOT="${2:-}"; shift 2 ;;
    --max-p99-exec-us) MAX_P99_EXEC_US="${2:-}"; shift 2 ;;
    --max-overrun-count) MAX_OVERRUN_COUNT="${2:-}"; shift 2 ;;
    --bundle) BUNDLE="1"; shift 1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$MOUNT" || -z "$PORT" ]]; then
  echo "Missing required args: --mount/--port" >&2
  usage
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$OUT_ROOT" = /* ]]; then
  OUT_ROOT_ABS="$OUT_ROOT"
else
  OUT_ROOT_ABS="$REPO_ROOT/$OUT_ROOT"
fi
if [[ "$CASES_JSON" = /* ]]; then
  CASES_JSON_ABS="$CASES_JSON"
else
  CASES_JSON_ABS="$REPO_ROOT/$CASES_JSON"
fi

if [[ ! -f "$CASES_JSON_ABS" ]]; then
  echo "Cases file not found: $CASES_JSON_ABS" >&2
  exit 1
fi

mkdir -p "$OUT_ROOT_ABS"
SUMMARY_JSON="$OUT_ROOT_ABS/hil_daily_summary.json"

CASES_TSV="$(mktemp)"
CASE_SUMMARIES_TXT="$(mktemp)"
trap 'rm -f "$CASES_TSV" "$CASE_SUMMARIES_TXT"' EXIT

python3 - "$CASES_JSON_ABS" > "$CASES_TSV" <<'PY'
import json
import sys

path = sys.argv[1]
obj = json.load(open(path, "r", encoding="utf-8"))
cases = obj.get("cases", [])
for case in cases:
    values = [
        case.get("id", ""),
        case.get("title", ""),
        case.get("focus", ""),
        case.get("plc", ""),
        case.get("scenario", ""),
        case.get("io_map", ""),
        case.get("assertions", ""),
        str(case.get("duration_sec", "")),
    ]
    print("\t".join(v.replace("\t", " ") for v in values))
PY

if [[ ! -s "$CASES_TSV" ]]; then
  echo "No cases found in $CASES_JSON_ABS" >&2
  exit 1
fi

overall_status=0
case_count=0

echo "[daily-gate] cases file: $CASES_JSON"
while IFS=$'\t' read -r CASE_ID CASE_TITLE CASE_FOCUS CASE_PLC CASE_SCENARIO CASE_IO_MAP CASE_ASSERTIONS CASE_DURATION; do
  if [[ -z "$CASE_ID" || -z "$CASE_PLC" || -z "$CASE_SCENARIO" || -z "$CASE_IO_MAP" ]]; then
    echo "Invalid case entry in $CASES_JSON: id/plc/scenario/io_map are required" >&2
    exit 1
  fi

  case_count=$((case_count + 1))
  case_out="$OUT_ROOT_ABS/$CASE_ID"
  mkdir -p "$case_out"

  run_duration="$DURATION"
  if [[ -n "$CASE_DURATION" ]]; then
    run_duration="$CASE_DURATION"
  fi

  echo "[daily-gate] case=$CASE_ID focus=$CASE_FOCUS"
  gate_args=(
    --plc "$CASE_PLC"
    --scenario "$CASE_SCENARIO"
    --io-map "$CASE_IO_MAP"
    --mount "$MOUNT"
    --port "$PORT"
    --baud "$BAUD"
    --duration "$run_duration"
    --out-dir "$case_out"
  )
  if [[ -n "$MAX_P99_EXEC_US" ]]; then
    gate_args+=(--max-p99-exec-us "$MAX_P99_EXEC_US")
  fi
  if [[ -n "$MAX_OVERRUN_COUNT" ]]; then
    gate_args+=(--max-overrun-count "$MAX_OVERRUN_COUNT")
  fi
  if [[ "$BUNDLE" == "1" ]]; then
    gate_args+=(--bundle)
  fi

  set +e
  "$REPO_ROOT/scripts/rp2040_hil_gate.sh" "${gate_args[@]}"
  gate_status=$?
  set -e

  assertions_status=0
  assertions_report="$case_out/assertions_report.json"
  first_failure_context="null"

  if [[ $gate_status -eq 0 && -n "$CASE_ASSERTIONS" ]]; then
    if [[ "$CASE_ASSERTIONS" = /* ]]; then
      assertions_path="$CASE_ASSERTIONS"
    else
      assertions_path="$REPO_ROOT/$CASE_ASSERTIONS"
    fi

    if [[ ! -f "$assertions_path" ]]; then
      echo "Assertion spec not found: $assertions_path" >&2
      assertions_status=2
      first_failure_context='{"message":"assertion spec not found"}'
    else
      set +e
      python3 "$REPO_ROOT/scripts/hil_case_assert.py" \
        --spec "$assertions_path" \
        --trace "$case_out/board_trace.jsonl" \
        --out "$assertions_report"
      assertions_status=$?
      set -e

      if [[ -f "$assertions_report" ]]; then
        first_failure_context="$(python3 - "$assertions_report" <<'PY'
import json
import sys

obj = json.load(open(sys.argv[1], "r", encoding="utf-8"))
ctx = obj.get("first_failure_context")
print(json.dumps(ctx, ensure_ascii=False))
PY
)"
      fi
    fi
  fi

  case_status="pass"
  if [[ $gate_status -ne 0 || $assertions_status -ne 0 ]]; then
    case_status="fail"
    overall_status=2
  fi

  case_summary="$case_out/case_summary.json"
  export CASE_ID CASE_TITLE CASE_FOCUS CASE_PLC CASE_SCENARIO CASE_IO_MAP CASE_ASSERTIONS
  export RUN_DURATION="$run_duration"
  export CASE_STATUS="$case_status"
  export GATE_STATUS="$gate_status"
  export ASSERTIONS_STATUS="$assertions_status"
  export CASE_OUT="$case_out"
  export ASSERTIONS_REPORT="$assertions_report"
  export FIRST_FAILURE_CONTEXT="$first_failure_context"
  export CASE_SUMMARY_PATH="$case_summary"

  python3 - <<'PY'
import json
import os
from pathlib import Path

try:
    first_failure = json.loads(os.environ.get("FIRST_FAILURE_CONTEXT", "null"))
except json.JSONDecodeError:
    first_failure = {"message": os.environ.get("FIRST_FAILURE_CONTEXT", "")}

summary = {
    "id": os.environ["CASE_ID"],
    "title": os.environ["CASE_TITLE"],
    "focus": os.environ["CASE_FOCUS"],
    "status": os.environ["CASE_STATUS"],
    "inputs": {
        "plc": os.environ["CASE_PLC"],
        "scenario": os.environ["CASE_SCENARIO"],
        "io_map": os.environ["CASE_IO_MAP"],
        "duration_sec": int(os.environ["RUN_DURATION"]),
    },
    "gate": {
        "status_code": int(os.environ["GATE_STATUS"]),
        "summary": str(Path(os.environ["CASE_OUT"]) / "hil_summary.json"),
        "diff_report": str(Path(os.environ["CASE_OUT"]) / "diff_report.json"),
        "dashboard": str(Path(os.environ["CASE_OUT"]) / "trace_diff_dashboard.html"),
    },
    "assertions": {
        "status_code": int(os.environ["ASSERTIONS_STATUS"]),
        "spec": os.environ["CASE_ASSERTIONS"],
        "report": os.environ["ASSERTIONS_REPORT"],
        "first_failure_context": first_failure,
    },
}

Path(os.environ["CASE_SUMMARY_PATH"]).write_text(
    json.dumps(summary, indent=2, ensure_ascii=False) + "\n",
    encoding="utf-8",
)
PY

  echo "$case_summary" >> "$CASE_SUMMARIES_TXT"
done < "$CASES_TSV"

python3 - "$SUMMARY_JSON" "$CASE_SUMMARIES_TXT" "$overall_status" "$case_count" <<'PY'
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
case_paths_file = Path(sys.argv[2])
overall_status = int(sys.argv[3])
case_count = int(sys.argv[4])

cases = []
for line in case_paths_file.read_text(encoding="utf-8").splitlines():
    p = Path(line.strip())
    if p.exists():
        cases.append(json.loads(p.read_text(encoding="utf-8")))

summary = {
    "status": "pass" if overall_status == 0 else "fail",
    "status_code": overall_status,
    "case_count": case_count,
    "cases": cases,
}
summary_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
PY

echo "[daily-gate] summary: $SUMMARY_JSON"
exit "$overall_status"
