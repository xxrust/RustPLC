# PRD: 元件库与元件级异常模型（后端 + DSL/场景解析 + 仿真最小闭环）

## 1. 介绍 / 概述

当前项目已经有传感器卡滞（`sensor_stuck`）与 force 覆盖等能力，但缺少统一的“元件库抽象”和“按元件类型定义异常”的模型。

本期目标是在**不做前端**的前提下，完成后端最小闭环：

- 定义元件库（首批：气缸、传感器、开关、步进电机）
- 定义元件级异常模型（按元件类型，不是只有通用 fault）
- 接入 DSL/场景解析与仿真执行
- 在无板调试中可稳定复现异常行为

根据你的选择，本期采用：

- 范围：后端模型 + DSL/场景解析 + 仿真最小闭环（1B）
- 元件范围：气缸、传感器、开关、步进电机（2A）
- 异常粒度：元件专属异常（3B）
- 兼容策略：不保留旧模型兼容，直接切新模型（4B）
- 完成标准：契约稳定与仿真可跑同优先级（5C）

---

## 2. Goals（目标）

- 建立统一元件库数据模型，支持 4 类首批元件。
- 建立元件专属异常模型，支持“异常定义 -> 注入 -> 仿真生效”的闭环。
- 新模型成为唯一入口：旧 `faults.sensor_stuck` / 旧 `forces` 方案不再作为正式接口。
- 保证异常注入结果可回放、可审计、可测试（确定性）。
- 输出明确文档，说明新旧格式差异和迁移方式。

---

## 3. User Stories

### US-001: 定义元件库核心 Schema
**Description:** 作为后端开发者，我希望有统一元件库 Schema，这样不同元件可以用一致方式被解析和校验。

**Acceptance Criteria:**
- [ ] 新增元件库 Schema（含 `schema_version`）
- [ ] 首批元件类型包含：`cylinder`、`sensor`、`switch`、`stepper_pd`
- [ ] 每个元件都包含基础字段：`id`、`name`、`type`、`params`
- [ ] 校验规则可返回稳定错误码（字段缺失、类型错误、重复 ID）
- [ ] Typecheck passes
- [ ] Tests pass

### US-002: 拓扑文件接入元件库并落盘
**Description:** 作为工程师，我希望拓扑文件可直接描述元件实例与连接关系，这样模型可持久化并用于后续运行。

**Acceptance Criteria:**
- [ ] 拓扑文件新增 `component_library` / `components` / `connections` 结构（或等效结构）
- [ ] 支持元件实例化与连接关系校验（端口存在、方向合法、无悬空关键连接）
- [ ] 保存后的拓扑可被 CLI 重新加载并解析一致
- [ ] 对版本不匹配给出明确错误信息与错误码
- [ ] Typecheck passes
- [ ] Tests pass

### US-003: 定义元件专属异常模型
**Description:** 作为调试工程师，我希望每类元件有自己的异常类型，这样故障注入更贴近真实问题。

**Acceptance Criteria:**
- [ ] 新增统一异常容器（含 `at_ms`、`duration_ms`、`target_component_id`、`fault_kind`）
- [ ] 气缸至少支持：`jammed`、`motion_timeout`
- [ ] 传感器至少支持：`stuck_on`、`stuck_off`、`chatter`
- [ ] 开关至少支持：`stuck_on`、`stuck_off`
- [ ] 步进电机至少支持：`lost_step`、`stall`、`direction_reversed`
- [ ] 异常模型含参数校验（阈值/比例/时间范围）
- [ ] Typecheck passes
- [ ] Tests pass

### US-004: 解析器切换到新模型并移除旧接口
**Description:** 作为系统维护者，我希望解析器仅接受新模型，避免双轨长期并存造成代码复杂度上升。

**Acceptance Criteria:**
- [ ] 解析器默认走新元件库 + 新异常模型路径
- [ ] 遇到旧字段（如旧 `faults.sensor_stuck`/旧 `forces`）时明确报错并提示迁移
- [ ] CLI 帮助与示例更新为新格式
- [ ] 错误提示包含“旧格式 -> 新格式”的最小迁移建议
- [ ] Typecheck passes
- [ ] Tests pass

### US-005: 气缸/传感器/开关仿真状态机最小闭环
**Description:** 作为仿真使用者，我希望这些元件在 tick 推进中有可预测状态变化，这样异常注入结果可验证。

**Acceptance Criteria:**
- [ ] 气缸状态至少支持：伸出/回缩/运动中
- [ ] 传感器状态可由元件逻辑或异常注入驱动
- [ ] 开关状态可作为输入源参与仿真
- [ ] tick 回放时状态变化确定性一致
- [ ] Typecheck passes
- [ ] Tests pass

