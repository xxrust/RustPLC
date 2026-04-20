# RP2040 (Raspberry Pi Pico) board flow

This repo includes an RP2040 firmware target in `crates/board-rp2040` that can run generated runtime programs from `.plc`.

## Prereqs

- Rust toolchain with `rustup`
- Install the RP2040 target:

```bash
rustup target add thumbv6m-none-eabi
```

If you want to emit UF2 from the CLI (`build-rp2040 --emit-uf2`), install:

```bash
cargo install elf2uf2-rs
```

## Build (ELF)

```bash
cargo build -p board-rp2040 --target thumbv6m-none-eabi
```

Or from DSL in one flow (`.plc -> verify -> generated program/io map/meta`):

```bash
# Step 1: generate artifacts (includes `io_map.template.toml`)
cargo run --release -- build-rp2040 examples/rp2040_motion_minimal.plc --out out/rp2040

# Step 2: copy + edit the IO map (pin mapping is board-specific)
cp out/rp2040/io_map.template.toml out/rp2040/io_map.toml

# Step 3 (optional): build firmware + UF2 (requires `thumbv6m-none-eabi` + `elf2uf2-rs`)
cargo run --release -- build-rp2040 examples/rp2040_motion_minimal.plc \
  --out out/rp2040 \
  --io-map out/rp2040/io_map.toml \
  --emit-uf2 out/firmware.uf2
```

Notes:
- The firmware uses `defmt` over RTT for structured logs (`TICK` / `TRACE` / `LOG`).
- Runtime ticks are paced by RP2040 hardware timer (`1ms`), not CPU-cycle busy-loop constants.
- `cargo build -p board-rp2040` (without `--target`) builds a host stub that prints these instructions.
- Runtime output includes structured transition lines (`TRACE ...`) and DSL log action lines (`LOG ...`).
- `build-rp2040` now also emits `analog_contract.toml` (AI/AO engineering ranges + AO ramp) and `analog_calibration.template.toml` (optional AI/AO scale/offset).

## I/O map notes (RP2040-specific)

- `[digital_inputs]` / `[digital_outputs]` / `[analog_outputs]`: GPIO range `0..=29` or `"virtual"`
- `[analog_inputs]`: GPIO range **`26..=29` only** (RP2040 ADC-capable pins) or `"virtual"`
- Firmware samples mapped `analog_inputs` each tick, reads ADC voltage (`0.0..3.3V`), then linearly maps to engineering range from `analog_contract.toml`.
- Firmware applies optional per-channel calibration: `eng_cal = eng_raw * scale + offset` (from `analog_contract.toml`, override via `build-rp2040 --analog-calibration`).
- Firmware applies `analog_outputs` via PWM and supports per-channel ramp (`ramp_ms`) from `analog_contract.toml`.

Example:

```toml
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16

[analog_inputs]
ai0 = 26

[analog_outputs]
ao0 = 20
```

### Virtual channels (no physical GPIO binding)

If you set a DI/DO/AI/AO mapping to `"virtual"`, the build toolchain will accept the channel ID, but the RP2040 firmware will **not** bind it to a physical GPIO:
- virtual DI: firmware returns the last synthetic value written by a board subsystem (e.g. motion)
- virtual DO: firmware latches the program value, but does not drive a pin
- virtual AI: firmware does not sample ADC; a board subsystem may publish a synthetic value
- virtual AO: firmware latches the program value, but does not drive PWM

This is the intended mechanism for exposing motion feedback as DSL-visible AI/DI channels without consuming ADC-capable pins.

### Motion config (optional, dual-axis template)

When you need board-level Pulse/Dir + AB encoder support, add a `motion` section to `io_map.toml`.

Example:

```toml
[motion.stepper.axis0]
step_gpio = 2
dir_gpio = 3
en_gpio = 4
dir_inverted = false
v_max_sps = 20000
acc_sps2 = 40000
dec_sps2 = 40000

[motion.stepper.axis1]
step_gpio = 5
dir_gpio = 6
en_gpio = 7
dir_inverted = false
v_max_sps = 20000
acc_sps2 = 40000
dec_sps2 = 40000

[motion.encoder.axis0]
a_gpio = 8
b_gpio = 9
ppr = 1024
quad = 4
count_sign = "normal"  # normal | inverted
scale = 1.0

[motion.encoder.axis1]
a_gpio = 10
b_gpio = 11
ppr = 1024
quad = 4
count_sign = "normal"
scale = 1.0
```

Validation rules (current stage):
- axis key must be `axis0` or `axis1`
- GPIO fields must be in `0..=29`
- stepper `step_gpio/dir_gpio/en_gpio` must be distinct
- encoder `a_gpio/b_gpio` must be distinct
- stepper trapezoid defaults: if any of `v_max_sps/acc_sps2/dec_sps2` is set, all three must be set, and each must be `> 0`
- encoder `ppr` must be `> 0`
- encoder `quad` must be one of `1`, `2`, `4`
- encoder `count_sign` must be `normal` or `inverted`
- encoder `scale` must be finite and `> 0`
- all GPIO assignments are validated for duplicates across DI/DO/AI/AO/motion pins

