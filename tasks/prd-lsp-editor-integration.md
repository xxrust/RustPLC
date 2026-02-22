# PRD: RustPLC LSP 优先改造（VS/VS Code 等编辑器适配）

## 1. 介绍 / 概览
当前重构方向过度偏向 UI，而你的实际优先级是 IDE 开发体验：需要先完成 RustPLC DSL 的 LSP 能力，适配 VS Code、Visual Studio 等编辑器。  
本期以 **LSP 能力闭环** 为核心，UI 仅保留最低兼容与验证工作。

## 2. 目标
- 优先交付可用的 RustPLC DSL LSP 服务。
- 支持诊断、补全、跳转、重命名、快速修复、格式化等核心能力。
- 优先完成 VS Code 与 Visual Studio（或通用 LSP 客户端）接入。
- UI 仅做最低限度支撑，不作为主交付目标。

## 3. 用户故事

### US-001: 定义 LSP 能力边界与协议契约
**Description:** 作为语言工具链维护者，我希望明确本期 LSP 的方法范围和返回结构，避免实现发散。

**Acceptance Criteria:**
- [ ] 明确支持的 LSP 方法清单（initialize/diagnostics/completion/hover/definition/references/rename/codeAction/formatting）。
- [ ] 输出统一能力矩阵文档（支持/不支持/降级策略）。
- [ ] 定义错误码与诊断等级映射规范。
- [ ] Typecheck passes。

### US-002: LSP 服务骨架与会话管理
**Description:** 作为开发者，我希望先有稳定的 LSP 服务进程和会话管理能力。

**Acceptance Criteria:**
- [ ] 提供 stdio 模式 LSP 服务入口。
- [ ] 实现文档打开/变更/关闭生命周期管理。
- [ ] 实现 workspace 多文件缓存结构。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-003: 解析与语义诊断映射
**Description:** 作为工程师，我希望编辑器中实时看到解析和语义错误定位。

**Acceptance Criteria:**
- [ ] parse 错误映射到 LSP Diagnostic（含 range/message/severity/code）。
- [ ] semantic 错误映射到 LSP Diagnostic 并保留规则来源。
- [ ] 增量编辑后诊断可更新且不闪烁。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-004: 智能补全
**Description:** 作为工程师，我希望在 topology/constraints/tasks 上下文得到准确补全。

**Acceptance Criteria:**
- [ ] 支持关键字补全、设备名补全、状态名补全。
- [ ] 支持上下文补全（如 `detects:` 后补全 `device.state`）。
- [ ] 支持常用片段补全（模板代码）。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-005: Hover 信息与文档提示
**Description:** 作为工程师，我希望悬停时可查看设备类型、来源定义和语义说明。

**Acceptance Criteria:**
- [ ] hover 显示符号类型、定义位置和简要文档。
- [ ] 对未知符号返回可读降级提示。
- [ ] hover 内容支持中英文基础描述。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-006: 跳转定义与查找引用
**Description:** 作为工程师，我希望快速在大型 PLC 文件中定位符号。

**Acceptance Criteria:**
- [ ] 支持 go-to-definition。
- [ ] 支持 find-references（当前文件 + workspace）。
- [ ] 对重复名冲突给出明确候选。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-007: 安全重命名
**Description:** 作为工程师，我希望重命名设备或任务时自动更新所有引用。

**Acceptance Criteria:**
- [ ] 支持 rename 并返回 WorkspaceEdit。
- [ ] 禁止重命名保留关键字并给出错误。
- [ ] 跨文件引用可正确更新。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-008: Code Action 快速修复
**Description:** 作为工程师，我希望对常见错误一键修复，降低迁移成本。

**Acceptance Criteria:**
- [ ] 对 `connected_to` 提供迁移到新字段的 code action。
- [ ] 对未定义符号提供候选修复建议。
- [ ] 修复前可预览变更文本。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-009: 格式化能力
**Description:** 作为工程师，我希望 PLC 文件有统一格式，便于审阅与版本管理。

**Acceptance Criteria:**
- [ ] 支持 document formatting。
- [ ] 支持 range formatting。
- [ ] 格式化不改变语义。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-010: 增量解析与性能门禁
**Description:** 作为平台维护者，我希望 LSP 在大文件和高频编辑下依然响应稳定。

**Acceptance Criteria:**
- [ ] 支持增量更新路径（避免全量重算）。
- [ ] 建立性能基线（大文件、多文件 workspace）。
- [ ] CI 加入性能回归告警阈值。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-011: VS Code 客户端适配
**Description:** 作为开发者，我希望 VS Code 能开箱使用 RustPLC LSP。

**Acceptance Criteria:**
- [ ] 提供 VS Code language client 配置与启动脚本。
- [ ] 在 VS Code 中验证诊断/补全/跳转/重命名/格式化。
- [ ] 输出安装与使用说明。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-012: Visual Studio 与通用 LSP 客户端接入指引
**Description:** 作为团队成员，我希望在 VS 等非 VS Code 环境也能快速接入。

**Acceptance Criteria:**
- [ ] 提供 Visual Studio（或等价 LSP 客户端）的接入步骤文档。
- [ ] 提供最小可运行配置示例。
- [ ] 提供常见问题与排错指南。
- [ ] Typecheck passes。

### US-013: 测试治理（去重与参数化）
**Description:** 作为测试维护者，我希望降低重复测试成本并保持有效覆盖。

**Acceptance Criteria:**
- [ ] 输出测试盘点矩阵（Parser/Semantic/LSP/API/UI）。
- [ ] 重复测试改为参数化/table-driven。
- [ ] 删除无断言或无业务价值测试。
- [ ] Typecheck passes。
- [ ] Tests pass。

### US-014: UI 低优先保底兼容
**Description:** 作为产品维护者，我希望 UI 仅保持基础可用，不阻塞 LSP 主线。

**Acceptance Criteria:**
- [ ] 保留最小拓扑加载与渲染能力。
- [ ] UI 改动仅限修复阻塞性问题。
- [ ] 不新增高复杂交互功能。
- [ ] Typecheck passes。
- [ ] Verify in browser using dev-browser skill。

## 4. 功能需求
- FR-1: 本期主交付为 LSP 核心能力，不以 UI 为主。
- FR-2: LSP 必须支持诊断、补全、hover、跳转、引用、重命名、代码修复、格式化。
- FR-3: LSP 必须支持 workspace 级多文件分析。
- FR-4: 必须提供 VS Code 适配与 VS/通用客户端接入文档。
- FR-5: 测试体系必须去重并保留关键回归覆盖。
- FR-6: UI 仅做低优先保底兼容。

## 5. 非目标
- 不在本期推进高复杂 UI 交互（标签分组大改、拓扑高级编辑器等）。
- 不在本期实现所有可选 LSP 扩展特性（如 call hierarchy、semantic tokens 全量）。

## 6. 技术考虑
- 优先复用现有 parser/semantic 能力，避免重复实现语言前端。
- 采用增量缓存与失效策略，控制大文件响应时延。
- 建立 LSP 协议层与语义层分层测试，降低回归成本。

## 7. 成功指标
- VS Code 中核心 LSP 能力可稳定使用。
- Visual Studio / 通用 LSP 客户端可按文档接入。
- 大文件场景下编辑反馈延迟满足门限。
- 测试总量下降但有效覆盖率提升。

## 8. 开放问题
- Visual Studio 适配采用原生插件方式还是文档化通用 LSP 接入。
- 增量解析策略采用语法树局部更新还是文件级缓存重算。
