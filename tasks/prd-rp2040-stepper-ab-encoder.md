# PRD：RP2040 实机步进轴（Pulse/Dir）+ AB 编码器接入（Ralph 可执行拆分版）

日期：2026-02-18
状态：Draft（待你审核后再转 `prd.json` 并执行 Ralph）

本 PRD 采用你已确认的首版决策：
- **1A**：脉冲/AB 实现优先 **PIO**（稳定可回归优先）。
- **2A**：PLC 与 motion 的接口走“**motion 配置段 + 固件内部虚拟反馈通道**”，不强行复用 ADC-only 的 `analog_inputs` 语义。
- **3B**：首版做 **双轴（axis0/axis1）**。
- **4B**：首版运动命令支持 **简单加减速（trapezoid）**，不做 S 曲线/jerk。

---

## 1. 背景与目标

当前仓库已完成：
- Stepper/AB 的**建模文档**与 DSL 规则模板（`zone_code`、双向互锁、拓扑抽象）
- 示例与回归场景（SIL 侧）

当前缺口：
- `board-rp2040` 侧尚未实现真实的**脉冲/方向输出**与**AB 解码/高速计数**能力
- 也未形成“板级运动反馈 -> DSL 输入”的统一工程接口

本 PRD 目标是把“文档级能力”推进为“可运行的板级能力”，并保证可回归、可交付。

---

## 2. 总体方案（本期决策）

1. **分层不变**：DSL 负责顺控/互锁/安全；板级负责实时脉冲与 AB 计算。  
2. **清晰优先（开发阶段可破兼容）**：允许引入新的 `io_map` 运动配置段，不为旧格式做复杂兼容分支。  
3. **板级输出“工程信号”给 DSL**：最小集合包括 `axis_count`、`axis_speed`、`enc_dir_positive`、`inpos/busy/alarm`、`zone_code`。  
4. **防碰撞规则沿用已落地模型**：`zone_code` + 执行器互斥 + `move_cmd` 双向互锁。

---

## 3. 范围（In Scope）

- RP2040 上的 Step/Dir/EN 控制通道（**双轴 axis0/axis1**）
- AB 相解码与计数、方向、速度估计（**双轴 axis0/axis1**）
- 运动命令：启停、方向、目标/速度，以及 **trapezoid 加减速**（首版简化）
- 板级反馈信号映射到 RustPLC 运行时输入（虚拟反馈通道）
- 对应 io_map 配置、解析、校验、示例、测试、wiki
- HIL/PIL 回归链路中的运动能力证据

## 4. 非目标（Out of Scope）

- 在线轨迹规划（S 曲线、jerk 限制等高级 profile）
- 多轴联动插补/运动学逆解
- 在 DSL 内直接处理 AB 原始边沿
- 闭环伺服控制器设计（本期仅做状态与反馈接入）

---

## 5. 用户故事（Ralph 一轮可完成粒度，13 条）

### US-001：定义并落地 io_map 运动配置段（破兼容、清晰优先）
**Description**：作为固件开发者，我需要在 `io_map` 中声明 stepper/encoder 通道，避免把运动映射散落在代码里。  
**Acceptance Criteria**：
- [ ] 增加统一运动配置段并文档化（至少包含 `axis0`、`axis1`）。
- [ ] 每轴明确必填项：step/dir/en 引脚、A/B 引脚、计数方向、PPR/倍率、缩放参数等。
- [ ] 明确 per-axis 与 shared 字段边界（例如共享 tick、共享 trace 格式）。
- [ ] 对非法配置给出可定位报错（字段路径 + 原因）。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-002：实现运动配置解析与语义校验
**Description**：作为维护者，我希望解析阶段拦截冲突和越界，减少板上调试成本。  
**Acceptance Criteria**：
- [ ] 新增解析器/校验器，覆盖引脚冲突、重复轴 ID、非法参数范围。
- [ ] 增加单元测试覆盖成功与失败路径。
- [ ] 失败信息包含配置路径与修复提示。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-003：扩展 board-rp2040 HAL 最小形状以容纳 motion 子系统
**Description**：作为架构维护者，我希望在现有 HAL 结构中接入 motion 生命周期，避免主循环散乱增长。  
**Acceptance Criteria**：
- [ ] 在 `initialize/update_in/update_out/finalize_on_error` 流程中加入 motion hook。
- [ ] motion 子模块独立文件组织，主流程保持可读。
- [ ] 不破坏现有 TRACE/TIMING 输出与 safe-state 行为。
- [ ] Cross build passes (`cargo build -p board-rp2040 --target thumbv6m-none-eabi --release`)。
- [ ] Typecheck passes。

