# plc-gen Troubleshooting

当用户在生成、修复或验证 RustPLC DSL source set 时卡住，用本文排障。

## 1. `cargo run --release -- new ...` 失败

原因：
- 这个 workspace 有多个 binary

正确写法：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

## 2. `cd my_plc_project` 后再跑 `cargo run ...` 失败

原因：
- scaffold 目录本身不是 Cargo 项目

修复方式：
- 如果用户装了 `rust_plc` binary，就在 scaffold 目录里直接运行 `rust_plc ...`
- 如果用户仍在 RustPLC 源码仓里运行 `cargo run --release --bin rust_plc -- ...`，就回到仓库根目录，并把 scaffold 文件路径写全

## 3. 用户没有源码，只有已安装工具

做法：
- 把 `cargo run --release --bin rust_plc -- ...` 切成 `rust_plc ...`
- 其他参数保持不变

这条路径适合已安装 binary 的项目用户。

## 4. scenario 文件缺失

按 source entry 生成或重建 scenario skeleton：

```bash
<run> scenario-init <source.plc|source.bundle.toml> --out <scenario.yaml> --preset normal
```

如果项目来自 `new`，先检查 `scenarios/nominal/normal.yaml` 是否已经存在。

## 5. 用户要求“优化命令”

直接说明：
- 当前没有 optimization subcommand
- 现有 optimization 能力在 Rust library API，路径是 `rust_plc::optimization`
- 如需准确识别边界，读取 `references/optimization.md`

## 6. `scenario-*` 或 `no-board-gate` 报 `unsupported guard expression`

已观察到复杂 PLC 在当前 scenario 工具链下可能触发：

```text
unsupported guard expression in <task.step>: <expr>
```

做法：
- 先把状态表述为当前 toolchain 兼容性限制
- 如果用户必须跑当前 scenario 工具链，再考虑把关键复合 `wait` guard 拆成更细的 helper step 或 readiness gate
- 如果业务语义允许，优先改成顺序单条件 `wait`
- 如果当前目标是 DSL 交付而不是立即跑通 scenario 工具链，状态写成 `blocked by toolchain limitation`

## 7. `project-check` 失败时如何解释

`project-check` 不是单一步骤。它会串起 compile / verify、`sequence-lint`、`scenario-doctor`、`no-board-gate`。

做法：
- 先告诉用户是哪个子步骤失败
- 再引用 `out/project_check/...` 下的日志或报告路径
- 给出下一条最小复现或排查命令
