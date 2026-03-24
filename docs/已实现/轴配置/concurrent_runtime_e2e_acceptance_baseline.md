# 并发 Runtime 端到端闭环验收基线（US-020）

## 1. 文档目的

本文件定义并发 task + 阻塞 step 重构的最终验收清单，作为 US-020 的唯一执行基线。

验收目标是把以下环节串成闭环：

- 语义冻结
- IR/Bridge/Runtime 执行闭环
- 四类 verification 闭环
- 迁移与 CI 闭环

## 2. Gate 分阶段清单

### Gate-A：语义冻结

**必须产物**

- `docs/architecture/signal-direction.md`（Gate-A 术语与调度语义源）
- `AGENTS.md` 与 `.codex/skills/plc-system/SKILL.md`、`.codex/skills/plc-gen/SKILL.md`（术语同义同步）

**执行要求**

- 人工审阅：术语必须覆盖 `active task / task context / blocking step / pending action / completion condition`
- 迁移语义边界必须在 `docs/concurrent_runtime_migration_guide.md` 明确声明旧语义仅用于迁移对比

**通过判据**

- 上述文档存在且语义同源，无“单执行点”作为新实现指导的表述

### Gate-B：IR + Bridge + Runtime 执行闭环

**必须产物**

- IR task 上下文模型（`src/ir/mod.rs`）
- runtime task 上下文与调度实现（`crates/runtime-core/src/lib.rs`）
- bridge task 边界保留实现（`src/runtime_bridge.rs`）

**可执行命令**

```bash
cargo test --test runtime_bridge_us006 bridge_preserves_task_boundaries_for_independent_roots -- --exact --nocapture
cargo test -p runtime-core axis_move_pending_blocks_and_polls_without_replaying_prior_actions -- --exact --nocapture
cargo test -p runtime-core axis_move_pending_then_fault_clears_pending_state_and_surfaces_error -- --exact --nocapture
```

**通过判据**

- runtime 不回退到单 `Location` 语义
- `axis.move_*` 在无显式 `wait` 时仍保持 Pending 阻塞生命周期
- bridge 输出保持 task 边界并可执行

### Gate-C：四类 Verification 闭环

**必须产物**

- `src/verification/safety.rs`
- `src/verification/liveness.rs`
- `src/verification/timing.rs`
- `src/verification/causality.rs`

**可执行命令（CI 同源）**

```bash
scripts/concurrent_runtime_verification_gate.sh
```

该脚本最小覆盖：

- 并发 runtime 行为（阻塞 move + 双 task 并发）
- examples 编译回归
- safety/liveness/timing/causality 各 1 个并发回归

**通过判据**

- 四类引擎在并发语义下均有稳定回归测试且全部通过

### Gate-D：迁移 + 文档 + CI 闭环

**必须产物**

- `docs/concurrent_runtime_migration_guide.md`（迁移差异、告警码、复现命令）
- `scripts/concurrent_runtime_verification_gate.sh`（本地/CI 同源门禁脚本）
- `.github/workflows/rp2040_regression.yml`（包含 `concurrent-runtime-verification-gate` 任务）

**可执行命令**

```bash
rg -n "concurrent-runtime-verification-gate" .github/workflows/rp2040_regression.yml
cargo test --test verification_report report_emits_axis_blocking_migration_warning_with_stable_code -- --exact --nocapture
```

**通过判据**

- CI workflow 显式调用并发 gate 脚本
- 迁移告警码 `MIG-AXIS-BLOCK-001` 可稳定产出并具备兼容 payload

## 3. Ralph 顺序执行约束（PRD vs prd.json）

Ralph 执行顺序以根目录 `prd.json` 为准，必须按 priority 升序推进；该顺序已按依赖关系冻结：

1. `US-001`：语义冻结（Gate-A）
2. `US-002..US-010`：IR/Runtime/Bridge 执行模型闭环（Gate-B）
3. `US-011..US-014`：四类 verification 并发闭环（Gate-C）
4. `US-015..US-019`：迁移、示例、文档、CI 闭环（Gate-D）
5. `US-020`：端到端闭环基线收口（本文件）

`tasks/prd-concurrent-task-runtime-and-blocking-steps.md` 提供背景与分阶段设计说明；执行状态与 `passes` 以根目录 `prd.json` 为准。

## 4. 最终总门禁

完成 Gate-A/B/C/D 后，必须执行：

```bash
cargo test
```

仅当总门禁通过，US-020 才可标记 `passes: true`。
