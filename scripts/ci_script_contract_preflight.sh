#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

required_exec_scripts=(
  "scripts/stage3_runtime_dev_gate.sh"
  "scripts/ci_no_connected_to_regression.sh"
  "scripts/rp2040_trace_gate.sh"
  "scripts/rp2040_hil_gate.sh"
  "scripts/rp2040_hil_daily_gate.sh"
  "scripts/pil_trace_gate.sh"
  "scripts/pil_trace_baseline_suite.sh"
  "scripts/pil_semantic_baseline.sh"
  "scripts/collect_board_log.sh"
)

fail=0

echo "[script-contract] Checking executable bit for required scripts"
for path in "${required_exec_scripts[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "ERROR: missing required script: $path" >&2
    fail=1
    continue
  fi
  mode="$(git ls-files --stage -- "$path" | awk '{print $1}')"
  if [[ "$mode" != "100755" ]]; then
    echo "ERROR: $path must be executable in git metadata (mode=100755, got ${mode:-<none>})" >&2
    fail=1
  fi
done

echo "[script-contract] Checking LF-only line endings for tracked .sh files"
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  if grep -q $'\r' "$path"; then
    echo "ERROR: CRLF detected in $path (expected LF-only)." >&2
    fail=1
  fi
done < <(git ls-files "*.sh")

if [[ "$fail" -ne 0 ]]; then
  echo "[script-contract] FAIL" >&2
  exit 2
fi

echo "[script-contract] PASS"
