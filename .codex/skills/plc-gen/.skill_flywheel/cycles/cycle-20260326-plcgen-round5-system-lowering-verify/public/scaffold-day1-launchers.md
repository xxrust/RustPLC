# Scaffold Day-1 Launchers

先只判断一件事：用户是在用已安装好的 `rust_plc`，还是仍在 RustPLC 源码仓里用 `cargo run`。

## 1. 已安装 `rust_plc`

这是对陌生用户最省事的路径：

```bash
rust_plc new my_plc_project
cd my_plc_project
```

进入 scaffold 目录后，继续用：

```bash
rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

## 2. RustPLC 源码仓 workspace

必须一直留在 RustPLC 仓库根目录执行：

```bash
cargo run --release --bin rust_plc -- new out/my_plc_project
cargo run --release --bin rust_plc -- scenario-validate out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- scenario-doctor out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- no-board-gate out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/gate/no_board/normal --output human
```

不要写成：

- `cargo run --release -- ...`
- 先 `cd` 进 scaffold 目录后再跑 `cargo run --release --bin rust_plc -- ...`

因为这个 workspace 有多个 binary，而 scaffold 本身不是 Cargo 项目。
