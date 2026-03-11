# 并发 Runtime 迁移指南（Axis 默认阻塞语义）

## 适用范围

本指南用于从“单执行点 + axis.move 近似非阻塞假设”迁移到“多 task 并发调度 + axis.move 默认阻塞 step”模型的项目。

注意：本文中的“旧语义”仅用于迁移差异对比，不是新项目设计依据。新实现请以 `docs/architecture/signal-direction.md` 为准。

## 关键行为差异

| 主题 | 旧语义（迁移前） | 新语义（迁移后） |
| --- | --- | --- |
| task 执行模型 | 常被实现为单执行点在 task.step 间切换 | 每个 active task 持有独立上下文，按 task 索引顺序逐 tick 调度 |
| 阻塞影响范围 | 局部语义不稳定，常依赖实现细节 | 某 task 命中 blocking step 仅阻塞自身，不阻塞同 tick 其它 task |
| `axis.move_*` | 易被当作“发命令即继续” | 默认 blocking 长时动作，走 `Pending -> Done/Fault/Timeout` 生命周期 |
| step 离开条件 | 分散在指令分支，易出现特例 | 统一 completion rule，`delay/wait/pending action` 未完成前不得离开 step |

## `axis.move_*` 迁移要点

1. `axis.move_relative` / `axis.move_absolute` 在同一 step 内会阻塞后续语句，直到动作完成、故障或超时。
2. 如果旧流程依赖“move 发起后立即执行同 step 后续动作”，请改为：
   - 拆分为多个 step，并通过 `on_complete` 或显式跳转连接。
3. 继续强制保留 AXIS 语义门禁字段：
   - `timeout`
   - `on_reject`
   - `on_motion_fault`
   - `on_safety_fault`

## 迁移告警码

- 稳定告警码：`MIG-AXIS-BLOCK-001`
- 触发条件：检测到同一 step 内混合 `axis.move_*` 与其它语句，存在“旧非阻塞假设”迁移风险。
- 建议处理：将后续语句拆到后继 step，按完成条件显式编排。

## 诊断 Payload 兼容策略

验证报告中的 warning 条目新增可选字段 `code`，保持向后兼容：

- 旧消费者（仅识别 `level/message`）可忽略未知字段继续解析。
- 新消费者可读取 `code` 进行稳定规则分流。

示例：

```json
{
  "level": "warn",
  "message": "迁移提示：axis.move_* 现按默认阻塞语义执行。",
  "code": "MIG-AXIS-BLOCK-001"
}
```

## 并发门禁本地复现（CI 同源）

并发 runtime 与四类 verification 的 CI 门禁统一脚本：

- `scripts/concurrent_runtime_verification_gate.sh`

本地复现命令序列：

```bash
scripts/concurrent_runtime_verification_gate.sh
```

若需要逐条定位失败，可按脚本内顺序单独执行：

```bash
cargo test --test runtime_bridge_us006 axis_move_blocking_baseline_example_blocks_without_explicit_wait_until_done -- --exact --nocapture
cargo test --test runtime_bridge_us006 load_unload_concurrent_example_keeps_load_blocked_while_unload_advances -- --exact --nocapture
cargo test --test examples_integration parses_axis_move_blocking_baseline_example_without_explicit_wait -- --exact --nocapture
cargo test --test examples_integration parses_load_unload_concurrent_tasks_example_into_verified_ir_json -- --exact --nocapture
cargo test --lib verification::safety::tests::reports_conflict_when_independent_tasks_overlap_on_conflicting_outputs -- --exact --nocapture
cargo test --lib verification::liveness::tests::reports_deadlock_when_two_tasks_only_wait_each_other_resource_release -- --exact --nocapture
cargo test --lib verification::timing::tests::concurrent_worst_case_analysis_distinguishes_task_local_and_global_completion -- --exact --nocapture
cargo test --lib verification::causality::tests::accepts_cross_task_variable_chain_with_compute_dataflow -- --exact --nocapture
```
