# plc-gen Troubleshooting

当用户还没开始真正生成 `.plc` 就卡住时，用本文件排障。

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

这是对 scaffold 用户最省事的路径。

## 4. scenario 文件缺失

只在文件确实不存在时推荐：

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

如果项目来自 `new`，先检查 `scenarios/nominal/normal.yaml` 是否已经存在。

## 5. 用户要求“优化命令”

不要编造 CLI。直接说明：
- 当前没有 optimization subcommand
- 现有 optimization 能力在 Rust library API：`rust_plc::optimization`
- 如需准确认识边界，读 `references/optimization.md`

## 6. `scenario-*` 或 `no-board-gate` 报 `unsupported guard expression`

已观察到复杂 PLC 在当前 scenario 工具链下可能触发：

```text
unsupported guard expression in <task.step>: <expr>
```

做法：
- 不要直接把它说成“PLC 一定写错了”
- 先明确这是当前 toolchain 兼容性限制
- 如果用户必须跑当前 scenario 工具链，再考虑把关键复合 `wait` guard 拆成更细的 helper step / readiness gate
- 如果业务语义允许，优先改成顺序单条件 `wait`
- 如果用户只是要 DSL 交付而不是立刻跑通当前 scenario 工具链，应把状态写成 `blocked by toolchain limitation`，而不是假装 `validated`

## 7. `project-check` 失败时如何解释

`project-check` 不是单一步骤。它会串起 compile / verify、`sequence-lint`、`scenario-doctor`、`no-board-gate`。

做法：
- 先告诉用户是哪个子步骤失败
- 再引用 `out/project_check/...` 下的日志或报告路径
- 不要只回一句“project-check failed”就结束
