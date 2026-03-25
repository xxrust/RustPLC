# RustPLC Commands

Use this file when the caller needs exact runnable commands.

## Launcher Rule

Choose one launcher and reuse it consistently:

- installed binary mode: `rust_plc`
- source workspace mode: `cargo run --release --bin rust_plc --`

Do not use `cargo run --release -- ...`.
This workspace contains multiple binaries, so that form fails.

## Command Discovery Rule

Do not depend on top-level `--help`.
Current CLI behavior expects a `.plc` path at the top level and does not provide a generic help screen.

If the caller asks what RustPLC can do, answer with the curated subcommand list below.
If they need usage for one subcommand, give the exact syntax directly.

## Day-1 Commands

### Create a Scaffolded Project

```bash
<run> new my_plc_project
```

Overwrite an existing target directory only when intended:

```bash
<run> new my_plc_project --force
```

### Create a Scenario Skeleton

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

Available presets:

- `minimal`
- `normal`
- `timeout`
- `sensor_stuck`
- `bounce`

### Validate PLC Against a Scenario

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

Use `--output json` when the caller wants machine-readable results.

### Diagnose a Scenario Before Runtime Work

```bash
<run> scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

Use `--fix-preview` when you want suggested repairs without mutating files.

### Run SIL Simulation

```bash
<run> sim-plc plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl
```

### Run the No-Board Gate

```bash
<run> no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

### Generate IEC 61131-3 Structured Text

```bash
<run> gen-st plc/main.plc --out out/codegen/st/main.st
```

## Delivery Commands

### Build RP2040 Delivery Inputs

```bash
<run> build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

Optional UF2 emission:

```bash
<run> build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml --emit-uf2 out/firmware.uf2
```

### Flash RP2040 Firmware

```bash
<run> flash-rp2040 --uf2 out/firmware.uf2 --mount <path>
```

### Package an Auditable Release Bundle

```bash
<run> release-bundle plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/release
```

## Response Pattern

When giving commands to a caller, first state which launcher you are using:

```text
Launcher: rust_plc
```

or:

```text
Launcher: cargo run --release --bin rust_plc --
```

Then provide only the smallest useful command sequence.

## Typical Sequences

### New Project, Fastest Safe Path

```bash
<run> new my_plc_project
cd my_plc_project
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

### Existing PLC, Validation First

```bash
<run> scenario-validate existing/main.plc --scenario existing/scenarios/normal.yaml --output human
<run> scenario-doctor existing/main.plc --scenario existing/scenarios/normal.yaml --output human
```

### Existing PLC, ST Export

```bash
<run> gen-st existing/main.plc --out out/codegen/st/main.st
```
