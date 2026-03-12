# 诊断后端方法学（面向非专业读者）

日期：2026-02-19

## 1. 这份文档解决什么问题

当现场只看到“超时”时，通常不知道真正原因。  
本方案把“现象”变成“候选原因列表”，并且保证：

- 无板评审（no-board）
- 有板联调（HIL/board）
- 真实运行（runtime live）

三种场景都用同一套诊断编码和输出格式。

---

## 2. 核心名词（白话版）

- `anchor`（锚点）：系统先找到“出问题的关键时刻”，比如首次超时、首次轨迹不一致。
- `candidate`（候选原因）：系统给出的“可能原因”，按概率（置信度）排序。
- `evidence`（证据）：支持该候选原因的事实描述。
- `evidence_source`（证据来源场景）：证据来自 no-board / hil-board / runtime-live 哪种工况。
- `evidence_inputs`（本次真正用到的证据输入）：例如 trace、diff、timing_report、io_snapshot。

---

## 3. 规则图（Rule Graph）

```text
输入工件
  ├─ PLC源码
  ├─ scenario
  ├─ trace(可选)
  ├─ diff_report(可选)
  ├─ timing_report(可选)
  └─ io_snapshot(可选)
        │
        v
锚点提取
  ├─ timeout 锚点
  └─ first_mismatch 锚点
        │
        v
规则评分器（确定性）
  ├─ expected_input_never_changed
  ├─ actuator_command_missing
  ├─ interlock_or_requires_blocked
  ├─ mapping_or_alias_mismatch
  └─ timeout_budget_too_short
        │
        v
按分数排序 + 稳定输出 JSON
```

---

## 4. 评分策略（Scoring Strategy）

每个候选原因先有基础分，再根据证据加分，最后截断到固定范围并排序：

- 基础分：每类原因都有固定起点。
- 证据加分：例如出现 timeout 锚点、发现 wait 输入从未变化、发现 realtime overrun。
- 稳定排序：先按分数降序，再按类别固定顺序，保证同输入必得同输出。

这意味着：同一批工件反复运行，结果顺序不会漂移，便于 CI 比较和审计追溯。

---

## 5. HMI 告警字段定义（alarm_event）

运行态向 HMI 推送的核心字段：

- `alarm_id`：告警唯一标识（用于去重/限流）
- `severity`：严重级别（例如 critical）
- `first_seen_ms`：首次发现时间（毫秒）
- `top_candidates`：Top-N 候选原因（带 issue_code、confidence、evidence）
- `evidence_ref`：证据文件引用（如 trace 路径）
- `evidence_source`：证据场景来源（runtime_live 等）
- `scenario_or_recipe_id`：场景/配方标识（用于回溯）

---

## 6. AI 使用边界（必须明确）

门禁与主判定使用**确定性规则**。  
AI（如果后续接入）只做“解释增强”（例如把技术结果翻译成人话），**不参与是否放行的硬判定**，也不阻断控制流程。

---

## 7. 两个典型示例

### 示例 A：传感器无响应导致 timeout

- 现象：等待 `X0 == true`，一直等不到，超时。
- 常见 Top 候选：`AXF-IN-001`（expected_input_never_changed，兼容字段保留 `DIAG-IN-001`）。
- 常见证据：超时锚点 + wait 通道无变化 + io_snapshot 显示该通道在超时前始终未变化。

### 示例 B：现场 HMI 实时看到原因推测

- 现象：运行中 timeout。
- 系统动作：生成 `alarm_event`，实时推送 WebSocket，同时写 NDJSON 审计。
- 如果实时通道故障：主循环不中断，审计仍保留，现场可继续运行并事后追溯。

---

## 8. 输出格式变更说明（旧格式 vs 新格式）

### 8.1 `no-board-gate --output json`

- 旧格式（schema_version=1）：仅包含 trace_match / realtime_failures / trace & timing 路径。
- 新格式（schema_version=2）：失败时新增：
  - `diagnosis_report`（诊断工件路径）
  - `diagnosis_top_candidate_code`
  - `diagnosis_evidence_source`

### 8.2 `trace-doctor --output json`

- 新增 `evidence_inputs`：明确本次诊断实际用了哪些输入（如 `trace`、`diff`、`timing_report`、`io_snapshot`）。
- `artifacts` 新增 `io_snapshot` 路径字段（如果提供该工件）。

### 8.3 `sim-plc`

- 新增可选参数：`--io-snapshot-out <io_snapshot.json>`
- 新增工件：`io_snapshot.json`（`schema_version=1`，包含逐 tick 的 DI/AI/DO/AO 状态快照）。

---

## 9. 为什么这套方案适合“有板/无板通用”

- 诊断编码统一（主码 `AXF-*`，兼容码 `DIAG-*`）
- 证据来源可标记（`evidence_source`）
- 证据输入可显式声明（`evidence_inputs`）

这样同一故障能在离线评审、联调、现场运行三条链路里互相对照，不再各说各话。
