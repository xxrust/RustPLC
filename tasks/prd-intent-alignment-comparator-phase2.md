# PRD: 意图对齐比较器二阶段

## 1. 简介

本阶段要补的是 RustPLC 真正的“意图对齐验证”能力，而不是继续补 skill 侧边界说明。

当前仓库已经把以下边界澄清清楚：

- `intent sequence` / `expected-path` 属于作者化合同
- `trace` / `runtime` / `validation report` 属于真实工具链证据
- 没有稳定 comparator 时，agent 必须停在 blocker，而不能伪造“已对齐”

但这还不等于已经具备意图对齐验证能力。原始架构文档要求的核心能力是：

1. 把工艺意图写成可比较的 `intent sequence`
2. 把真实 trace / runtime 证据抽取成 `behavior sequence`
3. 在固定规则下比较 required steps、ordering、postconditions、next-cycle
4. 输出稳定的 mismatch 类型，而不是只输出“blocked”

本 PRD 的目标就是把这套能力补齐成 Ralph 可逐步实施的范围。

## 2. Goals

- 交付一套可执行的意图对齐比较流程：作者化意图合同 -> 可观察里程碑 -> 行为证据抽取 -> comparator -> mismatch 报告。
- 让系统能够输出稳定的 mismatch 类型，而不是只停留在“证据缺失”口径。
- 把 `required-step`、`ordering`、`postcondition`、`next-cycle drift` 四类判定冻结为仓库内可测试的实现契约。
- 让 cylinder 顺序、recovery、跨周期漂移这类架构文档中的 canonical 例子进入回归测试。
- 保持和已有 formal verification、runtime、trace 体系分层一致，不把业务意图问题下沉为 runtime 特判。
- 让 Ralph 在每个 story 内都能直接定位要改的 Rust 模块、函数和测试文件，而不是再从说明文档反推实现边界。

## 3. 实现骨架

本阶段默认按下列 Rust 模块骨架实施；如果实现时发现需要微调文件名，必须保持职责不变。

- `src/intent_alignment/mod.rs`：对外导出 `load_intent_contract(...)`、`compile_expected_behavior_spec(...)`、`extract_observed_behavior_sequence(...)`、`compare_intent_alignment(...)`
- `src/intent_alignment/contract.rs`：`IntentContract`、`IntentMilestone`、`PostconditionSpec`、`RestartConditionSpec`、`ExpectedBehaviorSpec`
- `src/intent_alignment/observed.rs`：`ObservedMilestone`、`ObservedBehaviorSequence`、`ObservedEvidenceGap`、`extract_observed_behavior_sequence(...)`
- `src/intent_alignment/compare.rs`：`compare_required_steps(...)`、`compare_ordering(...)`、`evaluate_postconditions(...)`、`detect_premature_readiness(...)`、`detect_cross_cycle_drift(...)`、`compare_intent_alignment(...)`
- `src/intent_alignment/report.rs`：`IntentAlignmentReport`、`IntentMismatchKind`、`IntentMismatch`
- `tests/intent_alignment_contract.rs`、`tests/intent_alignment_observed.rs`、`tests/intent_alignment_compare.rs`、`tests/intent_alignment_pipeline.rs`：最小契约与 canonical regressions

比较主链必须固定为：

`authored intent contract -> compile_expected_behavior_spec -> extract_observed_behavior_sequence -> compare_intent_alignment -> IntentAlignmentReport`

## 4. User Stories

### US-001: 落地 authored intent contract 与 expected behavior spec 编译器
**Description:** 作为比较器实现者，我希望 authored intent contract 先被落成仓库内可加载的数据模型，并能编译成 expected behavior spec，这样后续比较面对的是形式化结构，而不是继续围绕说明文档打转。

