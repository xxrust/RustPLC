# RP2040 (Raspberry Pi Pico) board flow

This repo includes an RP2040 firmware target in `crates/board-rp2040` that can run generated runtime programs from `.plc`.

## Prereqs

- Rust toolchain with `rustup`
- Install the RP2040 target:

```bash
rustup target add thumbv6m-none-eabi
```

## Build (ELF)

```bash
cargo build -p board-rp2040 --target thumbv6m-none-eabi
```

Or from DSL in one flow (`.plc -> verify -> generated program/io map/meta`):

```bash
cargo run --release -- build-rp2040 examples/assembly_station.plc \
  --out out/rp2040 \
  --io-map out/rp2040/io_map.toml \
  --emit-uf2 out/firmware.uf2
```

Notes:
- The firmware uses `defmt` over RTT for structured logs (`TICK` / `TRACE` / `LOG`).
- Runtime ticks are paced by RP2040 hardware timer (`1ms`), not CPU-cycle busy-loop constants.
- `cargo build -p board-rp2040` (without `--target`) builds a host stub that prints these instructions.
- Runtime output includes structured transition lines (`TRACE ...`) and DSL log action lines (`LOG ...`).

## I/O map notes (RP2040-specific)

- `[digital_inputs]` / `[digital_outputs]` / `[analog_outputs]`: GPIO range `0..=29`
- `[analog_inputs]`: GPIO range **`26..=29` only** (RP2040 ADC-capable pins)
- Firmware samples mapped `analog_inputs` each tick and exposes values in **volts** (`0.0..3.3`) to runtime `wait: AIx ...` predicates.

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

## End-to-end gate script

For reproducible board comparison, use:

```bash
scripts/rp2040_trace_gate.sh \
  --plc examples/assembly_station.plc \
  --io-map out/rp2040/io_map.toml \
  --sil-trace out/trace.jsonl \
  --out-dir out/rp2040_gate \
  --mount /media/RPI-RP2 \
  --collect-mode serial --port /dev/ttyACM0 --baud 115200 --duration 20
```

What it does:
1. `build-rp2040 --emit-uf2`
2. `flash-rp2040` (dry-run + actual, when `--mount` is set)
3. collect board log (`serial` or custom `cmd`)
4. `trace-parse` + `trace-diff --fail-on-mismatch`
