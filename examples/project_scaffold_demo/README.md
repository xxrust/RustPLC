# Project Scaffold Demo

## Project Identity

- Project slug: `project_scaffold_demo`
- Manifest: `rustplc.project.toml`

## Project Layout

- `plc/main.system.md`: human/AI confirmed system intent
- `plc/main.plc`: executable RustPLC DSL
- `scenarios/nominal/normal.yaml`: nominal regression scenario
- `config/io_map.toml`: deployment I/O mapping
- `config/retain.toml`: retain/persistence baseline
- `out/`: all generated artifacts (sim/gate/codegen/build/release)

## Quick Start Checklist

Use one of these two launcher modes:

- Installed binary:
  Run `rust_plc ...` directly inside this scaffold directory.
- Source workspace:
  Run `cargo run --release --bin rust_plc -- ...` from the RustPLC repo root and use full paths such as `examples/project_scaffold_demo/plc/main.plc`.

The scaffold itself is not a Cargo project.

1. Validate scenario contract:

```bash
rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

2. Run diagnostic pre-check (`scenario-doctor`):

```bash
rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

3. Run no-board regression gate:

```bash
rust_plc no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

4. Generate ST code (optional):

```bash
rust_plc gen-st plc/main.plc --out out/codegen/st/main.st
```

5. Optional RP2040 build baseline:

```bash
rust_plc build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

## VS Code

- Open Command Palette and run `Tasks: Run Task`.
- Use prefixed tasks (`RustPLC: ...`) from `.vscode/tasks.json`.
- See `.vscode/README.md` for troubleshooting.
