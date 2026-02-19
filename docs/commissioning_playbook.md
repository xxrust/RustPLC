# Commissioning Playbook (Doctor + Force + Retain + Gate)

Date: 2026-02-19

This playbook provides one end-to-end commissioning flow from diagnostics to final no-board gate.

## Scope

- PLC fixture: `examples/force_override_demo.plc`
- Artifact root: `out/commissioning/`
- Tools chained in this playbook:
  - `scenario-doctor`
  - `sim-plc` (`--retain-*`, `--online-force-*`, `--online-var-*`)
  - `no-board-gate`

---

## Flow A: Nominal startup rehearsal

### Step 1) Generate nominal scenario

```bash
cargo run -- scenario-init examples/force_override_demo.plc --preset normal --out out/commissioning/nominal.yaml
```

Pass/Fail checkpoint:
- Pass: `out/commissioning/nominal.yaml` exists.
- Fail: command exits non-zero.

### Step 2) Run diagnostics (`scenario-doctor`)

```bash
cargo run -- scenario-doctor examples/force_override_demo.plc --scenario out/commissioning/nominal.yaml --output json > out/commissioning/doctor_nominal.json
```

Pass/Fail checkpoint:
- Pass: `doctor_nominal.json` has `"status":"pass"`.
- Fail: non-zero `error_count`.

### Step 3) Prepare retain config

Create `out/commissioning/retain.toml`:

```toml
schema_version = 1
[digital_inputs]
di0 = false
[digital_outputs]
do0 = false
[analog_outputs]
ao0 = 0.0
```

### Step 4) Simulate with retain enabled

```bash
cargo run -- sim-plc examples/force_override_demo.plc \
  --scenario out/commissioning/nominal.yaml \
  --out out/commissioning/nominal_trace.jsonl \
  --retain-config out/commissioning/retain.toml \
  --retain-state out/commissioning/retain_state.json
```

Pass/Fail checkpoint:
- Pass: `nominal_trace.jsonl` and `retain_state.json` are generated.
- Fail: trace missing or retain write errors.

### Step 5) Final no-board gate

```bash
cargo run -- no-board-gate examples/force_override_demo.plc \
  --scenario out/commissioning/nominal.yaml \
  --out-dir out/commissioning/gate_nominal \
  --output json > out/commissioning/gate_nominal.json
```

Pass/Fail checkpoint:
- Pass: `gate_nominal.json` has `"status":"pass"`.
- Artifacts: `gate_nominal/sil_trace.jsonl`, `gate_nominal/board_trace.jsonl`, `gate_nominal/diff_report.json`, `gate_nominal/timing_report.json`.
- Fail: gate status not `pass` or artifact missing.

---

## Flow B: Fault-injection debug rehearsal

### Step 1) Generate fault scenario

```bash
cargo run -- scenario-init examples/force_override_demo.plc --preset sensor_stuck --out out/commissioning/fault.yaml
```

### Step 2) Diagnostics with fix preview

```bash
cargo run -- scenario-doctor examples/force_override_demo.plc \
  --scenario out/commissioning/fault.yaml \
  --fix-preview \
  --output json > out/commissioning/doctor_fault.json
```

Pass/Fail checkpoint:
- Pass: `doctor_fault.json` is emitted and parseable.
- Debug signal: review `issues[]` and `suggestion` text if present.

### Step 3) Prepare online scripts

`out/commissioning/online_force.jsonl`:

```json
{"at_ms":0,"actor":"commissioning","source":"panel","channel":"DI0","value":true}
{"at_ms":40,"actor":"commissioning","source":"panel","channel":"DI0","value":null}
```

`out/commissioning/online_var.jsonl`:

```json
{"at_ms":0,"actor":"commissioning","source":"panel","variable":"BOOL:diag_latch","value":true}
{"at_ms":20,"actor":"commissioning","source":"panel","variable":"REAL:gain_k","value":1.25}
{"at_ms":40,"actor":"commissioning","source":"panel","variable":"BOOL:diag_latch","value":null}
```

### Step 4) Simulate with retain + online force + online variable control

```bash
cargo run -- sim-plc examples/force_override_demo.plc \
  --scenario out/commissioning/fault.yaml \
  --out out/commissioning/fault_trace.jsonl \
  --retain-config out/commissioning/retain.toml \
  --retain-state out/commissioning/retain_state.json \
  --enable-online-force-dev \
  --online-force-script out/commissioning/online_force.jsonl \
  --online-force-audit-out out/commissioning/online_force_audit.jsonl \
  --online-var-script out/commissioning/online_var.jsonl \
  --online-var-audit-out out/commissioning/online_var_audit.jsonl
```

Pass/Fail checkpoint:
- Pass artifacts:
  - `fault_trace.jsonl`
  - `online_force_audit.jsonl`
  - `online_var_audit.jsonl`
- Fail: missing audit files, tick-alignment errors, or command exits non-zero.

### Step 5) Run no-board gate on fault scenario

```bash
cargo run -- no-board-gate examples/force_override_demo.plc \
  --scenario out/commissioning/fault.yaml \
  --out-dir out/commissioning/gate_fault \
  --output json > out/commissioning/gate_fault.json
```

Pass/Fail checkpoint:
- Pass: command completes and emits `gate_fault.json` + `gate_fault/diff_report.json`.
- Debug path: if status is fail, inspect `gate_fault.json` + `gate_fault/diff_report.json` + `gate_fault/timing_report.json`.

---

## Acceptance checklist for this playbook

- Both rehearsal flows are runnable with documented commands.
- Each step defines artifact paths and pass/fail checkpoint.
- Final gate step is always `no-board-gate`.
