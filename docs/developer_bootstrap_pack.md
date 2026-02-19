# Developer Bootstrap Pack（`rust_plc new`）

日期：2026-02-19

## 1. 目标

通过一条命令生成可直接跑通的项目骨架：

- PLC 示例工程
- 场景文件
- `io_map.toml`
- CI baseline（no-board gate）
- VS Code 最小支持（语法关联 + 常用命令入口）

## 2. 命令

```bash
cargo run --release -- new my_plc_project
```

可选：

```bash
cargo run --release -- new my_plc_project --force
```

## 3. 生成内容

- `README.md`（从 0 到 first gate pass 的 checklist）
- `plc/main.plc`
- `scenarios/normal.yaml`
- `io_map.toml`
- `.github/workflows/no_board_gate.yml`
- `.vscode/tasks.json`
- `.vscode/settings.json`
- `.vscode/extensions.json`

## 4. Onboarding Checklist（零到首个 gate）

1. 场景校验：

```bash
cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output human
```

2. no-board gate：

```bash
cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output human
```

3. 可选 RP2040 构建：

```bash
cargo run --release -- build-rp2040 plc/main.plc --out out/rp2040 --io-map io_map.toml
```
