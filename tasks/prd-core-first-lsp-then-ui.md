# PRD v2: RustPLC 核心优先改造（核心能力 -> LSP -> UI）

## 1. 背景与目标（无缓存可读）
本期唯一主线：
1) 先完成**核心语义与可扩展能力**（拓扑方向、端口模型、MIMO、标签、测试治理、性能门禁）；
2) 再完成**LSP 编辑器能力**（VS Code / Visual Studio / 通用 LSP）；
3) 最后做**UI 保底兼容**（只保证不阻塞，不反向牵引核心设计）。

禁止反向依赖：UI 需求不得改变核心语义与数据模型。

## 2. 术语与方向定义（必须先读）
- **Producer（生产者）**：产生信号的一侧（如传感器输出端、控制器 Y 端）。
- **Consumer（消费者）**：消费信号的一侧（如控制器 X 端、执行器命令端）。
- **Port（端口）**：一等公民，所有连线必须是 `from_port -> to_port`。
- **Relation（关系类型）**：
  - `driven_by`：命令消费关系（语义归一为 producer -> consumer）。
  - `reports_to`：检测上报关系（语义归一为 producer -> consumer）。
  - `detects`：传感器与被观测对象关系（非命令通道）。
- **字段规则**：DSL 采用白名单字段；凡是不在规范内的字段，一律按编译错误处理。

### 2.1 规范样例（two_cylinder 语义）
目标是保证“两个传感器可区分、不可合并到同一出口语义”：
- `Y1 -> cylinder_A.cmd_extend`
- `sensor_A_extended.out -> X1`
- `sensor_A_retracted.out -> X2`

要求：`extended` 与 `retracted` 在拓扑中必须落到不同的 `from_port/to_port` 组合。

## 3. 阶段门禁（硬约束）
- **Gate-Core**：US-001 ~ US-013 全部 `passes=true` 前，US-014（LSP）不得开始。
- **Gate-LSP**：US-014 ~ US-016 全部 `passes=true` 前，US-017（UI）不得开始。
- **Gate-Release**：UI 仅验收“兼容性”，不能引入改变核心模型的新需求。

## 3.1 全局执行规则放置位置
- 反复执行的规则（如“先读架构文档”“未知字段必须编译失败”）统一写在 `CODEX.md` / `prompt.md`。
- PRD 只写“该故事特有的增量目标与验收”，避免在每个故事里重复同一前置语句。

## 4. 用户故事（按依赖顺序）

### US-001: 冻结核心术语与方向规范
**Description:** 作为架构负责人，我希望用文档冻结术语和方向，避免后续解释漂移。

**Acceptance Criteria:**
- [ ] 新增 `docs/architecture/signal-direction.md`，明确 producer/consumer/port/relation 定义。
- [ ] 文档包含 two_cylinder 的可执行语义样例（含 extended/retracted 区分）。
- [ ] 文档声明“连线只允许端口到端口”。
- [ ] 文档声明“仅允许 DSL 白名单字段，未知字段一律编译失败”。
- [ ] 文档写明“US-002 及后续故事必须先读取本文件后再实现”。
- [ ] Typecheck passes。

### US-002: DSL 字段与端口/标签 Schema 定稿
**Description:** 作为 DSL 设计者，我希望新模型可直接编码核心语义。

**Acceptance Criteria:**
- [ ] DSL 层定义并采用 `driven_by`、`reports_to`、`detects`。
- [ ] 端口结构最少包含 `id/type/role`。
- [ ] 标签结构最少包含 `functional_group`、`danger_level`、`location_group`。
- [ ] `location_group` 支持层级格式（如 `line/cell/station`）。
- [ ] Typecheck passes。

### US-003: Parser/AST 迁移到新语义并拒绝非规范字段
**Description:** 作为编译器开发者，我希望 Parser/AST 原生承载新语义并对任何非规范字段直接阻断。

**Acceptance Criteria:**
- [ ] AST 仅存储规范字段，不保留任何非规范字段语义路径。
- [ ] Parser 遇到未知字段直接报阻断错误（含文件、行列、字段名）。
- [ ] 错误码可被 API/LSP 复用。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-004: 拓扑图数据模型端口化
**Description:** 作为核心维护者，我希望拓扑图结构以端口为主键，避免节点级歧义。