### Motion command/feedback channels (current convention)

In the current dev stage, the firmware motion subsystem consumes command channels and publishes feedback channels using the following fixed IDs:

Axis0:
- Commands (from PLC outputs): `DO24` = enable, `DO25` = dir, `AO24` = vel_cmd_sps
- Feedback (to PLC inputs): `AI24` = count, `AI25` = speed, `DI24` = enc_dir_positive

Axis1:
- Commands (from PLC outputs): `DO26` = enable, `DO27` = dir, `AO26` = vel_cmd_sps
- Feedback (to PLC inputs): `AI26` = count, `AI27` = speed, `DI26` = enc_dir_positive

Recommendation: in `io_map.toml`, map these channels to `"virtual"` so they do not collide with physical GPIO/ADC usage.

Encoder signal semantics (current stage):
- `count` is published as a signed engineering value: `count = raw * (quad/2) * scale`, where the PIO counter tracks A-edge counts (base quad=2).
- `speed` is derived at tick boundaries: `speed_inst = delta / dt`, then published as a low-pass filtered value (alpha=0.2) to mitigate edge jitter.
- `enc_dir_positive` is computed from the raw signed delta sign (after applying count_sign).

## End-to-end gate script

For reproducible board comparison, use:

```bash
scripts/rp2040_trace_gate.sh \
  --plc examples/rp2040_motion_minimal.plc \
  --io-map out/rp2040/io_map.toml \
  --sil-trace out/trace.jsonl \
  --out-dir out/rp2040_gate \
  --mount /media/RPI-RP2 \
  --max-p99-exec-us 2000 \
  --max-overrun-count 0 \
  --collect-mode serial --port /dev/ttyACM0 --baud 115200 --duration 20
```

What it does:
1. `build-rp2040 --emit-uf2`
2. `flash-rp2040` (dry-run + actual, when `--mount` is set)
3. collect board log (`serial` or custom `cmd`)
4. `board-parse` + `trace-diff --fail-on-mismatch`
5. evaluate realtime timing thresholds and write `timing_gate_verdict.json`
6. render `trace_diff_dashboard.html`

## HIL regression script (real board)

Daily multi-case gate entry (motion + fail-safe bundles):

```bash
scripts/rp2040_hil_daily_gate.sh \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --duration 20 \
  --max-p99-exec-us 2000 \
  --max-overrun-count 0 \
  --out-root out/rp2040_hil_daily_gate \
  --bundle
```

Per-case outputs include `hil_summary.json`, `diff_report.json`, `timing_report.json`,
`timing_gate_verdict.json`, `trace_diff_dashboard.html`, and `assertions_report.json`
(`axis/signal/step/tick` context on failure).

Timing threshold tuning workflow:

1. Run nightly without strict limits for several days and collect baseline `timing_report.json`.
2. Set `max_p99_exec_us` around baseline peak p99 with a practical margin (usually 1.2~1.5x).
3. Keep `max_overrun_count` at `0` unless you have a documented temporary waiver.
4. Treat `timing_gate_verdict.json.violations` as the single source for threshold-fail root cause.

For a single-case debug run, use the lower-level script:

```bash
scripts/rp2040_hil_gate.sh \
  --plc examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/count_stuck.yaml \
  --io-map examples/rp2040_motion_minimal.io_map.toml \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --out-dir out/rp2040_hil_single_case \
  --bundle
```

## Abnormal-exit safety matrix (A/B/C/D)

Reference assets:
- `scenarios/rp2040_hil_gate/abnormal_exit/matrix.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence_schema.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/*.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/class_d_checklist_template.json`

Verifier command (A/B/C auto, D manual hardware chain):

```bash
python3 scripts/abnormal_exit_matrix_verify.py \
  --matrix scenarios/rp2040_hil_gate/abnormal_exit/matrix.json \
  --evidence-dir scenarios/rp2040_hil_gate/abnormal_exit/evidence \
  --out out/rp2040_hil_daily_gate/abnormal_exit_report.json
```

## PIL-style gate (no physical board)

```bash
scripts/pil_trace_gate.sh \
  --sil examples/trace_golden/sil_trace.jsonl \
  --out-dir out/pil_gate \
  --board-log examples/trace_golden/board_log_match.log
```

Or capture log from a simulator command:

```bash
scripts/pil_trace_gate.sh \
  --sil out/trace.jsonl \
  --out-dir out/pil_gate \
  --runner-cmd "renode -e 'include @scripts/renode/run.resc'" \
  --duration 30
```

Both PIL and board gate scripts also emit `trace_diff_dashboard.html`.
