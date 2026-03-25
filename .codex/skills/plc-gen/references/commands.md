# plc-gen Commands

本文件只记录已经在源码里真实存在、适合面向用户暴露的命令。

## Launcher Rule

只使用这两种真实 launcher：

- 已安装 binary：`rust_plc`
- source workspace：`cargo run --release --bin rust_plc --`

不要写成 `cargo run --release -- ...`。
这个 workspace 有多个 binary，短写法不可靠。

另外要注意：

- `rust_plc` binary 可以在 scaffold 目录里直接运行
- `cargo run --release --bin rust_plc -- ...` 必须从 RustPLC 源码仓根目录运行
- scaffold 本身不是 Cargo 项目

## 顶层帮助规则

不要依赖顶层 `--help` 让用户自己摸索。
当前 CLI 顶层不是一个稳定的总帮助入口。
如果需要命令说明，直接给出精确 subcommand 语法。

## Day-1 命令链

### 新项目

#### 已安装 binary

```bash
rust_plc new my_plc_project
cd my_plc_project
rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

#### source workspace

```bash
cargo run --release --bin rust_plc -- new out/my_plc_project
cargo run --release --bin rust_plc -- scenario-validate out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- scenario-doctor out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- no-board-gate out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/gate/no_board/normal --output human
```

### 仅当确实需要覆盖已有目录时

```bash
<run> new my_plc_project --force
```

### 现有 `.plc`

#### 已安装 binary

```bash
rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
rust_plc gen-st plc/main.plc --out out/codegen/st/main.st
```

#### source workspace

```bash
cargo run --release --bin rust_plc -- scenario-validate <project_dir>/plc/main.plc --scenario <project_dir>/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- scenario-doctor <project_dir>/plc/main.plc --scenario <project_dir>/scenarios/nominal/normal.yaml --output human
cargo run --release --bin rust_plc -- gen-st <project_dir>/plc/main.plc --out <project_dir>/out/codegen/st/main.st
```

## 可稳定说明的项目级命令

### 生成 scenario skeleton

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

只在 scenario 文件缺失、或用户明确要求重新生成 skeleton 时推荐。

### 导出 ST

```bash
<run> gen-st plc/main.plc --out out/codegen/st/main.st
```

### 仿真

```bash
<run> sim-plc plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl
```

### 板级基线构建

```bash
<run> build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

### 交付包

```bash
<run> release-bundle plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/release
```

## Optimization 特别说明

不要给用户编造 `optimize`、`optimization`、`rank-opt` 一类 CLI。
当前没有 optimization subcommand。
如果用户要“优化候选方案”，请改读 `references/optimization.md`，按 library API 能力说明。
