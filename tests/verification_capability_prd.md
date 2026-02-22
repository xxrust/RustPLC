# 形式化验证能力测试 PRD

**文件**: `tests/verification_capability.rs`
**状态**: 全部通过 (6/6)

---

## 目标

用 5 个从简单到困难的端到端测试用例，自动检测 RustPLC 形式化验证引擎（Safety / Liveness / Timing / Causality）的真实能力。每个用例构造完整的 `.plc` 程序，走完 `parse → semantic → verify` 全流水线，断言验证引擎能正确通过或拒绝，并检查诊断信息的关键内容。

---

## 测试用例总览

| 编号 | 难度 | 名称 | 覆盖的验证器 | 预期结果 |
|------|------|------|-------------|---------|
| 1 | 简单 | 单气缸往返 — 全部通过基线 | Safety + Liveness + Timing + Causality | 全部通过 |
| 2 | 中等 | 双气缸顺序 vs 并行 — Safety 正反对比 | Safety | 2a 通过 / 2b 失败 |
| 3 | 中等 | Liveness 多场景 — 三重违规同时检出 | Liveness | 失败（≥3 个错误） |
| 4 | 较难 | Timing + Causality 联合检测 | Timing + Causality | 失败（3 个错误） |
| 5 | 困难 | 双工位加工站 — 四项验证全部失败 | Safety + Liveness + Timing + Causality | 失败（≥4 个错误） |

---

## 测试 1（简单）：单气缸往返 — 全部验证通过的基线

**场景**：一个气缸伸出再缩回，有完整的拓扑、因果链、timeout、on_complete 循环。

**拓扑**：Y0 → valve_A → cyl_A → sensor_A_ext / sensor_A_ret，外加 start_button。

**约束**：
- `timing: task.work.step_extend must_complete_within 500ms`（stroke_time 200ms + response_time 20ms = 220ms < 500ms）
- `causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext`

**预期**：
- Safety：完备证明（无 conflicts_with 约束）
- Liveness：通过（所有 wait 有 timeout，ready 有 allow_indefinite_wait）
- Timing：通过（220ms < 500ms）
- Causality：通过（链路完整连通）

**验证能力**：确认引擎在"一切正确"时不会误报。

---

## 测试 2（中等）：双气缸顺序安全 vs 并行冲突 — Safety 正反对比

### 2a：顺序动作 — Safety 通过

**场景**：两个气缸顺序动作（先 extend A → retract A → extend B → retract B），声明 `cyl_A.extended conflicts_with cyl_B.extended`。

**预期**：Safety 通过（顺序执行中 A 缩回后 B 才伸出，不会同时 extended）。

### 2b：并行动作 — Safety 失败

**场景**：同样的拓扑和约束，但改为 `parallel` 块同时伸出两个气缸。

**预期**：Safety 失败，错误包含 `conflicts_with` 和源码位置。

**验证能力**：Safety 引擎能区分"顺序安全"和"并行冲突"，并给出反例路径。

---

## 测试 3（中等）：Liveness 多场景 — 三重违规同时检出

**场景**：一个程序同时触发多种 Liveness 违规：
1. `entry` task 的 `wait: sensor_X == true` 无 timeout 且无 allow_indefinite_wait
2. `entry` task 声明 `on_complete: unreachable`，但末步存在非跳转路径（unreachable 标记无效）
3. `spin_a` ↔ `spin_b` 互相 goto 形成 SCC，但无 timeout/allow 出边

**预期**：
- 检出 "缺少 timeout 分支"
- 检出 "on_complete: unreachable" 标记无效
- 检出 "强连通分量" 无超时出口
- 至少报告 3 个以上的 liveness 问题

**验证能力**：Liveness 引擎能同时检出多种死锁风险，不会只报第一个就停。

---

## 测试 4（较难）：Timing + Causality 联合检测

**场景**：
- 气缸 stroke_time=200ms，上游 valve response_time=20ms，但 `must_complete_within 100ms` → Timing 违反
- 因果链声明 `Y0 → valve_A → cyl_A → sensor_A_ext`，但 cyl_A 缺少 `driven_by: valve_A` → Causality 断裂
- `must_start_after 200ms` 但前驱 timeout 只有 50ms → Timing must_start_after 违反

**预期**：
- Timing 错误：包含 "无法在 100ms 内完成"
- Causality 错误：指出 valve_A → cyl_A 断裂
- Timing 错误：包含 "无法保证" + "200ms"

**验证能力**：Timing 和 Causality 引擎能同时工作，各自独立报告问题。Timing 能正确计算上游 response_time 链路时间，也能检测 must_start_after 违反。

---

## 测试 5（困难）：双工位加工站 — 四项验证全部失败

**场景**：模拟真实的双工位加工站，故意引入四类错误：

1. **Safety**：`parallel` 块同时伸出 clamp_A 和 clamp_B，违反 `clamp_A.extended conflicts_with clamp_B.extended`
2. **Liveness**：`error_recovery` task 的 `wait: sensor_A_released == true` 无 timeout 且无 allow_indefinite_wait
3. **Timing**：`task.main.clamp_both must_complete_within 50ms`，但 stroke_time=300ms + response_time=25ms = 325ms
4. **Causality**：声明 `Y2 → valve_C → clamp_B → sensor_B_clamped`，但 clamp_B 缺少 `driven_by: valve_C`

**拓扑**：
- 工位 A：Y0 → valve_A → clamp_A → sensor_A_clamped / sensor_A_released
- 工位 B：Y2 → valve_C → clamp_B（断裂）→ sensor_B_clamped / sensor_B_released

**预期**：
- 4 个不同 checker 各至少报告 1 个错误
- 所有错误包含 `位置:` 和 `建议:`
- unique_checkers 集合大小 = 4

**验证能力**：在接近真实复杂度的程序上，四项验证引擎全部独立工作，同时报告所有问题，错误信息包含行号、原因和修复建议。

---

## 验收标准

- [x] 6 个测试函数全部编译通过
- [x] `cargo test --test verification_capability` 全部 PASS
- [x] 每个测试覆盖至少一个验证器的真实能力
- [x] 测试 5 同时触发全部四个验证器的错误
- [x] 所有失败场景验证错误信息包含位置和建议
