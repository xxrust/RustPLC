# PLC Optimization Architecture Spec

## 1. 这份文档只讲重点

这份文档只回答两个问题：

1. 为了做 `*.plc` 优化，还需要新增哪些算法模块
2. 这些算法模块的实现顺序应该怎么排

除了这两件事，别的不展开。

---

## 2. 必须新增的算法模块

### 2.1 `analyze_optimization_opportunities()`

作用：

- 从现有 `*.plc` 里找出可优化点

最少要找出：

- 哪些 step 可以调序
- 哪些动作可能并行
- 哪些等待可以合并或前移
- 哪些恢复路线有替代方案

### 2.2 `generate_candidate_rewrites()`

作用：

- 根据固定规则生成候选优化方案

首版建议支持的 rewrite：

- 串行改并行
- 无依赖 step 调序
- 合并等待点
- 提前准备动作
- 恢复路线替换

### 2.3 `evaluate_candidate_timing()`

作用：

- 计算每个候选方案的时间

首版至少支持：

- 串行时间
- 并行时间
- `wait / delay / timeout`
- `repeat` 展开后的时间

### 2.4 `recheck_candidate_legality()`

作用：

- 对每个候选方案重新做合法性检查

这里不重新发明规则，只复用现有验证链。

### 2.5 `rank_candidates()`

作用：

- 在所有合法候选方案里选出更优方案

首版排序规则建议：

1. 先淘汰不合法方案
2. 再按总时间更短排序
3. 如相同，再按等待更少排序
4. 如还相同，再按改动更小排序

### 2.6 `emit_optimized_plc()`

作用：

- 稳定输出优化后的 `*.plc`

这里必须保证：

- 输出稳定
- 改动可对比
- 不改坏无关部分

## 3. 第一版建议的代码结构

建议直接新增：

- `src/optimization/mod.rs`
- `src/optimization/analyzer.rs`
- `src/optimization/rewrite.rs`
- `src/optimization/timing.rs`
- `src/optimization/ranker.rs`
- `src/optimization/emitter.rs`

如果需要用户说明文本，再加：

- `src/optimization/report.rs`

---

## 4. 第一版最合理的推进顺序

### Phase 1

- 先做 `analyze_optimization_opportunities()`

### Phase 2

- 再做 `generate_candidate_rewrites()`

### Phase 3

- 再做 `evaluate_candidate_timing()`

### Phase 4

- 接上 `recheck_candidate_legality()`

### Phase 5

- 做 `rank_candidates()` 和 `emit_optimized_plc()`


## 5. 最后结论

这件事的边界很简单：

- 算法模块负责生成、计算、验证、选型、输出
