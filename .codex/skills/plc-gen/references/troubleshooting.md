# plc-gen Troubleshooting

当调用方在真正开始 PLC generation 之前就卡住时，使用本文件。

## Problem: `cargo run --release -- new ...` Fails

原因：

- 这个 workspace 有多个 binary

修复方式：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

## Problem: Top-Level `--help` Is Not Usable

原因：

- 当前 CLI 没有暴露通用顶层帮助界面

修复方式：

- 不要让调用方依赖顶层 `--help` 自行摸索
- 直接给出 `references/commands.md` 中的精确 subcommand 语法

## Problem: The Caller Does Not Have Source Code

修复方式：

- 把命令从 `cargo run --release --bin rust_plc -- ...` 切换为 `rust_plc ...`
- 其余参数保持不变

## Problem: No Scenario Exists Yet

修复方式：

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

然后运行：

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

如果项目来自 `new`，先检查 `scenarios/nominal/normal.yaml` 是否已经存在，再决定是否需要重新生成。
