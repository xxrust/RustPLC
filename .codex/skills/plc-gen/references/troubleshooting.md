# plc-gen Troubleshooting

当用户还没开始真正生成 `.plc` 就卡住时，用本文件排障。

## 1. `cargo run --release -- new ...` 失败

原因：

- 这个 workspace 有多个 binary

正确写法：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

## 1.5 `cd my_plc_project` 后再跑 `cargo run ...` 失败

原因：

- scaffold 目录本身不是 Cargo 项目

修复方式：

- 如果用户装了 `rust_plc` binary，就直接在 scaffold 目录里运行 `rust_plc ...`
- 如果用户仍在 RustPLC 源码仓里运行 `cargo run --release --bin rust_plc -- ...`，就回到仓库根目录，并把 scaffold 文件路径写全

## 2. 顶层 `--help` 不好用

原因：

- 当前 CLI 不是一个稳定的总帮助入口

做法：

- 不让用户自己靠 `--help` 猜
- 直接给出精确 subcommand 命令

## 3. 用户没有源码，只有安装好的工具

做法：

- 把 `cargo run --release --bin rust_plc -- ...` 切成 `rust_plc ...`
- 其他参数不变

这是对 scaffold 用户最省事的路径。

## 4. scenario 文件缺失

只在文件确实不存在时推荐：

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

如果项目来自 `new`，先检查 `scenarios/nominal/normal.yaml` 是否已经存在。

## 5. 用户要求“优化命令”

不要编造 CLI。
直接说明：

- 当前没有 optimization subcommand
- 现有 optimization 能力在 Rust library API：`rust_plc::optimization`
- 如需准确说明，读 `references/optimization.md`
