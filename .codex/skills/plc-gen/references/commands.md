# plc-gen Commands

当调用方需要可直接运行的精确命令时，使用本文件。

## Launcher Rule

先选定一种 launcher，并保持一致：

- 已安装 binary 模式：`rust_plc`
- source workspace 模式：`cargo run --release --bin rust_plc --`

不要使用 `cargo run --release -- ...`。

## Command Discovery Rule

不要依赖顶层 `--help`。
当前 CLI 不会提供通用顶层帮助界面。
直接给出精确 subcommand 语法。

## Day-1 Commands

### Scaffold a Project

```bash
<run> new my_plc_project
```

只有在确实要覆盖时才加：

```bash
<run> new my_plc_project --force
```

### Create a Nominal Scenario Skeleton

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

只有在 scenario 文件尚不存在时才使用这个命令。
不要在 `new` 之后立刻推荐它，因为 scaffold 已经创建了 `scenarios/nominal/normal.yaml`。

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
