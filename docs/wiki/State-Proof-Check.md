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
- `SPF-030`：startup/init task 在发出轴回零或运动命令之前等待 home 传感器。
- `SPF-031`：`allow_indefinite_wait` 被用于本 task 可控的本地传感器或反馈；无限等待只适用于操作者、上游 task、下游 task 等不受控他者。
- `SPF-032`：残片/清空/基线检查失败后跳到泛化故障，而没有显式区分自动恢复和人工协助边界。
- `SPF-033`：真空保持类 holder 在把工件转移到非 holder 位置时提前释放吸附，且没有接收方所有权证明。
- `SPF-040`：被 task 驱动的执行类设备没有 maintenance/self-check 路径，也没有机器可读豁免。

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

[[self_check_exempt_devices]]
device = "run_lamp"
reason = "This output lamp has no modeled feedback contact."
proof_basis = "commissioning procedure verifies the lamp during panel test"
```

`no_feedback_steps` 适用于真实没有闭环反馈、但已经有外部工程证明的 step。`trusted_initial_state` 适用于必须人工或制度保证的初始状态，例如某个缓存、夹爪、料道在自动启动前必须为空。`self_check_exempt_devices` 只适用于确实不能由 PLC 自检的执行类设备。三者都应少用；如果可以通过传感器、operator front-door、workpiece token、初始化清残流程或 maintenance task 证明，就不要写例外。