### US-006: 步进电机（脉冲/方向）最小仿真模型
**Description:** 作为运动调试工程师，我希望步进电机具备最小可用模型，这样可以在离线阶段验证位置相关逻辑。

**Acceptance Criteria:**
- [ ] 步进电机支持 `pulse/dir/enable` 输入语义
- [ ] 仿真可输出位置（step 计数）与方向
- [ ] `lost_step` / `stall` / `direction_reversed` 异常会影响位置演化
- [ ] 异常启停后行为可恢复（按定义）
- [ ] Typecheck passes
- [ ] Tests pass

### US-007: 异常注入执行引擎（统一调度）
**Description:** 作为平台开发者，我希望有统一异常调度器，这样不同元件异常都能按同一规则生效。

**Acceptance Criteria:**
- [ ] 异常按 `at_ms` 与 tick 对齐执行
- [ ] 支持持续时间窗口与自动失效
- [ ] 同 tick 多异常冲突时有确定优先级规则并文档化
- [ ] 注入事件写入审计工件（machine-readable）
- [ ] Typecheck passes
- [ ] Tests pass

### US-008: 诊断输出接入元件异常上下文
**Description:** 作为排障人员，我希望诊断结果能看到“异常注入上下文”，这样更快判断是否为注入导致。

**Acceptance Criteria:**
- [ ] 诊断输出中可关联触发时段的元件异常上下文（组件 ID、异常类型、时间窗）
- [ ] 不改动现有核心诊断字段稳定性（issue_code 等）
- [ ] 证据列表可区分“程序行为证据”与“注入证据”
- [ ] Typecheck passes
- [ ] Tests pass

### US-009: 文档与迁移说明
**Description:** 作为项目维护者，我希望有明确文档说明新旧差异，这样团队可快速切换并避免误用。

**Acceptance Criteria:**
- [ ] 在 `docs/` 新增元件库与异常模型说明文档
- [ ] 明确列出旧格式与新格式差异（字段级别）
- [ ] 提供至少 2 个完整示例（正常 + 异常）
- [ ] 给出迁移步骤与常见报错对照表
- [ ] Typecheck passes
- [ ] Tests pass

---

## 4. Functional Requirements（功能需求）

- FR-1: 系统必须支持元件库 Schema 解析与版本校验。
- FR-2: 系统必须支持 4 类首批元件：气缸、传感器、开关、步进电机。
- FR-3: 系统必须支持元件实例与连接关系校验。
- FR-4: 系统必须支持元件专属异常定义与参数校验。
- FR-5: 系统必须在 tick 级执行异常注入，并可持续/失效。
- FR-6: 系统必须在仿真中体现异常对状态演化的影响。
- FR-7: 系统必须输出可审计的异常注入记录。
- FR-8: 系统必须将异常上下文接入诊断证据链。
- FR-9: 系统必须拒绝旧异常字段并提供迁移指引。
- FR-10: 系统必须提供文档化的新旧格式对照与示例。

---

## 5. Non-Goals（非目标 / 不在范围）

- 不实现前端页面与交互。
- 不实现实体板硬件驱动层改造。
- 不做高精度多体动力学仿真（本期为工程可用最小模型）。
- 不保证旧 `faults.sensor_stuck` / 旧 `forces` 配置继续可运行。

---

## 6. Design Considerations（设计考虑）

- 元件库和异常模型必须可序列化为稳定 JSON/YAML，便于 CI 与审计。
- 错误码命名保持稳定，可被 HMI/工具链引用。
- 采用“声明式配置 + 确定性执行”模式，减少隐式行为。

---

## 7. Technical Considerations（技术考虑）

- 建议新增独立模块（示例）：`component_library`、`component_faults`、`fault_scheduler`。
- 解析层和仿真层分离：解析负责合法性，仿真负责状态演化。
- 对旧字段建议返回专用迁移错误码（例如 `CFG-MIG-*`），并附修复提示。
- 关键路径需有契约测试，锁定 schema 字段与错误消息关键字。

---

## 8. Success Metrics（成功指标）

- 至少 4 类元件可在新模型下被成功解析与仿真。
- 至少 8 类元件专属异常可被注入并在仿真行为中可观测。
- 关键契约测试通过率 100%（schema/解析/仿真/诊断关联）。
- 文档示例可直接被 CLI 跑通（正常场景 + 异常场景）。

---

## 9. Open Questions（开放问题）

- 步进电机异常中的 `lost_step` 应按固定步数还是按比例丢步？
- `chatter` 的标准参数集合是否统一为频率 + 占空比？
- 元件连接关系是否需要引入“信号单位/量纲”校验（如角度/距离）在本期一起做？
- 是否需要为未来电缸/编码器 AB 相预留标准端口命名规范（即使本期不实现硬件）？
