# RP2040 (Raspberry Pi Pico) minimal firmware build

This repo includes a minimal Pico firmware skeleton in `crates/board-rp2040`.

## Prereqs

- Rust toolchain with `rustup`
- Install the RP2040 target:

```bash
rustup target add thumbv6m-none-eabi
```

## Build

```bash
cargo build -p board-rp2040 --target thumbv6m-none-eabi
```

Or from DSL in one flow:

```bash
cargo run --release -- build-rp2040 examples/assembly_station.plc \
  --out out/rp2040 \
  --io-map out/rp2040/io_map.toml \
  --emit-uf2 out/firmware.uf2
```

Notes:
- The binary uses `defmt` over RTT for logging and a simple busy-loop tick counter.
- `cargo build -p board-rp2040` (without `--target`) builds a host stub that prints these instructions.
- Runtime output includes structured transition lines (`TRACE ...`) and DSL log action lines (`LOG ...`).
