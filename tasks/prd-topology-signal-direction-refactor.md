# PRD: RustPLC 拓扑语义与标签驱动重构（生产者 -> 消费者 + MIMO）

## 1. 介绍 / 概览
当前 DSL 中 `connected_to` 存在多义性，导致拓扑方向、I/O 绑定与前端渲染理解不一致。随着系统复杂度提升（多输入多输出、风险分级、功能分组、批量改造），现有模型在语义清晰性、可扩展性和测试成本上都已到瓶颈。

本需求进行一次结构性重构：
- 统一拓扑方向为 **生产者 -> 消费者**；
- 用明确字段替代 `connected_to`；
- 将端口与标签设为一等公民；
- 建立标签驱动的批量修改与规则校验；
- 清理和重构测试体系，减少重复测试并增强有效回归。

## 2. 目标
- 彻底移除 `connected_to` 语义歧义。
- 支持复杂 MIMO 拓扑（多入多出、汇聚分流、端口级连接）。
- 支持器件按标签分组与批量改造（功能、危险等级等维度）。
- 在不牺牲质量的前提下降低测试重复与维护成本。
- 保证 `two_cylinder` 与至少一个复杂 MIMO 示例全链路可用。

## 3. 用户故事

### US-001: 新 DSL 关系字段与端口模型规范
**Description:** 作为 DSL 设计者，我希望用明确字段和端口模型表达拓扑关系，避免连接语义混乱。

**Acceptance Criteria:**
- [ ] 定义并采用 `driven_by`、`reports_to`、`detects` 字段。
- [ ] 端口为一等公民，端口定义包含 `id/type/role`。
- [ ] 明确禁止 `connected_to`。
- [ ] Typecheck passes。

### US-002: Parser/AST 支持新字段与端口级连线
**Description:** 作为编译器开发者，我希望解析器和 AST 原生支持端口级连线和新语义。

**Acceptance Criteria:**
- [ ] AST 移除 `connected_to` 存储路径。
- [ ] 连线结构支持 `from_port/to_port`。
- [ ] 遇到 `connected_to` 返回迁移错误（含行号）。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-003: 语义构图统一 producer -> consumer
**Description:** 作为语义分析维护者，我希望所有拓扑边都按统一方向构图。

**Acceptance Criteria:**
- [ ] 拓扑构图仅允许 producer -> consumer。
- [ ] 支持一对多、多对一、多对多端口连接。
- [ ] 方向错误提示清晰可定位。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-004: 场景解析、验证器、运行桥接适配
**Description:** 作为运行链路维护者，我希望场景解析和验证执行与新方向完全一致。

**Acceptance Criteria:**
- [ ] scenario_resolve 和别名解析适配新方向。
- [ ] timing/causality/safety/runtime_bridge 去除旧方向假设。
- [ ] two_cylinder 与 assembly_station 回归通过。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-005: 旧 DSL 迁移工具与 CI 禁回流
**Description:** 作为维护者，我希望旧 DSL 可批量迁移并防止旧写法回流。

**Acceptance Criteria:**
- [ ] 提供旧 DSL -> 新 DSL 迁移命令。
- [ ] 无法自动迁移项输出人工确认提示。
- [ ] CI 增加“禁止新增 connected_to”规则。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-006: 标签模型设计（多维标签）
**Description:** 作为系统建模者，我希望器件支持多维标签，便于按功能、风险和位置管理拓扑。

**Acceptance Criteria:**
- [ ] 标签支持多维结构（如 `functional_group`、`danger_level`、`location_group`）。
- [ ] 支持一个器件多个标签。
- [ ] 标签 schema 在 DSL/API/store 统一。
- [ ] Typecheck passes。

### US-007: DSL/API/Store 标签一致化
**Description:** 作为全栈开发者，我希望标签在 DSL、后端 API、前端状态中含义一致。

**Acceptance Criteria:**
- [ ] parse-plc 与拓扑 API 返回标准化 `tags`。
- [ ] 前端 store 持久化标签信息。
- [ ] 标签字段文档化并版本化。
- [ ] `location_group` 支持层级表达（如 `line_a/cell_2/station_7`）。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-008: 标签驱动批量改造能力
**Description:** 作为工程师，我希望按标签批量修改端口、参数、连线与命名，以快速变更代码。

**Acceptance Criteria:**
- [ ] 支持按标签筛选器件并批量修改属性。
- [ ] 批量操作可预览 diff 并支持回滚。
- [ ] 变更后可导出/写回拓扑代码。
- [ ] Typecheck passes。
- [ ] Verify in browser using dev-browser skill。

### US-009: 标签规则引擎（功能、危险等级与位置）
**Description:** 作为安全负责人，我希望标签可驱动编译期规则校验。

**Acceptance Criteria:**
- [ ] 支持按 `danger_level` 配置约束规则（例如高危器件强制双通道检测）。
- [ ] 支持按 `functional_group` 配置组内/组间连接规则。
- [ ] 支持按 `location_group` 配置区域隔离/跨区连接约束。
- [ ] 规则违规输出结构化错误（code/path/message）。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-010: 标签可视化分组与过滤
**Description:** 作为前端用户，我希望按标签高亮、过滤、分组查看拓扑，并在故障时快速定位区域。

