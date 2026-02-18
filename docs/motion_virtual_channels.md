# Motion Virtual Channels (RP2040 dev-stage convention)

This repo currently exposes stepper + AB encoder motion signals to the PLC runtime via **virtual DI/AI channels**:

- The PLC program reads them as normal `DI*` / `AI*` inputs.
- The `io_map.toml` maps those IDs to `"virtual"` so the firmware does **not** bind a physical GPIO/ADC pin.
- The `board-rp2040` firmware motion subsystem publishes synthetic values into those channels each tick.

This keeps the DSL topology explicit and portable (the PLC consumes engineering signals, not raw edges).

## 1) io_map.toml: how to declare virtual channels

Example:

```toml
[digital_inputs]
di24 = "virtual"   # axis0 enc_dir_positive (published by firmware)

[analog_inputs]
ai24 = "virtual"   # axis0 count (published by firmware)
ai25 = "virtual"   # axis0 speed (published by firmware)

[digital_outputs]
do24 = "virtual"   # axis0 enable (consumed by firmware)
do25 = "virtual"   # axis0 dir (consumed by firmware)

[analog_outputs]
ao24 = "virtual"   # axis0 vel_cmd_sps (consumed by firmware)
```

The same `"virtual"` mechanism works for any DI/DO/AI/AO channel:
- DI/DO/AO: `0..=29` or `"virtual"`
- AI: `26..=29` (ADC-capable) or `"virtual"`

## 2) Firmware-side fixed channel mapping (current stage)

Axis0:
- Commands: `DO24` = enable, `DO25` = dir, `AO24` = vel_cmd_sps
- Feedback: `AI24` = count, `AI25` = speed, `DI24` = enc_dir_positive

Axis1:
- Commands: `DO26` = enable, `DO27` = dir, `AO26` = vel_cmd_sps
- Feedback: `AI26` = count, `AI27` = speed, `DI26` = enc_dir_positive

This mapping is documented in `docs/board_rp2040.md` and implemented in `crates/board-rp2040/src/firmware/motion.rs`.

## 3) PLC topology: recommended shape

Use explicit physical channels (`X*` / `Y*` / `AI*` / `AO*`) and connect readable logical device names to them:

```plc
[topology]

device DO24: digital_output
device DO25: digital_output
device AO24: analog_output { range: 0..20000, unit: "step_s" }

device AI24: analog_input { range: 0..4000000, unit: "count", external: true }
device AI25: analog_input { range: 0..200000, unit: "count_s", external: true }
device DI24: digital_input { external: true }

device axis0_enable: digital_output { connected_to: DO24 }
device axis0_dir: digital_output { connected_to: DO25 }
device axis0_vel_cmd: analog_output { connected_to: AO24 }

device axis0_count: analog_input { connected_to: AI24, external: true }
device axis0_speed: analog_input { connected_to: AI25, external: true }
device axis0_enc_dir_positive: digital_input { connected_to: DI24, external: true }

[constraints]

[tasks]
task main:
  step enable:
    action: set axis0_enable = true
  step run:
    action: set axis0_vel_cmd = 8000
  step wait_move:
    wait: axis0_count >= 10000
    timeout: 2000ms -> goto fault
  on_complete: goto done

task fault:
  step stop:
    action: set axis0_vel_cmd = 0
  on_complete: goto done

task done:
  step halt:
```

Notes:
- The current DSL `range: A..B` does not support negative bounds, so direction is modeled as a separate DO (`axis0_dir`).
- Mark motion feedback inputs as `external: true`; they are published by firmware, not derived from local IO wiring.

