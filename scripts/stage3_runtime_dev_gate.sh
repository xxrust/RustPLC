#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d -t rust_plc_stage3_gate.XXXXXX)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "[stage3-gate] Running focused integration tests"
scripts/ci_script_contract_preflight.sh
cargo test --test online_force_control_plane --test online_variable_control_plane --test retain_persistent --test scenario_gen --test sim_regress --test new_scaffold
cargo test --test abnormal_exit_matrix --test abnormal_exit_matrix_doc
cargo test --test commissioning_playbook_doc
cargo test --test developer_bootstrap_pack_doc
cargo test --test no_board_playbook_doc

echo "[stage3-gate] Checking scenario-gen summary contract"
cargo run --bin rust_plc -- scenario-gen \
  --plc examples/rp2040_motion_minimal.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir "$TMP_DIR/scenario_gen" \
  --coverage-mode boundary-first \
  --dry-run

python3 - <<'PY' "$TMP_DIR/scenario_gen/summary.json" "$TMP_DIR/scenario_gen"
import json
import pathlib
import sys

summary_path = pathlib.Path(sys.argv[1])
out_dir = pathlib.Path(sys.argv[2])
summary = json.loads(summary_path.read_text(encoding="utf-8"))
required = ["coverage_mode", "dry_run", "template_library", "templates", "cases"]
for key in required:
    if key not in summary:
        raise SystemExit(f"missing summary key: {key}")
if summary["dry_run"] is not True:
    raise SystemExit("dry_run must be true")
cases = summary.get("cases", [])
if not cases:
    raise SystemExit("cases must be non-empty")
if "template_id" not in cases[0]:
    raise SystemExit("cases[0].template_id missing")
if (out_dir / "scenario_0001.yaml").exists():
    raise SystemExit("dry-run should not write scenario files")
PY

echo "[stage3-gate] Checking sim-regress feedback contract"
mkdir -p "$TMP_DIR/plcs" "$TMP_DIR/scenarios"
cat > "$TMP_DIR/plcs/fixture.plc" <<'PLC'
[topology]

device Y0: digital_output { purpose: "测试输出通道" }
device X0: digital_input { purpose: "测试输入通道" }

device start_button: digital_input {
    purpose: "启动按钮输入"
    driven_by: X0
}

device valve_A: solenoid_valve {
    purpose: "气缸伸缩控制阀"
    driven_by: Y0
}

device cyl_A: cylinder {
    purpose: "执行气缸"
    driven_by: valve_A
}

device sensor_ext: sensor {
    purpose: "气缸伸出到位检测"
    driven_by: X0
    detects: cyl_A.extended
}

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_button:
        wait: start_button == true
        timeout: 50ms -> goto fault

    on_complete: goto done

task fault:
    step retract_fault:
        action: retract cyl_A
    on_complete: goto done

task done:
    step halt:
PLC

cat > "$TMP_DIR/scenarios/fail.yaml" <<'YAML'
tick_ms: 10
duration_ms: 200
YAML

cargo run --bin rust_plc -- sim-regress \
  --plc-dir "$TMP_DIR/plcs" \
  --scenario-dir "$TMP_DIR/scenarios" \
  --artifacts-dir "$TMP_DIR/sim_regress" \
  --minimize-failure

python3 - <<'PY' "$TMP_DIR/sim_regress/feedback.json"
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
feedback = json.loads(path.read_text(encoding="utf-8"))
if feedback.get("schema_version") != 1:
    raise SystemExit("feedback schema_version must be 1")
entries = feedback.get("feedback")
if not isinstance(entries, list) or not entries:
    raise SystemExit("feedback entries missing")
first = entries[0]
for key in ["plc", "scenario", "failure_kind", "template_hint", "parameter_hints"]:
    if key not in first:
        raise SystemExit(f"feedback entry missing key: {key}")
if not isinstance(first["parameter_hints"], list) or not first["parameter_hints"]:
    raise SystemExit("parameter_hints must be a non-empty list")
PY

echo "[stage3-gate] PASS"
