# 状态证明检查

`state-proof-check` 是 RustPLC 的项目级状态证明门禁。它不替代 safety / liveness / timing / causality 四引擎，而是补上一个更早的工程审查点：生产状态不能靠变量初值或内部 flag 假装成立，设备恢复启动前也不能忽略机内可能残留的工件。

## 命令

```bash
cargo run --release --bin rust_plc -- state-proof-check <source.plc|source.bundle.toml> \
  --config config/state_proof.toml --output human

cargo run --release --bin rust_plc -- state-proof-check <source.plc|source.bundle.toml> \
  --output json
```

如果不显式传 `--config`，命令会从项目根目录自动查找 `config/state_proof.toml`。JSON 输出固定包含 `schema_version`、`command`、`source_plc`、`status`、`issue_count` 和 `issues[]`，其中 issue 带 `code`、`severity`、`line`、`task`、`step`、`symbol`、`message`、`fix`。

## project-check 集成

`project-check` 对这些输入默认自动运行 `state_proof_check`：

- 输入是 `.bundle.toml`。
- PLC 源里声明了 `[topology] variable`。
- 项目存在 workpiece location / holder / carrier，或 task 使用 workpiece effect。

步骤顺序放在 `sequence-lint` 之后、`process-model-check` 之前。失败时，聚合报告中该步骤为 `fail`，并保留独立报告到 `state_proof_check/report.json`。

## 检查范围

- `SPF-001`：`bool` 变量初值为 `true`，并被生产 `wait` 当作成立条件，但没有传感器、操作者输入、上游交接、workpiece token、拓扑闭环动作或机器可读例外。
- `SPF-002`：`*_has_seed`、`*_ready`、`*_done`、`*_available` 等内部 flag 被当作物理状态使用，但赋值链只来自常量或内部 compute。
- `SPF-003`：`ingress_sites` 被当作有限料盒、料仓、缓存的库存证明使用。
- `SPF-020`：项目有 workpiece location / holder / carrier，但初始化层没有残料策略。
- `SPF-021`：自动流程会消费或移动工件，但 startup 没有检测、清理、回收、拒绝启动或要求人工确认。
- `SPF-022`：急停/停止恢复路径直接回到自动流程，但没有证明设备内工件状态回到受控基线。

## 例外配置

例外必须机器可读，不能只写在注释里。每条例外都必须包含 `reason` 和 `proof_basis`；缺失字段会被视为无效配置。

```toml
schema_version = 1

[[no_feedback_steps]]
task = "startup"
step = "home_axis_without_feedback"
reason = "This axis has no discrete home sensor in the current machine variant."
proof_basis = "commissioning procedure verifies hard-stop homing torque limit"

[[trusted_initial_state]]
symbol = "outfeed"
reason = "Outfeed must be emptied before automatic startup."
proof_basis = "operator startup checklist item A-03"
```

`no_feedback_steps` 适用于真实没有闭环反馈、但已经有外部工程证明的 step。`trusted_initial_state` 适用于必须人工或制度保证的初始状态，例如某个缓存、夹爪、料道在自动启动前必须为空。两者都应少用；如果可以通过传感器、operator front-door、workpiece token 或初始化清残流程证明，就不要写例外。
