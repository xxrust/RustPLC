# RP2040 End-to-End Example

This package is a minimal AI/AO example to exercise:

- `.plc -> verify -> build-rp2040`
- analog contract generation (`analog_contract.toml`)
- UF2 emit + flash
- board/PIL trace parse + diff gate

## 1) Build RP2040 artifacts from PLC

```bash
cargo run --release -- build-rp2040 \
  examples/rp2040_end_to_end/pressure_station.plc \
  --out out/rp2040_e2e
```

Generated files include:

- `generated_program.rs`
- `io_map.template.toml`
- `analog_contract.toml`
- `build_meta.json`

## 2) Emit UF2 with explicit wiring

```bash
cargo run --release -- build-rp2040 \
  examples/rp2040_end_to_end/pressure_station.plc \
  --out out/rp2040_e2e \
  --io-map examples/rp2040_end_to_end/io_map.toml \
  --emit-uf2 out/rp2040_e2e/firmware.uf2
```

## 3) Flash (optional, real board)

```bash
cargo run --release -- flash-rp2040 \
  --uf2 out/rp2040_e2e/firmware.uf2 \
  --mount /media/RPI-RP2 --dry-run
```

## 4) SIL baseline for comparison (optional)

```bash
cargo run --release -- sim-regress \
  --plc-dir examples/rp2040_end_to_end \
  --scenario-dir examples/rp2040_end_to_end/scenarios \
  --artifacts-dir out/rp2040_e2e/sim_regress
```

Then use the case trace from `out/rp2040_e2e/sim_regress/*/trace.jsonl` as `--sil-trace`.

## 5) Board/PIL gate

Real board flow:

```bash
scripts/rp2040_trace_gate.sh \
  --plc examples/rp2040_end_to_end/pressure_station.plc \
  --io-map examples/rp2040_end_to_end/io_map.toml \
  --sil-trace examples/trace_golden/sil_trace.jsonl \
  --out-dir out/rp2040_e2e/board_gate \
  --collect-mode serial --port /dev/ttyACM0 --baud 115200 --duration 20
```

PIL/no-board flow:

```bash
scripts/pil_trace_gate.sh \
  --sil examples/trace_golden/sil_trace.jsonl \
  --out-dir out/rp2040_e2e/pil_gate \
  --board-log examples/trace_golden/board_log_match.log
```
