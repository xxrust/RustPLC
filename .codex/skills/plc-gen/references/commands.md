# plc-gen Commands

Use this file when the caller needs exact runnable commands.

## Launcher Rule

Choose one launcher and keep it consistent:

- installed binary mode: `rust_plc`
- source workspace mode: `cargo run --release --bin rust_plc --`

Do not use `cargo run --release -- ...`.

## Command Discovery Rule

Do not depend on top-level `--help`.
Current CLI behavior does not provide a generic top-level help screen.
Give exact subcommand syntax instead.

## Day-1 Commands

### Scaffold a Project

```bash
<run> new my_plc_project
```

Overwrite only when intended:

```bash
<run> new my_plc_project --force
```

### Create a Nominal Scenario Skeleton

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

Use this only when the scenario file does not already exist.
Do not recommend it right after `new`, because the scaffold already creates `scenarios/nominal/normal.yaml`.

### Validate the PLC Against the Scenario

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

### Run Pre-Runtime Diagnosis

```bash
<run> scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

### Run the No-Board Gate

```bash
<run> no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

### Export IEC 61131-3 Structured Text

```bash
<run> gen-st plc/main.plc --out out/codegen/st/main.st
```

## Typical Sequences

### New Project

```bash
<run> new my_plc_project
cd my_plc_project
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

### Existing PLC

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
<run> gen-st plc/main.plc --out out/codegen/st/main.st
```
