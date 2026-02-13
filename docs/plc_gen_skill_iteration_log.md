# plc-gen Skill Iteration Log (10 Rounds)

目标：把 `.codex/skills/plc-gen/SKILL.md` 从“流程描述”变成可持续维护的“可验证规则集”，并用可运行的 fixture 覆盖关键语义点。

说明：
- 这里的 “User 子代理” 用于模拟常见工艺回答（用于覆盖测试面）；真实落地时仍应以工程师实际工艺为准。
- 每一轮输出都对应一个可通过 `cargo test -q` 的 `.plc` fixture：`.codex/skills/plc-gen/fixtures/valid/*.plc`。

---

## Round 01 — 三缸顺序工艺（基线闭环）

Codex：按阶段一复述动作序列，并追问启动方式、循环模式、初始状态、互锁。

User：启动按钮；单次循环；默认全缩回；冲压时必须保持压紧。

Maintainer 发现的问题：
- skill 没有明确“最小提问清单”，容易在缺信息时直接生成。

修复：
- 在 `SKILL.md` 阶段一加入“最小提问清单”。
- 新增 fixture：`01_push_clamp_press.plc`。

---

## Round 02 — 传送带分度 + 挡停（电机/延时/互锁）

Codex：确认电机需要 ramp、到位传感器检测点位；挡停缸伸出时是否允许电机继续转。

User：挡停伸出时必须停带；到位后需要机械沉降 200ms。

Maintainer 发现的问题：
- skill 对“电机类设备”的建模与安全互锁没有明确模板。

修复：
- 在 fixtures 覆盖 motor + delay + safety。
- 新增 fixture：`02_conveyor_index_and_stop.plc`。

---

## Round 03 — race 分支（互斥检测的工艺分流）

Codex：用 `race` 建模“先检测到哪一个就走哪条工艺”，并要求 `timeout` 兜底到故障处理。

User：1200ms 内必须检测到 A/B，否则报警。

Maintainer 发现的问题：
- skill 虽有 race 示例，但缺少“标准 fault_handler 骨架”来保证 liveness。

修复：
- 在 `SKILL.md` 新增“可验证任务骨架（模板）”。
- 新增 fixture：`03_race_sorting_gate.plc`。

---

## Round 04 — repeat 展开（重复小循环）

Codex：确认重复次数、每次动作的 timeout 上界、是否允许嵌套 repeat（不允许）。

User：重复 4 次，每次 1200ms 超时。

Maintainer 发现的问题：
- skill 提到 repeat，但缺少“用 fixture 防回归”的约束。

修复：
- 新增 fixture：`04_repeat_glue_cycles.plc`。

---

## Round 05 — 自定义状态（多位阀/多状态设备）

Codex：确认该设备状态来自外部（等待状态），并标注 `allow_indefinite_wait`。

User：这是状态监视点，不是控制动作。

Maintainer 发现的问题：
- skill 的自定义 states 示例需要被真实 DSL 支持的 fixture 覆盖，否则容易“文档写了但语法不通”。

修复：
- 新增 fixture：`05_custom_states_3pos_valve.plc`。

---

## Round 06 — AND 等待（多传感器同步到位）

Codex：强调 AND/OR 不混用；AND 等待必须有 timeout；并检查因果性闭环。

User：需要同时到位才继续。

Maintainer 发现的问题：
- skill 把“每个 wait 传感器都必须声明 causality 链”写死了，但编译器实际上可从拓扑推断。

修复：
- 在 `SKILL.md` 中把 causality 改为“最佳实践/复杂场景建议显式声明”，并解释推断行为。
- 新增 fixture：`06_and_wait_two_sensors.plc`（同时包含未显式声明的 wait 传感器，以验证推断仍通过）。

---

## Round 07 — if/else 选择（模式开关）

Codex：把“模式选择”建模成 `if: ... goto ... else: goto ...`，并确保每个 task 有 on_complete。

User：只需要简单模式分流。

Maintainer 发现的问题：
- skill 的 if/else 片段应在 fixtures 中覆盖，防止未来语法变更导致示例失效。

修复：
- 新增 fixture：`07_if_else_mode_select.plc`。

---

## Round 08 — Timing SLA（真实节拍 vs 最坏上界）

Codex：解释 `must_complete_within` 与 `must_complete_within_worst_case` 的差异，并用 delay/timeout 形成对比。

User：真实节拍 1200ms，最坏情况（含超时）3000ms。

Maintainer 发现的问题：
- skill 对 timing 的解释已有，但缺少“最小可运行例”作为约束样例。

修复：
- 新增 fixture：`08_timing_sla_vs_worst_case.plc`。

---

## Round 09 — requires vs conflicts（互锁语义澄清）

Codex：对“保持关系”用 `requires`，对“绝不能共存”用 `conflicts_with`；并要求工程师明确“干涉”是互斥还是顺序约束。

User：工作缸动作必须保持夹紧，且夹紧不能回原位。

Maintainer 发现的问题：
- 文档里已有表格，但缺少可验证例子来承载“同一含义的两种写法”。

修复：
- 新增 fixture：`09_requires_vs_conflicts.plc`。

---

## Round 10 — 人工确认点（允许无限等待的边界）

Codex：明确只有人工操作等待才允许 `allow_indefinite_wait: true`；其余 wait 必须 timeout。

User：升降到位后等待人工确认再回原位。

Maintainer 发现的问题：
- skill 没有明确“允许无限等待”的适用边界与典型用法骨架。

修复：
- 在 `SKILL.md` 的模板中强化 ready/confirm 的使用方式（通过 fixture 体现）。
- 新增 fixture：`10_manual_confirm_step.plc`。