**Acceptance Criteria:**
- [ ] 图边结构统一为 `from_port/to_port/relation/signal`。
- [ ] 节点结构可枚举端口定义与端口角色。
- [ ] 同一节点允许多输入多输出端口并存。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-005: 语义构图实现 producer -> consumer 与 MIMO
**Description:** 作为语义分析维护者，我希望构图规则在 MIMO 场景稳定可预测。

**Acceptance Criteria:**
- [ ] 构图结果统一方向为 producer -> consumer。
- [ ] 支持 1->N、N->1、N->M。
- [ ] 端口类型不匹配时输出可定位错误（含端口 ID）。
- [ ] `two_cylinder.plc` 中 `extended/retracted` 必须映射到不同目标端口。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-006: scenario_resolve 与别名解析适配新方向
**Description:** 作为链路维护者，我希望场景解析层不再依赖旧方向假设。

**Acceptance Criteria:**
- [ ] scenario_resolve 只消费新关系语义。
- [ ] 别名解析支持端口级目标。
- [ ] 异常路径输出统一错误结构。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-007: timing/causality/safety/runtime_bridge 适配
**Description:** 作为运行时维护者，我希望运行链路和语义层一致。

**Acceptance Criteria:**
- [ ] timing/causality/safety 不再依赖任何旧字段方向假设。
- [ ] runtime_bridge 消费端口级边数据。
- [ ] `two_cylinder`、`assembly_station`、至少 1 个 MIMO 示例回归通过。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-008: 存量 .plc 全量规范化（旧写法 -> 新 DSL）
**Description:** 作为维护者，我希望旧工程中的非规范字段一次性清零并可审计。

**Acceptance Criteria:**
- [ ] 提供迁移命令（CLI 或脚本）把旧写法转换为规范关系字段。
- [ ] 对 `src/`、`crates/`、`examples/`、`scenarios/`、`tests/` 下全部 `.plc` 完成存量清零。
- [ ] 无法自动迁移项写出待人工确认清单并要求清理后才可合并。
- [ ] 迁移结果支持 dry-run 与实际写回。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-009: CI 禁回流（阻止任何非规范字段）
**Description:** 作为平台维护者，我希望 CI 阶段阻止非规范字段回流并持续保持零存量。

**Acceptance Criteria:**
- [ ] 新增 CI 检查，在受管 `.plc` 目录发现任意未知字段即失败（不限新增/旧文件）。
- [ ] CI 输出违规文件路径与行号。
- [ ] 合并前校验结果必须为“未知字段=0”。
- [ ] `cargo check --workspace` 通过。

### US-010: 标签规则引擎（功能/危险/位置）
**Description:** 作为安全负责人，我希望标签可驱动编译期规则。

**Acceptance Criteria:**
- [ ] 支持 `danger_level` 规则（示例：高危对象需双通道检测）。
- [ ] 支持 `functional_group` 组内/组间规则。
- [ ] 支持 `location_group` 区域隔离与跨区约束。
- [ ] 违规输出 `code/path/message/suggestion`。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-011: 标签驱动批量改造、语义 diff 与回滚
**Description:** 作为工程师，我希望按标签快速批改并可安全撤销。

**Acceptance Criteria:**
- [ ] 支持按 `functional_group`/`danger_level`/`location_group` 选择对象。
- [ ] 支持批量改参数、连线、命名。
- [ ] 支持语义 diff 预览（节点/端口/关系/标签）。
- [ ] 支持回滚（快照或操作日志）。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-012: parse-plc / topology API 契约稳定化
**Description:** 作为调用方，我希望 API 元数据足够完整并可长期兼容。

**Acceptance Criteria:**
- [ ] API 返回 `relation/from_port/to_port/signal/tags`。
- [ ] API 返回节点端口定义（含 role/type）。
- [ ] `two_cylinder.plc` 响应中可区分 `extended/retracted`。
- [ ] MIMO 示例返回端口级连线正确。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-013: 测试治理 + 核心性能门禁 + 影响分析
**Description:** 作为平台维护者，我希望在减测成本同时守住质量与规模能力。

