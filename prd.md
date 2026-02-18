# PRD：步进轴（Pulse/Dir）+ AB 编码器防碰撞与拓扑抽象（含 Wiki 补充）

日期：2026-02-18

---

## 0. 概述

本 PRD 基于 `docs/stepper_ab_encoder.md`，目标是把“步进轴 + AB 编码器”从经验实现，升级为**可验证、可复用、可交付**的工程规范与样例体系。

核心问题：

1. 如何确保步进轴带动机构时不与其他执行机构发生碰撞。
2. 如何处理 `pls -> 角度 -> 距离` 等多坐标描述，并在 DSL 中做稳定、可维护的拓扑抽象。

---

## 1. 方案分析与决策（最佳方案）

### 1.1 备选方案

- 方案 A：在 DSL 中直接表达复杂角度区间逻辑（多条阈值 safety 组合）。
- 方案 B：驱动层先生成 `zone_code` / `collision_window`，DSL 只做互锁。
- 方案 C：A+B 混合（核心互锁信号化，辅助阈值保留）。

### 1.2 决策

采用 **方案 C（A+B 混合）**，但以 **B 为主**：

- 主路径：驱动层信号化（`zone_code`、`pos_consistent`、`range_valid`），DSL 负责状态机与安全约束。
- 辅路径：保留少量阈值规则用于快速校核与调试观测。

### 1.3 决策理由

- 兼容当前 RustPLC 能力边界（safety 二元关系、模拟量阈值离散化）。
- 可显著降低规则爆炸与误用风险。
- 易于扩展到多执行器碰撞矩阵与多传感器一致性判定。

---

## 2. 目标（Goals）

- 建立步进轴防碰撞的统一建模规范（`zone_code + 双向互锁`）。
- 建立多坐标抽象规范（主坐标 + 派生量 + 一致性信号）。
- 提供可运行示例、场景与自动化测试，形成可回归资产。
- 将规范与实践同步到 Wiki，形成团队长期知识库。
- 保持现有验证引擎可收敛，不引入不可判定特性。

---

## 3. 用户故事（8+）

### US-001：定义防碰撞建模基线
**Description:** 作为控制工程师，我希望有标准化的步进轴防碰撞建模规则，以便不同项目使用同一套安全抽象。

**Acceptance Criteria:**
- [x] 在 `docs/stepper_ab_encoder.md` 中明确 `zone_code`、危险窗口、双向互锁定义。
- [x] 明确“命令互锁 + 窗口互锁”的最小组合规则。
- [x] 给出至少 2 个正反例（推荐写法/反模式）。
- [x] 文档中的 DSL 片段可通过语法检查（示例级）。
- [x] `cargo test --workspace` 通过。

### US-002：提供单轴防碰撞示例 PLC
**Description:** 作为开发者，我希望有最小可运行示例，展示 `zone_code` 与执行器互斥，以便快速复用。

**Acceptance Criteria:**
- [ ] 新增 `examples/stepper_collision_guard.plc`。
- [ ] 示例包含 `zone_code`（analog_input external）与至少 1 条互斥 safety。
- [ ] 示例包含 fault 路径（timeout -> goto fault）。
- [ ] `cargo run --release -- examples/stepper_collision_guard.plc --no-print-ir` 成功。

### US-003：提供双向互锁示例 PLC
**Description:** 作为安全工程师，我希望示例同时约束“禁区禁止动作”和“危险姿态禁止继续运动”，避免单向互锁漏洞。

**Acceptance Criteria:**
- [ ] 在示例中包含两条规则：
- [ ] 规则 A：`zone_code` 与执行器状态冲突。
- [ ] 规则 B：`move_cmd.on` 与执行器危险状态冲突（或 requires 安全状态）。
- [ ] 验证报告能识别这两类约束。

### US-004：定义多坐标抽象规范（pls/角度/距离）
**Description:** 作为架构师，我希望明确多坐标如何分层，避免 DSL 里出现难维护的复杂计算。

**Acceptance Criteria:**
- [ ] 文档明确“主坐标 + 派生坐标”原则。
- [ ] 文档明确换算下沉到驱动层（含线性/非线性场景建议）。
- [ ] 给出标准信号清单：`axis_count`、`axis_theta`、`axis_pos_mm`、`axis_speed`、`pos_consistent`、`range_valid`。
- [ ] 给出“何时进 fault/何时降级”的建议策略。

### US-005：提供多传感器一致性示例
**Description:** 作为调试工程师，我希望示例体现“编码器位移 vs 激光位移”一致性判定，以便发现机构松动和传感异常。

**Acceptance Criteria:**
- [ ] 新增 `examples/stepper_multi_sensor_consistency.plc`（或同等示例）。
- [ ] 示例包含 `pos_consistent` / `sensor_fault_code` 的使用。
- [ ] 当一致性失败时，流程进入 fault 或降级分支。
- [ ] 示例可通过编译验证流程。

### US-006：补充场景与回归资产
**Description:** 作为测试工程师，我希望有标准场景集覆盖正常与故障路径，保证后续改动不破坏安全逻辑。

