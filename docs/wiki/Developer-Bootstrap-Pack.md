# Developer Bootstrap Pack

Date: 2026-03-12

## One Command

```bash
cargo run --release -- new my_plc_project
```

## Generated Essentials

- `rustplc.project.toml`
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`
- `config/io_map.toml`
- `config/retain.toml`
- `docs/project-layout.md`
- `.gitignore`
- `.github/workflows/no_board_gate.yml`
- `.vscode/tasks.json` + `.vscode/settings.json` + `.vscode/extensions.json`
- `.vscode/plc.code-snippets`
- `.vscode/README.md`

Project name is derived from `new <project_dir>` and injected into `README.md`, `plc/main.system.md`, and `rustplc.project.toml`.

## VS Code Day-1 Package

- Highlight strategy: `*.plc -> ini` (fallback, no custom extension required)
- Task entrypoints:
  - `RustPLC: scenario-init (normal)`
  - `RustPLC: scenario-validate`
  - `RustPLC: scenario-doctor`
  - `RustPLC: sim-plc`
  - `RustPLC: no-board-gate`
  - `RustPLC: gen-st`
  - `RustPLC: build-rp2040`
- Snippets:
  - `plc-skeleton`
  - `plc-wait-timeout`

## First Checks

```bash
cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

```bash
cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```