**Implementation Anchor:** `src/intent_alignment/mod.rs`、`src/intent_alignment/contract.rs`、`tests/intent_alignment_contract.rs`、`tests/fixtures/intent_alignment/contracts/`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/contract.rs` 新增 `IntentContract`、`IntentMilestone`、`PostconditionSpec`、`RestartConditionSpec`、`ExpectedBehaviorSpec`
- [ ] `src/intent_alignment/mod.rs` 导出 `load_intent_contract(...)` 与 `compile_expected_behavior_spec(...)`
- [ ] phase-2 v1 只支持独立 fixture 文件加载 contract，不实现 Markdown 段落解析
- [ ] `IntentContract` 或 `IntentMilestone` 必须显式携带“意图节点 -> 可观测里程碑”映射规则，不能把这一步留给比较器临时猜测
- [ ] `compile_expected_behavior_spec(...)` 必须把作者化合同编译为显式 expected milestones、required edges、cycle end checks；不能把比较逻辑留给后续自然语言解释
- [ ] `intent sequence` 节点在数据模型中被明确建模为业务里程碑，而不是 DSL `task.step` 名字镜像
- [ ] 至少新增一个顺序案例 fixture 和一个 recovery 案例 fixture，并由 `tests/intent_alignment_contract.rs` 直接加载
- [ ] Typecheck passes
- [ ] Tests pass

### US-002: 实现行为证据到 behavior sequence 的抽取层
**Description:** 作为比较器实现者，我希望真实 trace、runtime context 或 validation 产物能先被归一化为 behavior sequence，这样后续比较不再依赖人工脑补。

**Implementation Anchor:** `src/intent_alignment/mod.rs`、`src/intent_alignment/observed.rs`、`tests/intent_alignment_observed.rs`、`tests/fixtures/intent_alignment/observed/`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/observed.rs` 新增 `ObservedMilestone`、`ObservedBehaviorSequence`、`ObservedEvidenceGap`
- [ ] `src/intent_alignment/mod.rs` 导出 `extract_observed_behavior_sequence(...)`
- [ ] phase-2 v1 只支持从 `sim-plc` / `no-board-gate` 导出的 `trace.jsonl` 抽取 behavior sequence；validation report 和其他 artifact 适配留到后续
- [ ] `extract_observed_behavior_sequence(...)` 至少能消费 `trace_diff::NormalizedTraceEvent`，并输出里程碑名、发生顺序、证据来源、周期边界信息
- [ ] 当证据不足以生成 behavior sequence 时，返回结构化 `ObservedEvidenceGap`，而不是默认通过
- [ ] `tests/intent_alignment_observed.rs` 至少覆盖一个最小正例和一个证据缺失反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-003: 实现 required-step 与 ordering comparator
**Description:** 作为意图对齐验证者，我希望系统能先比较必经步骤覆盖和必经顺序，这样最核心的 `A -> B -> C` 对 `A -> C` 错误可以被稳定识别。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`src/intent_alignment/report.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-001`、`US-002`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/report.rs` 新增 `IntentMismatchKind::{missing_required_step, wrong_order, duplicated_required_step, ...}` 对应的 Rust 枚举变体
- [ ] `src/intent_alignment/compare.rs` 新增 `compare_required_steps(...)` 与 `compare_ordering(...)`
- [ ] `compare_intent_alignment(...)` 的执行顺序固定为 `compare_required_steps(...) -> compare_ordering(...) -> evaluate_postconditions(...) -> detect_cross_cycle_drift(...)`
- [ ] `compare_required_steps(...)` 与 `compare_ordering(...)` 必须产出 `IntentMismatch` 或等价 typed mismatch 结构，不能只返回文本说明
- [ ] comparator 能输出 `missing_required_step`、`wrong_order`、`duplicated_required_step`
- [ ] `tests/intent_alignment_compare.rs` 为顺序正确、缺步骤、错顺序、重复步骤分别添加测试
- [ ] Typecheck passes
- [ ] Tests pass

### US-004: 实现 postcondition 与 premature_readiness 判定
**Description:** 作为意图对齐验证者，我希望系统能验证周期结束时业务是否真的完成，而不是只看程序是否回到了某个等待点。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`src/intent_alignment/report.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-001`、`US-002`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/compare.rs` 新增 `evaluate_postconditions(...)` 与 `detect_premature_readiness(...)`
- [ ] phase-2 v1 的 postconditions 先用显式命名谓词集合或等价结构表示，不依赖自由文本解释
- [ ] `evaluate_postconditions(...)` 必须消费 `ExpectedBehaviorSpec` 中的 postconditions，而不是直接对 trace 做字符串判断
- [ ] `extract_observed_behavior_sequence(...)` 或等价 helper 必须提供 cycle-end snapshot 或 terminal milestone，供 postcondition 判定消费
- [ ] comparator 能基于周期结束证据输出 `postcondition_not_met`
- [ ] 当程序进入 `ready` 或等价状态但业务前提未恢复时，输出 `premature_readiness`
- [ ] `tests/intent_alignment_compare.rs` 中 recovery 场景至少覆盖单气缸和多机构示例中的一个正例与一个反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-005: 实现 next-cycle drift 判定
**Description:** 作为意图对齐验证者，我希望系统能检查下一周期是否从正确起点重新开始，这样“第一轮正常、第二轮漂移”的问题不会漏掉。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`src/intent_alignment/report.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-001`、`US-002`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/compare.rs` 新增 `detect_cross_cycle_drift(...)`
- [ ] `detect_cross_cycle_drift(...)` 必须消费至少两个连续周期的 `ObservedBehaviorSequence`
- [ ] phase-2 v1 的 cycle 边界规则固定为“按 contract 声明的 cycle-start milestone 重复出现切分周期，并由 extractor 写入 `cycle_index`”；comparator 不得从 `ready` 反推边界
- [ ] 当第二周期跳步、从中间状态继续、或重复上一周期尾部动作时，输出 `cross_cycle_drift`
- [ ] cycle 边界的判定规则必须体现在代码注释或 helper 函数中，不能留给人工口头解释
- [ ] `tests/intent_alignment_compare.rs` 至少添加一个“第一轮通过、第二轮失败”的回归测试
- [ ] Typecheck passes
- [ ] Tests pass

### US-006: 实现 mismatch report 数据结构与 pipeline 接入
**Description:** 作为比较器消费者，我希望 comparator 输出先以稳定数据结构接入 pipeline 或 API，这样后续 CLI 和 skill 只是消费结果，而不是再次主导设计。

**Implementation Anchor:** `src/intent_alignment/mod.rs`、`src/intent_alignment/report.rs`、`src/lib.rs`、`src/cli/utilities.rs`、`src/cli_support/plc_pipeline.rs`、`tests/intent_alignment_pipeline.rs`、`tests/self_check.rs`
**Dependency:** 依赖 `US-001` 至 `US-005`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/report.rs` 新增 `IntentAlignmentReport` 与 `IntentMismatch`
- [ ] `IntentAlignmentReport` 至少包含：场景、intent 节点、behavior 节点、mismatch 类型、证据来源、结论
- [ ] `src/intent_alignment/mod.rs` 提供单一入口 `compare_intent_alignment(...)` 并由 `src/lib.rs` 导出模块
- [ ] phase-2 v1 不新增公开 CLI；先把结果接入 `project-check` 或等价聚合 pipeline
- [ ] 当 comparator 已执行完成时，`compare_intent_alignment(...)` 返回稳定 mismatch 类型，而不是把所有失败折叠成 blocker
- [ ] 当 comparator 不能执行时，返回 `missing evidence` / `missing comparator` / `toolchain limitation` 边界口径；该分支与 mismatch 分支必须在数据结构上可区分
- [ ] `tests/intent_alignment_pipeline.rs` 覆盖“比较成功发现 mismatch”和“比较器无法执行返回 blocker”两个路径，并同步更新 `tests/self_check.rs` 中相关聚合断言
- [ ] Typecheck passes
- [ ] Tests pass

