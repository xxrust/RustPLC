# 诊断后端方案（有/无板 + 真实工况统一）

日期：2026-02-19

## 1. 目标重述（按现场要求）

目标不是只在离线报告里看到 `timeout`，而是：

1. 在真实工况运行时，告警与原因推测要能直接送到 HMI。
2. 无板评审（no-board）、有板联调（HIL/board）、真实运行（runtime live）三类场景要复用同一套诊断引擎与编码。
3. 输出要可审计、可追溯、可稳定回放。

---

## 2. 当前能力与差距

当前已有：

- `scenario-doctor`：偏静态检查（映射、tick、风险）
- `no-board-gate`：能报 mismatch / realtime fail

当前缺失：

- 失败后“Top-N 候选原因 + 证据 + 修复建议”的统一结构
- 真实运行时直接对 HMI 的告警推送链路
- 有/无板统一的 `evidence_source` 语义

---

## 3. 总体方案（单引擎 + 多数据源 + 多发布通道）

### 3.1 单引擎

- 诊断核心保持**确定性规则引擎**（门禁与现场主判定不依赖 AI）。
- AI（若后续引入）仅做解释增强，不影响 pass/fail。

### 3.2 多数据源适配

统一输入契约，按来源打标：

- `no_board`
- `hil_board`
- `runtime_live`
- `mixed`

这样可以让“无板评审”和“现场运行”使用同一套 issue code 与候选分类。

### 3.3 多发布通道（含 HMI）

运行态生成标准化 `alarm_event`，并至少提供两类输出：

1. 实时通道（给 HMI 直接消费，例如 WebSocket）
2. 审计通道（NDJSON 文件，供追溯与复盘）

要求：

- 通道异常不阻塞主控制循环
- 告警去重/限流，避免 HMI 告警风暴
- 每条告警携带 `evidence_source` 与场景/配方标识

---

## 4. 诊断输出最小字段

`diagnosis_report.json` / `alarm_event` 至少包含：

- `issue_code`
- `category`
- `rank`
- `confidence`
- `evidence`
- `suggested_fix`
- `evidence_source`

运行时事件额外包含：

- `alarm_id`
- `severity`
- `first_seen_ms`
- `top_candidates`
- `evidence_ref`

---

## 5. 实施边界

本阶段做：

- 后端诊断引擎
- `trace-doctor` 契约
- no-board / commissioning 集成
- 运行态 HMI 发布后端链路

本阶段不做：

- HMI 页面 UI 开发
- AI 作为门禁判定器

---

## 6. 完成后会变成什么样

从“只看到 timeout 现象”，升级为：

- HMI 实时看到：告警级别 + Top-N 原因 + 关键证据
- 离线报告看到：同一编码体系的完整诊断 JSON
- 有板/无板/现场三类链路的结论可互相对照
