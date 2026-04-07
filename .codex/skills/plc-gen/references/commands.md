# plc-gen Commands

本文只记录源码里真实存在、适合面向用户暴露的命令。

## Launcher Rule

只使用这两种真实 launcher：
- 已安装 binary：`rust_plc`
- source workspace：`cargo run --release --bin rust_plc --`

另外要注意：
- `rust_plc ...` 可以在 scaffold 项目目录里直接运行
- `cargo run --release --bin rust_plc -- ...` 必须在 RustPLC 源码仓根目录运行
- scaffold 本身不是 Cargo 项目

## PLC Source Entry Rule

项目与工具链的真实输入口径是：
- `<source.plc>`
- `<source.bundle.toml>`

因此：
- 单文件布局用 `.plc` 作为 source entry
- 多文件布局用 `.bundle.toml` 作为 source entry
- scenario / verification / codegen / deployment 命令统一接收 source entry，而不是只接收某个固定文件名

## Day-1 命令链

### 新项目

#### 已安装 binary

```bash
rust_plc new my_plc_project
cd my_plc_project
rust_plc project-check plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
```

#### source workspace

```bash
cargo run --release --bin rust_plc -- new out/my_plc_project
cargo run --release --bin rust_plc -- project-check out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/project_check/normal --output human
```

### 仅当确实需要覆盖已有目录时

```bash
<run> new my_plc_project --force
```

## 现有单文件 source entry

### 已安装 binary

```bash
rust_plc project-check machine.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
rust_plc gen-st machine.plc --out out/codegen/st/main.st
```

### source workspace

```bash
cargo run --release --bin rust_plc -- project-check <source.plc> --scenario <scenario.yaml> --out-dir <out_dir> --output human
cargo run --release --bin rust_plc -- gen-st <source.plc> --out <output.st>
```

## 现有 bundle source entry

### 已安装 binary

```bash
rust_plc project-check machine.bundle.toml --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
rust_plc gen-st machine.bundle.toml --out out/codegen/st/main.st
```

### source workspace

```bash
cargo run --release --bin rust_plc -- project-check <source.bundle.toml> --scenario <scenario.yaml> --out-dir <out_dir> --output human
cargo run --release --bin rust_plc -- gen-st <source.bundle.toml> --out <output.st>
```

## 可稳定说明的项目级命令

### 统一项目检查

```bash
<run> project-check <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <out_dir> --output human
```

`project-check` 当前会串起：
- compile / verify
- `sequence-lint`
- `scenario-doctor`
- `no-board-gate`

### 生成 scenario skeleton

```bash
<run> scenario-init <source.plc|source.bundle.toml> --out <scenario.yaml> --preset normal
```

通常在 scenario 文件缺失，或用户明确要求重新生成 skeleton 时推荐。

### 定点排查 scenario / gate

```bash
<run> scenario-validate <source.plc|source.bundle.toml> --scenario <scenario.yaml> --output human
<run> sequence-lint <source.plc|source.bundle.toml> --critical-wait-level error
<run> scenario-doctor <source.plc|source.bundle.toml> --scenario <scenario.yaml> --output human
<run> no-board-gate <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <out_dir> --output human
```

### 导出 ST

```bash
<run> gen-st <source.plc|source.bundle.toml> --out <output.st>
```

### 仿真

```bash
<run> sim-plc <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <trace.jsonl>
```

### 板级基线构建

```bash
<run> build-rp2040 <source.plc|source.bundle.toml> --out <out_dir> --io-map <io_map.toml>
```

### 交付包

```bash
<run> release-bundle <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out-dir <out_dir>
```

## Optimization 特别说明

不要给用户编造 `optimize`、`optimization`、`rank-opt` 一类 CLI。当前没有 optimization subcommand。

如果用户要“优化候选方案”，改读 `references/optimization.md`，按 library API 能力说明。
