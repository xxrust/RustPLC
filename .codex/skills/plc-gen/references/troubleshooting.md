# plc-gen Troubleshooting

## Recent generated-project failures to recognize

### Workpiece underflow on the next cycle

Typical diagnostics include:

```text
workpiece_flow: transfer reads endpoint ... before any free-standing workpiece is available
```

Likely cause:
- the generated PLC looped back for another cycle
- the workpiece ingress was only seeded once
- no modeled upstream task or scenario event replenished the source site

Fix:
- if the authored process is a single-piece acceptance flow, make the nominal success path terminal
- if the process is repeating, model replenishment explicitly before the next `acquire` or `transfer`
- do not treat `ingress_sites` as infinite supply unless that behavior is part of the system contract and executable evidence

### Terminal state still holds workpieces

Typical diagnostics include:

```text
reachable terminal state still holds workpieces at ...
```

Likely cause:
- a generic fault handler terminates the task without consuming the workpiece
- the workpiece can be at multiple stages when a fault route fires

Fix:
- split fault handlers by actual workpiece location or holder
- finish, reject, or transfer the active workpiece from the stage where it is reachable
- rerun verification after enumerating all normal and fault terminal paths

### Intent contract source kind is rejected

Typical diagnostics include JSON parse errors such as:

```text
unknown variant `system_contract`
unknown variant `patent`
```

The current intent-contract schema only accepts:
- `architecture_doc`
- `canonical_example`
- `authored_asset`

Fix:
- represent patent-derived notes, `*.system.md`, and generated delivery docs as `authored_asset`
- keep the exact source identity in `path`, `description`, and review-basis labels
- only introduce new source kinds after extending `src/intent_alignment/contract.rs` and its tests

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

## 8. `intent_alignment` 报 `invalid_contract`

常见原因：
- sibling `*.intent_alignment.contract.json` 仍是 scaffold placeholder
- 只修了 `docs/*.intent_alignment.contract.json`，但 source entry 同级 sibling sidecar 仍存在并被 `project-check` 优先选中
- `source_ref` / `review_basis[*].source` 不能从当前 launcher workspace root 解析

做法：
- 先确认 source entry 同级是否存在 sibling sidecar；若存在，它就是 `project-check` 默认优先消费的 contract
- 不要只修 docs-sidecar 而放着 sibling placeholder 不管
- 对放在 `out/...` 下的生成项目，优先把 contract source 路径写成 repo-root-relative 可解析路径

## 9. 过程站被 `workpiece required=true` 拦住

常见场景：
- 热处理炉
- 压力/流量/温控回路
- 阀站、隔离站
- 其他没有离散件流转的 process-only 资产

如果项目没有真实 `acquire/transfer/finish` 语义，却被 project policy 要求 first-class workpiece：

- 不要伪造一套假 workpiece 流程
- 把 `config/workpiece.toml` 改成 deliberate no-workpiece exception
- 然后再继续 compile / sim / gate

## 10. Raw AI/AO process-control boundary

如果需求需要压力、温度、比例阀或 PID 这类工程量过程控制，但当前设备语义库没有对应契约：

- 不要继续把它包装成“只差一点就 validated”
- 优先改用已经存在的过程设备语义动作
- 如果缺少设备族契约，明确报 `blocked by toolchain limitation` 或等价 blocker
- 在最终答复里写清：缺的是哪类过程设备语义，而不是把问题下沉成原始 I/O 阈值