### US-004：实现 PIO Step 发生器（axis0，支持 trapezoid）
**Description**：作为设备工程师，我希望 RP2040 能用 PIO 稳定输出 axis0 的方向与 step 脉冲，并支持简单加减速。  
**Acceptance Criteria**：
- [ ] axis0 的 step/dir/en 按 `io_map.motion` 配置绑定到 GPIO。
- [ ] 支持基本命令：enable/disable、start/stop、dir、目标（target steps/count）或目标速度。
- [ ] 支持 trapezoid profile：加速段/匀速段/减速段（参数可配置，先做最小集）。
- [ ] 方向切换有保护策略（例如 stop 后再翻向，或最小 deadtime）。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-005：实现 PIO Step 发生器（axis1，复用/抽象为双轴）
**Description**：作为设备工程师，我希望 axis1 也具备同等能力，并且代码结构能支撑双轴而不复制粘贴。  
**Acceptance Criteria**：
- [ ] axis1 的 step/dir/en 按配置绑定并可输出脉冲。
- [ ] 双轴共享一套实现骨架（抽象出 axis struct / trait），避免两份逻辑分叉。
- [ ] Cross build passes (`cargo build -p board-rp2040 --target thumbv6m-none-eabi --release`)。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-006：实现 AB 解码 + 高速计数 + 方向/速度估计（axis0）
**Description**：作为控制开发者，我希望板级实时产出 axis0 的 count/speed/dir_sign，供 DSL 可靠消费。  
**Acceptance Criteria**：
- [ ] axis0 的 AB 解码实现可在 tick 级输出 count 快照与 dir_sign。
- [ ] 速度估计在 tick 边界稳定（定义清晰的 dt 与单位）。
- [ ] 对异常边沿/抖动有最小过滤策略，并在 docs/wiki 说明。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-007：实现 AB 解码 + 高速计数 + 方向/速度估计（axis1）
**Description**：作为控制开发者，我希望 axis1 也具备同等 AB 反馈能力，且实现结构可维护。  
**Acceptance Criteria**：
- [ ] axis1 AB 解码与速度估计可用，并通过单测/集成测试覆盖。
- [ ] 双轴实现共享核心代码（抽象/复用），避免两份逻辑分叉。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-008：把 motion 反馈映射为 RustPLC 可用输入信号（虚拟反馈通道）
**Description**：作为 DSL 使用者，我希望直接在 PLC 中使用 `axis0_count/axis0_speed/axis1_count/...` 等信号。  
**Acceptance Criteria**：
- [ ] 定义并实现“板级反馈 -> 虚拟 DI/AI 逻辑通道”映射规则（不依赖 ADC-only AI）。
- [ ] `scenario-validate` 能识别这些通道并给出正确校验（至少覆盖 count/speed/dir_sign）。
- [ ] trace 工件中可观测到关键反馈变化。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-009：提供可运行示例（stepper_ab_board_minimal，双轴）
**Description**：作为项目使用者，我希望有一套最小示例能直接演示“运动命令 + 编码器反馈 + fault 路径”。  
**Acceptance Criteria**：
- [ ] 新增 `.plc` + `scenario` + `io_map` 示例，覆盖 axis0/axis1 的正常、计数卡住、方向错误。
- [ ] 示例包含 timeout -> fault 的可见路径。
- [ ] 示例命令可一键执行并产出 trace/report。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-010：落地防碰撞闭环（zone_code + 双向互锁）到板级链路
**Description**：作为安全负责人，我希望已有防碰撞模型在板级反馈下可验证，不仅停留在文档。  
**Acceptance Criteria**：
- [ ] 示例中使用 `zone_code` 与执行器互斥规则。
- [ ] 示例中使用 `move_cmd` 双向互锁规则。
- [ ] 至少 1 个危险窗口案例触发受控 fault，不出现“静默越界”。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-011：补齐运动相关回归测试与门禁
**Description**：作为 CI 维护者，我希望运动能力纳入自动回归，防止后续改动退化。  
**Acceptance Criteria**：
- [ ] 新增运动场景回归测试（正常 + 两类故障至少 3 条）。
- [ ] 回归测试纳入现有 workflow，失败可复现。
- [ ] 关键失败日志包含轴 ID/信号名/时间点。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

