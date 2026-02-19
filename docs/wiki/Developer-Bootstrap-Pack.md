# Developer Bootstrap Pack

Date: 2026-02-19

## One Command

```bash
cargo run --release -- new my_plc_project
```

## Generated Essentials

- `plc/main.plc`
- `scenarios/normal.yaml`
- `io_map.toml`
- `.github/workflows/no_board_gate.yml`
- `.vscode/tasks.json` + `.vscode/settings.json` + `.vscode/extensions.json`

## First Checks

```bash
cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output human
```

```bash
cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output human
```
