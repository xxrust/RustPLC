# PRD：非 RTOS 实时性证据与门禁强化（No-Board）

日期：2026-02-17

本阶段目标：在**不引入 RTOS**的前提下，把当前 tick 驱动架构从“结构上确定性”升级为“可量化、可门禁、可交付”的实时性证据链。

---

## 0. Ralph 执行合同（必须遵守）

### 0.1 输入文件

- `prd.json`
  - `branchName`：目标分支
  - `userStories[]`：待实现故事
- `progress.txt`
  - 先读顶部 `## Codebase Patterns`

### 0.2 Story 选择规则

- 每次只做 1 条 story
- 仅从 `passes: false` 里选
- 选 `priority` 最小者
- 不并行做多条，不顺手改无关项

### 0.3 分支规则

- 当前分支应等于 `prd.json.branchName`
- 不一致则切换；不存在则从 `main` 创建

### 0.4 质量门禁

- 至少通过：`cargo test --workspace`
- 改 CLI/报告结构时必须补集成测试（`env!("CARGO_BIN_EXE_rust_plc")`）
- 新命令必须补文档（输入/输出工件说明）

### 0.5 交付记录

- 完成后更新 `prd.json` 对应 story 的 `passes: true`
- 追加 `progress.txt`（实现内容 + learnings）
- 可复用规律上提到 `## Codebase Patterns`

### 0.6 提交规范

- Commit message：`feat: [Story ID] - [Story Title]`
- 不提交 broken code

---

## 1. 背景问题（为什么继续做）

当前系统已具备：
- 固定 tick 语义
- runtime budget 结构上界（动作数/转移链）
- no-board trace 闭环与发布包

但还缺关键能力：
1. 缺少**每 tick 实际执行时间**观测（`exec_us/slack_us/jitter`）
2. 缺少**overrun（超周期）**统一判定与 CI 门禁
3. 缺少“实时性证据”随 release 一起交付

---

## 2. 目标（不依赖 RTOS）

1. 保持单循环 tick 架构，不引入任务调度复杂度
2. 建立统一 tick 时序证据模型：`tick_start/end/exec_us/slack_us/overrun`
3. 提供可门禁阈值：`max_exec_us`、`p99_exec_us`、`overrun_count`
4. 把实时性报告纳入 no-board gate 与 release-bundle
5. 形成可复现 playbook（命令、工件、阈值建议）

---

## 3. 范围（In Scope）

- runtime/firmware/virtual-board 的 tick 时序观测与统一输出
- 新增时序分析 CLI 与 JSON 报告
- no-board-gate 实时门禁
- release-bundle 纳入时序工件
- 文档与测试

## 4. 非目标（Out of Scope）

- 引入 RTOS（FreeRTOS/Embassy 等）
- 多任务抢占调度与优先级反转治理
- 真实硬件电磁噪声/温漂等物理层验证

---

## 5. 设计原则

- **确定性优先**：同输入同 seed 产生同样结论
- **证据优先**：先有结构化观测再谈优化
- **门禁优先**：阈值可配置且可 fail fast
- **向后兼容**：默认行为不破坏现有命令链路

---

## 6. 成功指标

- 可产出 `timing_report.json`（含 p50/p95/p99/max、overrun_count）
- `no-board-gate` 可按阈值阻断实时风险
- `release-bundle` 包含实时证据工件 + manifest 哈希
- 在仓库示例上提供 1 条最小可复现实战流程

---

## 7. 用户故事

以 `prd.json.userStories` 为唯一事实来源，按 `priority` 顺序迭代。