**Acceptance Criteria:**
- [ ] 输出测试盘点矩阵（Parser/Semantic/Runtime/API/LSP/UI）。
- [ ] 重复测试改为参数化或 table-driven，删除无效测试。
- [ ] 保留强回归集：`two_cylinder`、`assembly_station`、MIMO。
- [ ] 建立 500 节点/2000 边基线，并纳入 CI 性能阈值。
- [ ] 输出语义影响分析报告（节点/端口/关系/标签）。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-014: LSP 骨架 + 诊断/补全/Hover
**Description:** 作为 IDE 用户，我希望先获得实时反馈能力。

**Acceptance Criteria:**
- [ ] 提供 stdio LSP 服务入口与 workspace 缓存。
- [ ] parse/semantic 错误映射为 LSP Diagnostic。
- [ ] 支持关键字/设备/状态/上下文补全与 Hover 基础信息。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-015: LSP 跳转/引用/重命名/修复/格式化
**Description:** 作为 IDE 用户，我希望具备完整日常编辑能力。

**Acceptance Criteria:**
- [ ] 支持 definition/references/rename。
- [ ] 支持 code action（含非规范字段迁移建议）。
- [ ] 支持 document formatting 与 range formatting。
- [ ] `cargo check --workspace` 通过。
- [ ] `cargo test --workspace` 通过。

### US-016: VS Code / Visual Studio / 通用 LSP 接入
**Description:** 作为团队成员，我希望主流编辑器均可接入。

**Acceptance Criteria:**
- [ ] 提供 VS Code 客户端配置、安装、调试文档。
- [ ] 提供 Visual Studio 或通用 LSP 接入步骤。
- [ ] 提供常见故障排查清单。
- [ ] `cargo check --workspace` 通过。

### US-017: UI 末位保底兼容（不牵引核心）
**Description:** 作为产品维护者，我希望 UI 在最后阶段仅做兼容性兜底。

**Acceptance Criteria:**
- [ ] UI 能加载并显示端口级拓扑边。
- [ ] two_cylinder 在 UI 中可区分 extended/retracted 两条传感器链路。
- [ ] UI 本期只修阻塞问题，不新增复杂交互。
- [ ] `cargo check --workspace` 通过。
- [ ] Verify in browser using dev-browser skill。

## 5. 功能需求（FR）
- FR-1: 核心能力优先于 LSP，LSP 优先于 UI。
- FR-2: 拓扑方向唯一为 producer -> consumer。
- FR-3: 连线必须端口到端口。
- FR-4: 仅允许 DSL 白名单字段；非规范字段必须清零并由 CI 持续守护。
- FR-5: 标签必须支持 `functional_group`、`danger_level`、`location_group`。
- FR-6: 系统必须支持标签规则校验与批量改造回滚。
- FR-7: API 必须输出关系、端口、标签核心元数据。
- FR-8: 测试必须去重且保留强回归。
- FR-9: CI 必须包含性能阈值和回流阻断。
- FR-10: LSP 必须覆盖诊断、补全、悬停、跳转、引用、重命名、修复、格式化。
- FR-11: UI 只做末位兼容，不可主导核心设计。

## 6. 非目标
- 不在本期推进高复杂 UI 交互能力。
- 不保留旧语义作为长期双轨运行方案。

## 7. 成功指标
- 核心链路：`two_cylinder` + `assembly_station` + MIMO 示例全部通过。
- 语义正确性：端口级连线中 `extended/retracted` 可稳定区分。
- 质量与效率：测试总量下降但有效覆盖不下降。
- 可用性：LSP 在 VS Code 与 Visual Studio/通用客户端可接入。
- 风险控制：UI 不阻塞核心交付且不反向牵引模型。

## 8. 开放决策
- 批量回滚优先“快照”还是“操作日志”。
- Visual Studio 采用原生扩展还是通用 LSP 桥接。