### US-007: 加入 canonical examples 与端到端回归
**Description:** 作为仓库维护者，我希望架构文档中的 cylinder 与 recovery 例子进入真正的 fixture 和回归，这样后续实现不会再次漂回只写文档不落地。

**Implementation Anchor:** `tests/intent_alignment_compare.rs`、`tests/intent_alignment_pipeline.rs`、`tests/intent_alignment_regress.rs`、`tests/fixtures/intent_alignment/*`

**Acceptance Criteria:**
- [ ] 至少新增一组 canonical intent-alignment fixtures，覆盖双气缸顺序、单气缸 recovery、多机构 recovery、跨周期 drift
- [ ] `tests/intent_alignment_compare.rs` 中至少一个回归直接对应原架构文档中的 `A -> B` 顺序例子
- [ ] `tests/intent_alignment_compare.rs` 或 `tests/intent_alignment_pipeline.rs` 中至少一个回归直接对应 `fault_detected -> safe_home_restored -> cycle_restartable` 例子
- [ ] canonical authored contracts 与 observed traces 默认放在 `tests/fixtures/intent_alignment/`；只有涉及现有 `.plc` 示例时才补 `tests/examples_integration.rs`
- [ ] 文档中引用的 canonical 例子路径与测试夹具保持一致
- [ ] Typecheck passes
- [ ] Tests pass

## 5. Functional Requirements

