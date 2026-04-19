# RP2040 Motion Minimal Example (Wiki Draft)

Date: 2026-02-19

This is a repo-local Wiki draft meant to be readable offline.

Source of truth:
- Motion overview: `docs/已实现/board_rp2040.md`
- io_map delta note: `docs/已实现/motion_io_map_format_delta.md`

## Goal

Provide a small, copyable fixture that demonstrates:

- Dual-axis Pulse/Dir (enable/dir/vel_cmd) command wiring
- Dual-axis AB encoder feedback wiring (count/dir sign)
- Nominal and fault paths in scenarios (timeouts)
- A regression test that can run in CI without a physical board

## Files

- PLC: `examples/rp2040_motion_minimal.plc`
- IO map: `examples/rp2040_motion_minimal.io_map.toml`
- Scenarios: `scenarios/rp2040_motion_minimal/*.yaml`
- Regression test: `tests/rp2040_motion_minimal_scenarios.rs`

## Channel Convention (current dev stage)

Axis0:
- Commands: `DO24` enable, `DO25` dir, `AO24` vel_cmd_sps
- Feedback: `AI24` count, `AI25` speed, `DI24` enc_dir_positive

Axis1:
- Commands: `DO26` enable, `DO27` dir, `AO26` vel_cmd_sps
- Feedback: `AI26` count, `AI27` speed, `DI26` enc_dir_positive

Recommendation: map those IDs to `"virtual"` in `io_map.toml` so they do not consume physical GPIO/ADC.

## Local Command Checklist

Validate scenarios against the PLC:

```bash
cargo run -- scenario-validate examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml
```

Run SIL simulation and emit a trace:

```bash
cargo run -- sim-plc examples/rp2040_motion_minimal.plc \\
  --scenario scenarios/rp2040_motion_minimal/normal.yaml \\
  --out out/rp2040_motion_minimal.normal.trace.jsonl
```

Run the CI-style regression gate:

```bash
cargo test -p rust_plc --test rp2040_motion_minimal_scenarios
```

Cross-build the firmware (matches CI `rp2040-cross-build` job):

```bash
rustup target add thumbv6m-none-eabi
cargo build -p board-rp2040 --target thumbv6m-none-eabi --release
```

## Troubleshooting Notes

- If `scenario-validate` warns about mismatch, regenerate a skeleton with:
  - `cargo run -- scenario-init examples/rp2040_motion_minimal.plc --out scenarios/rp2040_motion_minimal/<case>.yaml --preset normal`
- If the regression test fails, inspect the emitted trace JSONL and confirm:
  - at least one transition has `reason == "timeout"` for fault scenarios
  - the nominal scenario completes without timeouts
