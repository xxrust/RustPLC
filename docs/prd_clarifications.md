# PRD 澄清说明：问题 2 选项含义

## 2. 因果推断中对外部输入的处理方式

**背景**：因果验证会尝试推断“动作 -> 传感器”的链路。如果把所有 `analog_input` 都当作传感器参与推断，来自环境的输入（比如压力/温度的 ADC 通道）会被错误要求具备“从 PLC 输出驱动到该输入”的链路，从而产生误报。以下选项是不同的处理策略。

### A. 增加 `external: true`（digital/analog input），跳过因果推断
- **做法**：在 `digital_input` / `analog_input` 设备上显式标注 `external: true`，因果推断遇到该输入时跳过“动作->传感器链”推断。
- **优点**：语义明确、可控，避免误报；工程上最接近“环境输入”真实含义。
- **代价**：需要 DSL 扩展（新增属性）以及相应语义/文档支持。

### B. 仅当 `analog_input` 有 `detects`/连接链时才参与推断
- **做法**：不新增 DSL。把 `analog_input` 视为传感器的前提是它**明确声明检测对象**（如 `detects: valve.on`）或能通过拓扑连接链推导出“内部可达”的来源；否则视为外部输入，不参与因果推断。
- **优点**：无 DSL 变更；兼容现有程序。
- **代价**：启发式规则，可能出现边界误判（某些内部信号未显式 `detects` 时会被误判为外部）。

### C. 保持现状，仅在文档中说明限制
- **做法**：不改代码，只在文档中提醒“同 step 同时出现 action + wait(analog_input) 会触发因果链推断，需拆步规避”。
- **优点**：零改动、最省开发成本。
- **代价**：误报仍然存在，用户体验受影响；工程风险由用户承担。

### D. 其他
- **做法**：用户自定义策略，例如：允许在某些场景通过 `causality` 规则显式“豁免”外部输入，或在验证时对外部输入采用不同策略。

---

## 为什么“传感器反馈”与“操作员设定值”的因果推断逻辑不同

**核心原则**：因果推断的前提是“被等待的输入信号可由系统动作导致”。这对反馈型传感器成立，但对操作员设定值不成立。

### 1) 传感器信号（反馈）
- **语义**：来自被控对象的状态或过程量（压力、位置、开关量等）。
- **因果方向**：`PLC 输出动作 -> 过程变化 -> 传感器变化`。
- **推断结论**：若代码 `wait` 某传感器，合理期待存在“动作->传感器”链路（拓扑/检测链可达）。

### 2) 操作员设定值（命令/外部输入）
- **语义**：人或上位系统主动给出的目标值，与 PLC 当前动作无因果依赖。
- **因果方向**：`外部输入 -> PLC 逻辑响应`。
- **推断结论**：不应要求“动作->设定值”的因果链，否则会产生误报。

### 结论（建议）
- **反馈型传感器**：允许参与因果推断。
- **设定值/外部输入**：应标注为外部输入，跳过因果推断，或只在显式声明来源时参与。

---

## 数值比较最佳实践（analog_input）

**建议方向**：当需要数值比较时，直接在 `analog_input` 上写阈值比较；`sensor` 仅用于离散反馈信号。

**最佳实践**：
- **只在 `analog_input` 上做数值比较**：`safety` / `wait` 的阈值比较以 `analog_input` 为目标。
- **必须声明 `range`**：缺少 `range` 会导致阈值比较缺乏语义边界，应视为错误。
- **避免 `==`**：用区间或容差带代替，例如 `pressure >= 58 AND pressure <= 62`。
- **使用“阈值集合”离散化**：验证引擎仅按出现的阈值分区，避免状态空间爆炸。
- **外部设定值标注 `external: true`**：避免被错误要求具备“动作→输入”的因果链。
- **传感器（sensor）用于离散信号**：如限位开关、已量化的反馈信号；不承担数值语义。

---

## 验证报告契约（US-001/US-002 对应）

从 2026-02-16 起，`rust_plc <file.plc>` 在验证通过后会额外产出结构化报告：

- 默认路径：`out/<plc文件名>.verification_report.json`
- 可指定路径：`--report <file>`

### 报告字段（最小契约）

- 顶层：
  - `schema_version`
  - `tool_version`
  - `source_plc`
  - `generated_at`
  - `verification`
- `verification.<checker>`（`safety/liveness/timing/causality`）：
  - `level`
  - `warnings`（数组，元素结构 `{ level, message }`）
  - `checked_rules`
  - `skipped_rules`

### `--deny-warnings` 门禁

当启用 `--deny-warnings` 时：

- 若存在 `warn` 或 `error` 级 warning，命令返回非 0（阻断）
- 默认（不加该参数）仍保持“仅提示不阻断”

这使 CI 可以把“有界验证/风险提示”纳入发布门禁，而不是仅凭字符串日志人工判断。

### Runtime Budget（US-003）

结构化报告与 `build_meta.json` 现在都包含 `runtime_budget`，用于无实板阶段的确定性预算评估。当前字段包括：

- `max_transitions_per_tick_cap`（runtime-core 固定上限，当前 64）
- `max_transitions_same_tick_upper_bound`
- `max_actions_per_transition`
- `max_actions_per_tick_upper_bound`
- `max_parallel_branches`
- `max_race_branches`
- `has_same_tick_cycle`

可通过 CLI 或环境变量配置预算阈值并触发 warn：

- CLI:
  - `--budget-max-actions-per-transition`
  - `--budget-max-actions-per-tick`
  - `--budget-max-parallel-branches`
  - `--budget-max-race-branches`
  - `--budget-warn-on-same-tick-cycle`
- ENV:
  - `RUST_PLC_BUDGET_MAX_ACTIONS_PER_TRANSITION`
  - `RUST_PLC_BUDGET_MAX_ACTIONS_PER_TICK`
  - `RUST_PLC_BUDGET_MAX_PARALLEL_BRANCHES`
  - `RUST_PLC_BUDGET_MAX_RACE_BRANCHES`
  - `RUST_PLC_BUDGET_WARN_ON_SAME_TICK_CYCLE`