- FR-1: 系统必须支持一份 authored intent contract，至少表达 `intent sequence`、可观察里程碑、required ordering、postconditions、next-cycle start conditions。
- FR-2: authored intent contract 中的节点语义必须是业务里程碑，而不是 DSL `task.step` 名称的简单镜像。
- FR-3: 系统必须先将 authored intent contract 编译为 `ExpectedBehaviorSpec` 或等价代码结构，再进行比较。
- FR-4: `ExpectedBehaviorSpec` 必须显式表达 expected milestones、required edges、cycle end checks，不能把规则留给 agent 或 skill 的自然语言推断。
- FR-5: 系统必须能从真实 trace、runtime context 或 validation 证据中抽取 `ObservedBehaviorSequence`。
- FR-6: `ObservedBehaviorSequence` 抽取结果必须保留证据来源和周期边界信息。
- FR-6.1: phase-2 v1 的 observed 输入先固定为 `trace.jsonl` 或其解析后的 `NormalizedTraceEvent`；其他证据适配后置。
- FR-7: comparator 必须先检查 required-step coverage，再检查 ordering conformance。
- FR-8: comparator 必须支持 `missing_required_step`、`wrong_order`、`duplicated_required_step` 三类基础 mismatch。
- FR-9: comparator 必须支持 `postcondition_not_met` 和 `premature_readiness`。
- FR-10: comparator 必须支持 `cross_cycle_drift`，且比较范围不能只限于单周期。
- FR-11: 当证据不足以执行比较时，系统必须返回结构化缺口，而不是默认判定 aligned。
- FR-12: 当 comparator 已成功运行时，系统必须输出稳定 `IntentAlignmentReport`，而不是只输出 blocker。
- FR-13: `IntentAlignmentReport` 必须包含 mismatch 类型、关联的 intent 节点、关联的 behavior 证据和最终判定。
- FR-14: skill 不能直接凭自然语言判断 trace 是否满足意图；所有 aligned/mismatch 结论必须来自 comparator 函数返回值。
- FR-15: 只有 required-step、ordering、postcondition、next-cycle 四维全部通过时，系统才允许输出 aligned。
- FR-15.1: phase-2 v1 的 cycle 边界必须由 contract 声明的 cycle-start milestone 切分，并在 observed 序列中显式写入 `cycle_index`。
- FR-16: canonical examples 必须进入仓库回归测试，而不是只停留在文档示意。

## 6. Non-Goals

- 不在本阶段实现全状态空间穷举或新的 formal verification 引擎。
- 不在本阶段引入自然语言自动生成 `intent sequence` 的能力。
- 不把现有 runtime/fault routing 逻辑重写成 comparator。
- 不默认新增一个公开的 `expected-path` CLI，除非后续有明确实现必要性。
- 不解决所有 scenario toolchain 兼容性问题；这类问题仍可作为 blocker 保留。
- 不允许用“更新 skill 文档”替代 comparator、report、tests 的真实代码实现。

## 7. Design Considerations

- authored intent contract 应优先保持“先意图链、再观测规则”的结构，避免退回“先 DSL step、再猜业务意图”。
- mismatch 输出必须对审计友好，能让人直接看见“缺了哪个 required step”“哪条 ordering 被打破”“哪个 postcondition 没满足”。
- worked examples 不应只是说明文字，最好直接指向 canonical fixtures。
- 如果某个 story 只改文档、不新增或修改上述 Rust 模块、函数、测试之一，则该 story 不算完成。

## 8. Technical Considerations

- 现有架构基线在 [intent_alignment_verification.md](/E:/personal_project/rust_plc/docs/architecture/intent_alignment_verification.md) 中已经冻结，后续实现必须与该文档的四维判定一致。
- 现有 `plc-gen` skill 文档已经把 authored assets、toolchain evidence、optional commands、blocker 口径写清楚；第二阶段不能重复发明另一套术语。
- `behavior sequence` 的抽取层应尽量复用已有 `sim-plc` trace、runtime evidence、validation report，而不是要求人工手写 behavior。
- comparator 的输出需要和现有 skill output contract 协同：比较器未运行时报 blocker，比较器已运行时报 mismatch。
- 需要特别关注 cycle 边界的定义，否则 `cross_cycle_drift` 很容易退化成不稳定启发式。
- 现有 `src/trace_diff.rs` 已经证明“规范化输入 -> comparator -> 结构化 report”模式可行；意图对齐能力应复用这一模式，而不是另起一套口头判断流。
- 若实现需要新增模块导出，必须同步更新 `src/lib.rs`。

## 9. Success Metrics

- 至少能稳定识别四类核心失败：缺步骤、错顺序、后置条件不满足、跨周期漂移。
- 至少两个来自架构文档的 canonical 例子能变成自动化回归并稳定通过。
- `plc-gen` 在 comparator 已存在时，不再把所有失败都收口成 blocker。
- 团队可以用同一份 authored contract + 同一份 trace 复现同一份 mismatch report，不依赖人工解释。
- Ralph 在执行单个 story 时，不需要额外查阅新的口头说明就能定位要改的模块、函数和测试文件。

## 10. Open Questions

- authored intent contract 的 fixture 先选 JSON 还是 YAML；如果不影响 story 边界，优先选实现成本更低的一种。
- mismatch report 最终挂到哪个 CLI 最合适可以后置，但 phase-2 v1 先接 `project-check` 聚合，库级入口 `compare_intent_alignment(...)` 必须先稳定。
