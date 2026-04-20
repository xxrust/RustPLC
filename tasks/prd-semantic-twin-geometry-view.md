# PRD: Semantic Twin Geometry View

## Introduction

为 RustPLC 增加一个“语义孪生几何视图”能力，把 `TopologyGraph`、`StateMachine`、`ConstraintSet`、runtime trace、intent-alignment report 汇总成统一 geometry artifact。该能力解决的问题不是物理数字孪生，而是“在不看代码、没有真实设备时，如何快速建立对程序逻辑闭环的信心”。

本 PRD 基于当前已知仓库事实直接收敛，未等待额外访谈。未冻结问题放在 Open Questions 中。

## Goals

- 让用户在不读 DSL 源码时，也能快速理解系统主流程、阻塞点、资源冲突面和故障分流面。
- 把 authored / derived / verified / observed / blocked 五类事实统一投影到一个稳定工件。
- 提供可被 CLI、Web UI、SVG、动画回放共同消费的 geometry artifact。
- 让 trace 和 intent-alignment 结果可以叠加到同一个语义几何上，而不是散落在多个独立报告里。

## User Stories

### US-001: 导出静态几何工件
**Description:** 作为 PLC 开发者，我希望从一个 `.plc` 或 `.bundle.toml` 直接导出语义几何工件，这样我可以在不写前端的情况下先验证结构是否完整。

**Acceptance Criteria:**
- [ ] 提供 `rust_plc geometry-export <source> --out <geometry.json>` CLI 子命令
- [ ] 工件至少包含 lanes、nodes、edges、summary
- [ ] 工件覆盖 topology、task/step、transition、resource/timing/causality
- [ ] Typecheck passes

### US-002: 叠加运行证据
**Description:** 作为调试者，我希望把 trace 叠加到静态几何上，这样我能看到哪些路径是“真实跑过”的。

**Acceptance Criteria:**
- [ ] `geometry-export` 支持 `--trace <trace.jsonl>`
- [ ] 输出工件包含 observed transition overlay
- [ ] overlay 至少保留 tick、task index、from/to step、reason
- [ ] Typecheck passes

### US-003: 叠加意图对齐结果
**Description:** 作为工艺或审查人员，我希望把 intent-alignment 结果叠加到几何工件上，这样我能分清“结构存在”和“证据对齐”是两回事。

**Acceptance Criteria:**
- [ ] `geometry-export` 支持 `--intent-report <report.json>`
- [ ] 输出工件包含 verdict、primary mismatch、warnings、blocker
- [ ] mismatch 数量进入 summary
- [ ] Typecheck passes

### US-004: 提供仓内架构文档
**Description:** 作为未来接手该能力的开发者，我希望仓内有一份稳定架构说明，说明这不是物理数字孪生，而是语义几何导出链。

**Acceptance Criteria:**
- [ ] 新增 architecture 文档说明 problem、non-goals、artifact model、phase rollout
- [ ] 文档明确 `IR -> evidence -> geometry artifact` 是唯一主链
- [ ] 文档包含至少一个可复制到 Markdown 的 ASCII 流程图
- [ ] 文档与当前 CLI MVP 方案一致

## Functional Requirements

1. FR-1: 系统必须提供 `geometry-export` CLI 子命令。
2. FR-2: `geometry-export` 必须消费 compile semantics，而不是前端手工模型。
3. FR-3: geometry artifact 必须包含稳定 schema version。
4. FR-4: geometry artifact 必须表达 `topology`、`task`、`step`、`transition`。
5. FR-5: geometry artifact 必须表达 `semantic_resources`、`resource_claims`、`timing`、`causality`。
6. FR-6: geometry artifact 必须支持可选 trace overlay。
7. FR-7: geometry artifact 必须支持可选 intent overlay。
8. FR-8: CLI 必须支持 `--output <human|json>`，便于机器与人工消费。
9. FR-9: geometry artifact 的节点和边必须标明 evidence status。

## Non-Goals

- 不做 3D 物理数字孪生
- 不做 CAD 机构建模
- 不做碰撞、受力、热、流体等物理求解
- 不在本期实现完整浏览器渲染器
- 不在本期实现实时动画编辑器

## Design Considerations

- 视觉语言应支持“星图 / 轨道 / 光晕 / 尾迹”的抽象审美，但这种审美必须建立在自动导出的稳定工件之上。
- artifact 先稳定，再讨论具体渲染器。
- 同一工件应能同时支持 Constellation / Orbit / Evidence 三类视图过滤。

## Technical Considerations

- 首选挂点是 `src/cli_support/plc_pipeline.rs` 产生的 compile semantics。
- trace overlay 首期消费 `src/trace_diff.rs` 的 `NormalizedTraceEvent`。
- intent overlay 首期消费 `src/intent_alignment/report.rs`。
- 未来 Web UI 应直接消费 geometry artifact，而不是重新实现 DSL 到图形的映射。

## Success Metrics

- 对同一个 PLC 项目，工程人员能在 30 秒内定位主 task、阻塞 step、主要 transition。
- 用户能在一个 artifact 中区分 authored / verified / observed / blocked。
- 后续 Web/SVG 渲染不需要重新解析 DSL。
- geometry artifact 可以稳定纳入自动化测试。

## Open Questions

- trace task index 到 runtime root task layout 的最终映射是否要在 MVP 后升级为强保证，而不是当前 best-effort。
- 后续是否要把 `trace-diff` 和 board trace 直接纳入同一 overlay。
- `component-sim` 的组件级 trace 是否需要成为 geometry artifact 的平行 evidence source。
