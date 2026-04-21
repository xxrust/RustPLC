# PLC 优化管线

RustPLC 的优化管线有一个关键约束：优化必须复用现有编译器和验证流水线，不能发明第二套合法性模型。

---

## 设计原则

优化不是绕过编译流水线的旁路。它坐在现有语义闭合之上，把候选方案送回同一组门禁：

```
原始 .plc 源码
    ↓
parse + preprocess → OptimizationContext
    ↓
analyze_optimization_opportunities()    # 识别优化机会
    ↓
generate_candidate_rewrites()           # 生成候选重写
    ↓
evaluate_candidate_timing()             # 复用 timing 引擎评估
    ↓
recheck_candidate_legality()            # 复用完整验证流水线检查
    ↓
rank_candidates()                       # 排序
    ↓
emit_optimized_plc()                    # 只重写 [tasks]，保留 topology/constraints
```

---

## 复用的基础设施

### 时序

候选方案的时序评估复用 `src/verification/timing.rs` 的现有 API：

- `estimate_program_timing()` — 程序级时序估算
- 覆盖：顺序时序、并发时序、wait、delay、timeout、repeat 展开

候选方案的时序和正常编译的时序来自同一个引擎，不存在 "优化说合法但编译说不合法" 的分裂。

### 合法性

每个候选方案经过完整的编译验证流水线：

```
preprocess_program → build_topology_graph → build_constraint_set → build_state_machine → verify_all
```

如果候选方案不通过，它在和正常编译完全相同的规则下失败。

### 发射

候选方案只重写 `[tasks]` 段，保留原始的 `[topology]` 和 `[constraints]` 不变。优化不假装拥有拓扑和约束的格式化权。

---

## 当前优化类别

Phase-1 分析器检测以下保守优化机会：

| 类别 | 说明 |
|------|------|
| 重排相邻独立步骤 | 两个无依赖的步骤交换顺序 |
| 并行化相邻独立步骤 | 两个无依赖的步骤合并为 parallel |
| 合并冗余 wait | 多个等待同一条件的 wait 合并 |
| 合并相邻 delay | 连续的 delay 合并为一个 |
| 替换简单超时恢复路由 | 简化 timeout → goto 模式 |

### 排序规则

1. 合法优先于不合法
2. 全局标称时间更短优先
3. wait 点更少优先
4. 变更代价更小优先
5. 稳定 ID 打破平局

---

## 使用

当前优化是库级 API，尚无 CLI 子命令：

```rust
use rust_plc::optimization::optimize_plc_source;

let source = std::fs::read_to_string("examples/project_scaffold_demo/plc/main.plc")?;
let candidates = optimize_plc_source(&source)?;

for c in candidates.iter().take(3) {
    println!("{} legal={} nominal_ms={} rewrite={}",
        c.id, c.legality.is_legal, c.timing.global_nominal_ms, c.rewrite.summary);
}
```

候选方案的关键字段：
- `rewrite.summary` — 重写摘要
- `timing.global_nominal_ms` / `timing.global_worst_case_ms` — 时序
- `legality.is_legal` / `legality.diagnostics` — 合法性
- `source` — 重写后的完整 .plc 源码

---

## 边界

这些是 Phase-1 的有意边界：

- 无 CLI 子命令
- 无优化专用验证规则（复用现有规则）
- 不跨 topology/constraints 做文本手术
- 不做全局调度搜索
- 不在 runtime/codegen 层做投机性语义变更

如果未来优化需要更丰富的变换，语义形状必须先上升到 IR 或验证模型层，不能在优化层猜测。

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/optimization/mod.rs` | 优化入口 |
| `src/optimization/analyzer.rs` | 机会分析 |
| `src/optimization/rewrite.rs` | 候选重写 |
| `src/optimization/timing.rs` | 时序评估 |
| `src/optimization/ranker.rs` | 排序 |
| `src/optimization/emitter.rs` | 源码发射 |

```bash
# 优化相关测试
cargo test optimization::

# 时序复用回归
cargo test verification::timing::tests::concurrent_worst_case_analysis
```