### US-012：文档与格式变更说明（含旧格式对比）
**Description**：作为团队成员，我希望清楚知道 `io_map` 与输出工件有哪些变化，方便迁移。  
**Acceptance Criteria**：
- [ ] 在 `docs/` 新增“motion io_map 变更说明”文档。
- [ ] 明确列出旧格式 vs 新格式字段对照与迁移示例。
- [ ] 说明 trace/report 新增字段及其语义。
- [ ] 文档命令可在本地复现。
- [ ] Typecheck passes。

### US-013：补充 wiki（离线可读）
**Description**：作为后续维护者，我希望 wiki 沉淀“怎么配、怎么测、怎么排障”。  
**Acceptance Criteria**：
- [ ] 更新 `docs/wiki` 至少 2 页：实现指南 + CI/排障。
- [ ] wiki 与 `docs/stepper_ab_encoder.md` 术语一致。
- [ ] 提供“从 0 到回归通过”的命令清单。
- [ ] Tests pass (`cargo test --workspace`)。
- [ ] Typecheck passes。

---

## 6. 功能需求（Functional Requirements）

- FR-1：系统必须支持在配置中定义 step/dir/en 与 AB 引脚。
- FR-2：系统必须支持双轴（axis0/axis1）的脉冲/方向控制生命周期。
- FR-3：系统必须输出双轴编码器计数、方向与速度快照。
- FR-4：系统必须支持板级反馈映射为 DSL 可消费输入信号。
- FR-5：系统必须在示例中体现 timeout/fault 保护路径。
- FR-6：系统必须支持 `zone_code` + 双向互锁防碰撞规则。
- FR-7：系统必须具备可复现的运动回归测试与 CI 门禁。
- FR-8：系统必须提供格式变更文档与迁移示例。

---

## 7. 技术约束与实现建议

- 建议优先复用现有 `board-rp2040` HAL 结构，不在 `main.rs` 堆叠大块逻辑。
- 运动信号映射需避免与 ADC-only AI 语义冲突（引入清晰的“虚拟反馈通道”定义）。
- Safe-state 语义继续保持：故障时输出进入配置定义的安全态。
- 证据链保持一致：TRACE/TIMING/board-parse/trace-diff 可继续工作。

---

## 8. 里程碑与依赖顺序（供 Ralph 执行）

推荐优先级：
1. US-001
2. US-002
3. US-003
4. US-004
5. US-005
6. US-006
7. US-007
8. US-008
9. US-009
10. US-010
11. US-011

说明：前 6 条完成后，板级核心能力闭环可用；后续为示例、门禁、文档沉淀。

---

## 9. 成功指标（Success Metrics）

- 在 RP2040 目标构建中，运动相关代码可稳定编译并通过回归。
- 新增示例可稳定复现“正常 + 故障”路径。
- 防碰撞规则在板级反馈下可验证，不依赖手工解释。
- 新成员可按 wiki 在半天内完成“配置 -> 运行 -> 回归 -> 排障”的完整流程。

---

## 10. 开放问题（待你确认）

- OQ-1：AB 解码首版采用纯 PIO（自写 quadrature decoder）还是引入外部计数芯片/模块（若有）？
- OQ-2：trapezoid profile 的最小参数集（例如 `v_max/acc/dec` vs `v_max/acc`）你希望固定默认还是必须配置？
- OQ-3：虚拟反馈通道的命名规范：`axis0_count/axis1_count` 还是更通用的 `motion.axis0.count`？
