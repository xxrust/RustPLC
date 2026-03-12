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

1. Validate scenario contract:

```bash
cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

2. Run diagnostic pre-check (`scenario-doctor`):

```bash
cargo run --release --bin rust_plc -- scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

3. Run no-board regression gate:

```bash
cargo run --release --bin rust_plc -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

4. Generate ST code (optional):

```bash
cargo run --release --bin rust_plc -- gen-st plc/main.plc --out out/codegen/st/main.st
```

5. Optional RP2040 build baseline:

```bash
cargo run --release --bin rust_plc -- build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

## VS Code

- Open Command Palette and run `Tasks: Run Task`.
- Use prefixed tasks (`RustPLC: ...`) from `.vscode/tasks.json`.
- See `.vscode/README.md` for troubleshooting.