**Acceptance Criteria:**
- [ ] 支持按标签过滤节点和边。
- [ ] 支持按标签分组高亮。
- [ ] 标签变更后视图实时更新。
- [ ] 支持按 `location_group` 一键定位故障相关区域与邻近器件。
- [ ] Typecheck passes。
- [ ] Verify in browser using dev-browser skill。

### US-011: parse-plc API 输出关系与端口元数据
**Description:** 作为前端调用方，我希望 parse-plc 输出可直接驱动端口级渲染。

**Acceptance Criteria:**
- [ ] API 返回 `relation/from_port/to_port/signal`。
- [ ] API 返回节点端口定义与标签信息。
- [ ] two_cylinder 可区分 `extended/retracted` 两条边。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-012: 前端端口契约与连线绑定重构
**Description:** 作为前端开发者，我希望连线严格依赖端口元数据，支持动态端口数量。

**Acceptance Criteria:**
- [ ] 端口契约覆盖 cylinder/sensor/switch/stepper/generic。
- [ ] 连线绑定 `sourceHandle/targetHandle/label` 并校验端口类型。
- [ ] 缺失端口元数据时显示降级样式和警告。
- [ ] Typecheck passes。
- [ ] Verify in browser using dev-browser skill。

### US-013: 测试盘点与参数化重构
**Description:** 作为测试维护者，我希望先盘点测试覆盖并把重复用例参数化。

**Acceptance Criteria:**
- [ ] 输出测试盘点矩阵（Parser/Semantic/Runtime/API/UI）。
- [ ] 重复逻辑测试改为参数化/table-driven。
- [ ] 保留关键回归集（two_cylinder、assembly_station、MIMO 示例）。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-014: 无效测试清理与回归增强
**Description:** 作为测试维护者，我希望删除无效测试并补齐契约测试。

**Acceptance Criteria:**
- [ ] 删除无断言、重复覆盖、无业务价值的无效测试。
- [ ] 新增端口契约与标签规则契约测试。
- [ ] 关键 E2E 回归通过。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-015: 语义 Diff 与影响分析
**Description:** 作为评审者，我希望查看拓扑语义差异与影响范围，降低批量改造风险。

**Acceptance Criteria:**
- [ ] 提供语义级 diff（节点/端口/关系/标签变化）。
- [ ] 标签或连线变更后输出影响分析（受影响规则/测试/模块）。
- [ ] 评审输出可用于审计记录。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-016: 性能门禁与规模基线
**Description:** 作为平台维护者，我希望在大规模拓扑下有明确性能门禁。

**Acceptance Criteria:**
- [ ] 建立 500 节点 / 2000 边基线样例。
- [ ] 编译、解析、渲染关键路径性能指标可度量。
- [ ] CI 加入性能回归告警阈值。
- [ ] Typecheck passes。
- [ ] Verify in browser using dev-browser skill。

## 4. 功能需求
- FR-1: 拓扑边方向唯一为 producer -> consumer。
- FR-2: DSL 必须使用 `driven_by` / `reports_to` / `detects`。
- FR-3: `connected_to` 必须报错并提示迁移。
- FR-4: 拓扑连线必须是端口到端口。
- FR-5: 系统必须支持一对多、多对一、多对多连接。
- FR-6: 标签必须支持多维结构与多标签（至少包含功能、危险等级、位置分组）。
- FR-7: 标签必须支持批量改造与规则校验。
- FR-8: API 必须返回关系、端口、标签元数据。
- FR-9: 前端必须按端口元数据渲染并支持标签过滤分组。
- FR-10: 测试体系必须去重并保留强回归集。
- FR-11: 系统必须提供语义 diff 与影响分析能力。
- FR-12: 系统必须具备大规模性能门禁。

## 5. 非目标
- 不保留旧语法兼容。
- 不在本阶段引入全新设备类型语言（仅扩展端口与标签能力）。
- 不重做整体 UI 框架。

## 6. 设计考虑
- 单图层展示全部关系，关系样式区分。
- 标签作为视图组织与规则执行的统一索引。
- 动态端口布局需保证可读性与可操作性。

## 7. 技术考虑
- 改动覆盖 parser/ast/semantic/verification/runtime/web-server/web-ui。
- 需配套迁移工具、示例批量改写、测试矩阵治理。
- 需定义稳定的数据契约版本策略（DSL/API/Store）。

## 8. 成功指标
- 示例与文档中 `connected_to` 使用量为 0。
- two_cylinder 与 MIMO 示例均能稳定端口级渲染。
- 标签驱动批量改造可在真实示例中复用。
- 故障发生后，工程师可在 UI 内基于 `location_group` 在 3 步内定位到目标区域。
- 测试总量下降但有效覆盖率提升，关键回归全部通过。
- 500 节点 / 2000 边基线下性能在门限内。

## 9. 开放问题
- `reports_to` 是否仅允许物理输入口（X/AI）或允许逻辑别名口。
- 批量改造回滚机制采用快照回滚还是操作日志回滚。
- 性能门禁阈值按固定值还是按历史滑窗动态阈值。
- `location_group` 层级深度是否固定（线体/单元/工位）还是允许自定义层级。