**Acceptance Criteria:**
- [ ] 新增场景：normal / count_stuck / wrong_direction / alarm_triggered。
- [ ] 场景覆盖至少 1 条成功路径和 3 条失败路径。
- [ ] 在 `tests/` 增加对应回归测试或命令级集成测试。
- [ ] `cargo test --workspace` 通过。

### US-007：增强规则可读性与落地指引
**Description:** 作为一线自动化工程师，我希望快速知道“应该写什么 safety，避免写什么”，减少建模歧义。

**Acceptance Criteria:**
- [ ] 文档新增“规则模板”章节（单阈值、区间编码、双向互锁、碰撞矩阵）。
- [ ] 文档新增“常见误区 -> 对应修正方式”。
- [ ] 每个模板都包含可复制 DSL 片段。
- [ ] 与 `docs/scenario_playbook.md` 建立交叉引用。

### US-008：输出 Wiki 页面（必须）
**Description:** 作为团队成员，我希望该方案在 Wiki 可检索、可培训、可长期维护。

**Acceptance Criteria:**
- [ ] 新增/更新 Wiki 页面 2 个：
- [ ] `Stepper-AB-Encoder-Safety-Modeling`
- [ ] `Topology-Abstraction-PLS-Angle-Distance`
- [ ] Wiki 内容与 `docs/stepper_ab_encoder.md` 保持一致。
- [ ] README 或相关文档包含 Wiki 链接入口。

### US-009：定义实现边界与非目标
**Description:** 作为项目负责人，我希望明确当前不做什么，避免 scope 膨胀。

**Acceptance Criteria:**
- [ ] 文档明确不在本期实现：实时脉冲轨迹规划、复杂运动学在线求解、原始 A/B 电平在 DSL 直接解码。
- [ ] 文档明确本期边界：DSL 做顺控与互锁，驱动层做高速计算与信号化。
- [ ] 评审记录中无“隐含新增能力”歧义项。

---

## 4. 功能需求（Functional Requirements）

- FR-1：系统必须支持用 `zone_code` 表达危险窗口（编码由驱动层提供）。
- FR-2：系统必须支持“危险窗口 vs 执行器状态”的 safety 互斥规则。
- FR-3：系统必须支持“运动命令 vs 执行器危险状态”的反向互锁规则。
- FR-4：示例必须体现 timeout/fault 恢复路径。
- FR-5：系统必须提供单轴最小防碰撞示例。
- FR-6：系统必须提供多传感器一致性示例。
- FR-7：系统必须提供至少 4 组标准场景用于回归。
- FR-8：回归测试必须纳入 workspace 测试门禁。
- FR-9：文档必须明确主坐标与派生坐标职责划分。
- FR-10：文档必须明确换算在驱动层完成，不在 DSL 做复杂算术。
- FR-11：文档必须提供可复制的规则模板。
- FR-12：文档必须提供常见误区及修正策略。
- FR-13：Wiki 必须补充并与仓库文档互链。
- FR-14：交付内容必须可被 Ralph 流程消费（结构化用户故事）。

---

## 5. 非目标（Out of Scope）

- 在 DSL 中直接输出高频 STEP 脉冲序列。
- 在 DSL 中直接处理原始 AB 相边沿解码。
- 在本期引入完整轨迹规划器（S 曲线、jerk 限制等）。
- 在本期引入复杂连续控制验证框架（hybrid/timed automata 全量建模）。

---

## 6. 技术考虑（Technical Considerations）

- 保持当前 RustPLC 验证可判定边界：优先有限状态 + 阈值离散抽象。
- `zone_code` 建议使用 `analog_input external`，编码范围固定（例如 0..N）。
- 多传感器比较（如 `abs(pos_mm - laser_mm) <= tol`）应在驱动层完成，并输出布尔/枚举信号。
- 示例规则尽量采用“单职责”写法，避免一条规则承载多语义。
- 若后续引入 lint，可考虑新增“运动命令缺失双向互锁”告警规则（本 PRD 可选项）。

---

## 7. 验收与成功指标（Success Metrics）

- PRD 审核通过后，Ralph 可拆分执行 8+ 条故事，无歧义阻塞。
- 新增示例至少 2 个，且均可通过编译验证。
- 新增场景至少 4 个，回归测试稳定通过。
- Wiki 完成 2 个页面补充并建立入口链接。
- 团队评审中“防碰撞建模方式不一致”问题显著减少（定性指标）。

---

## 8. 实施顺序建议（供 Ralph 选择）

建议优先级：

1. US-001（基线规范）
2. US-002（单轴示例）
3. US-003（双向互锁）
4. US-006（场景与回归）
5. US-004（多坐标规范）
6. US-005（一致性示例）
7. US-007（模板与误区）
8. US-008（Wiki）
9. US-009（边界固化）

---

## 9. 开放问题（Open Questions）

- OQ-1：`zone_code` 采用单通道编码（0..N）还是多布尔窗口输入（one-hot）作为团队标准？
- OQ-2：`pos_consistent` 的时间窗口（连续 N 周期）默认值是否需要在规范中给出推荐值？
- OQ-3：Wiki 页面由仓库内脚本同步，还是手工维护（需要明确流程）？
