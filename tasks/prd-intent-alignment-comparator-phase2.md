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
- `src/intent_alignment/contract.rs`：`IntentContract`、`IntentMilestone`、`ContractMetadata`、`ExpectedBehaviorSpec`
- `src/intent_alignment/observed.rs`：`RawObservedEvent`、`ObservedMilestone`、`ObservedBehaviorSequence`、`ObservedEvidenceGap`
- `src/intent_alignment/compare.rs`：统一 conformance core、postcondition、readiness、cross-cycle 比较
- `src/intent_alignment/report.rs`：`IntentAlignmentReport`、`IntentAlignmentVerdict`、`IntentMismatchKind`、`IntentMismatch`
- `tests/intent_alignment_contract.rs`、`tests/intent_alignment_observed.rs`、`tests/intent_alignment_compare.rs`、`tests/intent_alignment_pipeline.rs`、`tests/intent_alignment_regress.rs`

比较主链必须固定为：

`authored intent contract -> compile_expected_behavior_spec -> extract_observed_behavior_sequence -> compare_intent_alignment -> IntentAlignmentReport`

## 4. User Stories

### US-001: 冻结 contract authoring governance 与基础 schema
**Description:** 作为意图合同维护者，我希望先冻结 contract 的 authoring governance、source binding 和基础 schema，这样后续实现面对的是有来源约束的合同，而不是任意手写 sidecar 真相。

**Implementation Anchor:** `src/intent_alignment/contract.rs`、`src/intent_alignment/mod.rs`、`tests/intent_alignment_contract.rs`、`tests/fixtures/intent_alignment/contracts/`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/contract.rs` 新增 `IntentContract`、`IntentMilestone`、`ContractMetadata` 或等价基础 schema
- [ ] phase-2 v1 只支持独立 fixture 文件加载 contract，不实现 Markdown 段落解析
- [ ] `IntentContract` 必须包含 `contract_version`、`source_ref`、`source_digest` 或等价 provenance/source binding 字段
- [ ] 必须最小冻结 contract authoring governance：至少明确 business owner、authoritative intent source，以及哪些输入可作为 contract 的业务评审依据
- [ ] `intent sequence` 节点必须被建模为业务里程碑，而不是 DSL `task.step` 名字镜像
- [ ] 至少一个 contract fixture 必须绑定到真实架构来源、现有 authored asset 或 canonical example，而不是纯手写演示夹具
- [ ] Typecheck passes
- [ ] Tests pass

### US-002: 实现 contract semantic validation 与稳定诊断
**Description:** 作为比较器实现者，我希望坏 contract 在进入 compile 前就被稳定拦截，这样后续不会对一份技术上合法但语义错误的合同继续推理。

**Implementation Anchor:** `src/intent_alignment/contract.rs`、`tests/intent_alignment_contract.rs`
**Dependency:** 依赖 `US-001`

**Acceptance Criteria:**
- [ ] 在 `src/intent_alignment/contract.rs` 或等价模块新增 `validate_intent_contract(...)`
- [ ] 必须稳定拦截冲突 `required edges`、不可达里程碑、互相矛盾的 cycle/restart 约束
- [ ] validation 失败必须返回稳定诊断，而不是让 compile 阶段兜底
- [ ] `tests/intent_alignment_contract.rs` 必须包含至少一个坏 contract 反例并断言稳定诊断
- [ ] Typecheck passes
- [ ] Tests pass

### US-003: 编译 ExpectedBehaviorSpec 与 IR 语义视图
**Description:** 作为比较器实现者，我希望 contract 能被编译成 `ExpectedBehaviorSpec`，并显式对齐现有 IR 语义视图，这样 intent-alignment 不会变成第二套语义中心。

**Implementation Anchor:** `src/intent_alignment/mod.rs`、`src/intent_alignment/contract.rs`、`tests/intent_alignment_contract.rs`
**Dependency:** 依赖 `US-001`、`US-002`

**Acceptance Criteria:**
- [ ] `src/intent_alignment/mod.rs` 导出 `compile_expected_behavior_spec(...)`
- [ ] `ExpectedBehaviorSpec` 必须显式表达 expected milestones、required edges、cycle semantics、restartability 语义
- [ ] `IntentContract` / `ExpectedBehaviorSpec` 的核心概念必须映射到现有 IR 级语义原语，或明确声明为 IR 语义视图
- [ ] `tests/intent_alignment_contract.rs` 必须断言 compile 后的核心语义与 contract/IR 视图一致
- [ ] Typecheck passes
- [ ] Tests pass

### US-004: 实现 trace adapter v1 到 observed evidence
**Description:** 作为比较器实现者，我希望先把现有 `trace.jsonl` 稳定适配成 observed evidence 输入，这样 observed 链路的第一步不是隐式脚本，而是可测 adapter。

**Implementation Anchor:** `src/intent_alignment/observed.rs`、`src/intent_alignment/mod.rs`、`tests/intent_alignment_observed.rs`

**Acceptance Criteria:**
- [ ] 在 `src/intent_alignment/observed.rs` 新增 `RawObservedEvent`、`ObservedEvidenceGap` 或等价结构
- [ ] 在 `src/intent_alignment/mod.rs` 导出 `trace.jsonl` / `NormalizedTraceEvent` 到 observed evidence 的 adapter 入口
- [ ] phase-2 v1 只支持从 `sim-plc` / `no-board-gate` 导出的 `trace.jsonl` 进入该 adapter
- [ ] 当输入事件不在已知 adapter 规则内时，必须返回结构化 gap，不能静默映射
- [ ] `tests/intent_alignment_observed.rs` 必须覆盖最小正例和未知事件反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-005: 冻结 observable vocabulary、normalization 与证据门槛
**Description:** 作为比较器实现者，我希望 observed 证据先被归并成稳定语义单元，并为四个比较维度定义最低证据门槛，这样 extractor 不会凭残缺证据继续凑合比较。

**Implementation Anchor:** `src/intent_alignment/observed.rs`、`tests/intent_alignment_observed.rs`
**Dependency:** 依赖 `US-004`

**Acceptance Criteria:**
- [ ] 必须定义显式 observable vocabulary 或 event-to-milestone mapping contract
- [ ] 必须冻结 observation normalization 规则，至少覆盖去重、抖动折叠、pending 到 terminal 合并、同周期重复观测的归并口径
- [ ] 必须定义 required-step、ordering、postcondition、cross-cycle 四个维度的最低证据门槛
- [ ] 证据不足时必须返回结构化 gap/blocking 结论，而不是继续产出部分 aligned/mismatch
- [ ] `tests/intent_alignment_observed.rs` 必须包含重复上报、抖动或 pending 轮询不会膨胀成多个 milestone 的反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-006: 实现统一 path-conformance core
**Description:** 作为意图对齐验证者，我希望先有一个统一的 `path-conformance / graph-matching core`，这样 required-step、ordering 和 extra-behavior 不是多个临时 pass，而是同一个核心机制的不同视图。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-003`、`US-005`

**Acceptance Criteria:**
- [ ] 在 `src/intent_alignment/compare.rs` 新增统一 conformance core，而不是一组互不相干的 compare pass
- [ ] 该 core 必须覆盖 required steps、partial-order edges、allowed multiplicity、re-entry 与 extra-behavior 的内部判定
- [ ] `tests/intent_alignment_compare.rs` 必须证明合法重入不会误报，非法绕路不会被静默忽略
- [ ] Typecheck passes
- [ ] Tests pass

### US-007: 投影 mismatch taxonomy 并冻结主诊断规则
**Description:** 作为意图对齐验证者，我希望统一 conformance 结果能被稳定投影为固定 mismatch taxonomy，这样同一最小反例不会随着实现细节漂移出不同主诊断。

**Implementation Anchor:** `src/intent_alignment/report.rs`、`src/intent_alignment/compare.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-006`

**Acceptance Criteria:**
- [ ] 在 `src/intent_alignment/report.rs` 新增 `IntentMismatchKind`，至少包含 `missing_required_step`、`wrong_order`、`duplicated_required_step`、`premature_readiness`、`postcondition_not_met`、`cross_cycle_drift`
- [ ] 必须冻结 canonical mismatch prioritization / normalization 规则，确保同一最小反例产出稳定主诊断和稳定关联节点
- [ ] 对 contract 未声明允许的 unexpected milestone、forbidden edge 或 illegal re-entry，必须产生 non-aligned 结果而不是静默忽略
- [ ] `tests/intent_alignment_compare.rs` 必须断言同一最小反例在不同入口下得到相同主诊断
- [ ] Typecheck passes
- [ ] Tests pass

### US-008: 实现终态 postcondition 语义
**Description:** 作为意图对齐验证者，我希望先稳定终态 postcondition 判定，这样业务未完成不会只靠 ready 或 terminal step 被误判为完成。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-003`、`US-005`

**Acceptance Criteria:**
- [ ] 必须冻结一个小而闭合的 `PredicateExpr` / `ObservedFact` 代数
- [ ] `evaluate_postconditions(...)` 必须消费 `ExpectedBehaviorSpec` 中的终态 postconditions，而不是直接对 trace 做字符串判断
- [ ] `extract_observed_behavior_sequence(...)` 或等价 helper 必须提供 cycle-end snapshot 或 terminal milestone
- [ ] `tests/intent_alignment_compare.rs` 必须包含 `postcondition_not_met` 的正例与反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-009: 实现过程型恢复义务与 premature_readiness
**Description:** 作为意图对齐验证者，我希望恢复过程的历史 witness 与 readiness 语义被单独检查，这样不会把终态正确但恢复路径错误的 case 放过去。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-008`

**Acceptance Criteria:**
- [ ] readiness / restartability 必须作为 `ExpectedBehaviorSpec` 的一等状态语义建模
- [ ] 至少一类 postcondition 或 recovery obligation 必须能够消费历史 witness / 过程事实，而不是只看 cycle-end snapshot
- [ ] 必须明确 `postcondition_not_met` 与 `premature_readiness` 的判定边界、优先级或归约关系
- [ ] `tests/intent_alignment_compare.rs` 必须包含终态相同但恢复路径错误仍然 fail 的反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-010: 冻结 cycle semantics 与 inter-cycle handoff
**Description:** 作为意图对齐验证者，我希望 cycle 语义和 handoff invariant 先被显式冻结，这样跨周期比较不是靠启发式切片。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`src/intent_alignment/observed.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-003`、`US-005`

**Acceptance Criteria:**
- [ ] `ExpectedBehaviorSpec` 必须显式建模 `cycle_start`、`successful_cycle_end`、`aborted_cycle_end`、`restart_condition`
- [ ] phase-2 v1 的 cycle boundary 必须由 contract 语义驱动并由 extractor 写入 `cycle_index`
- [ ] `ExpectedBehaviorSpec` 必须显式表达 inter-cycle handoff invariant：`cycle_n` 的 terminal facts 如何约束 `cycle_n+1` 的 start facts
- [ ] `tests/intent_alignment_compare.rs` 必须包含单周期内重复 start-like milestone 但不应被切成两个周期的反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-011: 实现 cross-cycle conformance 与 drift 诊断
**Description:** 作为意图对齐验证者，我希望跨周期关系进入核心 conformance 模型，并只在确有独立判定价值时输出 `cross_cycle_drift`。

**Implementation Anchor:** `src/intent_alignment/compare.rs`、`tests/intent_alignment_compare.rs`
**Dependency:** 依赖 `US-007`、`US-010`

**Acceptance Criteria:**
- [ ] cross-cycle conformance 必须进入 `compare_intent_alignment(...)` 的核心语义，而不是单周期比较后的尾部补丁检查
- [ ] `cross_cycle_drift` 必须具备独有判定面；若失败已被单周期 mismatch 充分解释，则只能作为 cross-cycle context
- [ ] `detect_cross_cycle_drift(...)` 必须消费至少两个连续周期的 `ObservedBehaviorSequence`
- [ ] `tests/intent_alignment_compare.rs` 必须包含第一周期残留风险带入第二周期而触发 drift 的反例
- [ ] Typecheck passes
- [ ] Tests pass

### US-012: 实现 report contract、verdict lattice 与 diagnostics 对齐
**Description:** 作为比较器消费者，我希望库级 report 与 verdict 先稳定并对齐现有 diagnostics/self-check 模型，这样 pipeline 不会在最后一公里改写结论。

**Implementation Anchor:** `src/intent_alignment/report.rs`、`src/intent_alignment/mod.rs`、`src/lib.rs`、`tests/intent_alignment_pipeline.rs`、`tests/self_check.rs`
**Dependency:** 依赖 `US-007`、`US-009`、`US-011`

**Acceptance Criteria:**
- [ ] 在 `src/intent_alignment/report.rs` 新增 `IntentAlignmentReport` 与 `IntentMismatch`
- [ ] `IntentAlignmentReport` 必须包含 contract identity、evidence identity、comparator/rule version、cycle window 或等价 provenance 字段
- [ ] 必须定义 `IntentAlignmentVerdict` 或等价 verdict lattice，并明确 mismatch、gap、toolchain limitation、warning 并存时的 reduction policy
- [ ] library-level verdict 必须是 source of truth；intent-alignment 必须复用或扩展现有统一 diagnostics / self-check 聚合模型
- [ ] Typecheck passes
- [ ] Tests pass

### US-013: 接入 project-check 并锁死跨入口一致性
**Description:** 作为比较器消费者，我希望 library report 接入 `project-check` 后仍保持跨入口一致的最终 verdict，这样 CLI/pipeline 不会重新解释 comparator 结果。

**Implementation Anchor:** `src/cli/utilities.rs`、`src/cli_support/plc_pipeline.rs`、`tests/intent_alignment_pipeline.rs`、`tests/self_check.rs`
**Dependency:** 依赖 `US-012`

**Acceptance Criteria:**
- [ ] phase-2 v1 不新增公开 CLI；先把结果接入 `project-check` 或等价聚合 pipeline
- [ ] pipeline 只能做无信息损失的确定性归约，不能再解释或改写 comparator 结论
- [ ] `tests/intent_alignment_pipeline.rs` 必须断言严重 mismatch 不会被 pipeline 降成 warning 或 blocker 展示
- [ ] `tests/intent_alignment_pipeline.rs` 或等价入口测试必须断言同一 library report 在所有入口上得到相同最终 verdict
- [ ] Typecheck passes
- [ ] Tests pass

### US-014: 收口 canonical 与 mutation 回归集
**Description:** 作为仓库维护者，我希望先把每个已冻结 mismatch 的 canonical 与 mutation 回归集收口，这样 phase-2 的基础回归边界是明确的。

**Implementation Anchor:** `tests/intent_alignment_compare.rs`、`tests/intent_alignment_regress.rs`、`tests/fixtures/intent_alignment/`
**Dependency:** 依赖 `US-013`

**Acceptance Criteria:**
- [ ] 至少新增一组 canonical intent-alignment fixtures，覆盖双气缸顺序、单气缸 recovery、多机构 recovery、跨周期 drift
- [ ] 每个已冻结 mismatch 至少新增 1 条 canonical 和 1 条最小 mutation 反例
- [ ] 每个 canonical / mutation 回归都必须绑定到显式语义断言、FR 或 mismatch 规则
- [ ] Typecheck passes
- [ ] Tests pass

### US-015: 加入真实 golden path 并关闭 phase-2
**Description:** 作为仓库维护者，我希望最后再加入真实 golden path，并明确 phase-2 的固定关闭集，这样这一阶段可以客观收口，而不是无限扩张。

**Implementation Anchor:** `tests/intent_alignment_pipeline.rs`、`tests/examples_integration.rs`、`tests/intent_alignment_regress.rs`、`tests/fixtures/intent_alignment/`
**Dependency:** 依赖 `US-014`

**Acceptance Criteria:**
- [ ] 至少新增 1 条真实 golden path：从现有 `.plc` 示例或等价真实 authored asset 产出 toolchain evidence，再进入 extractor -> comparator -> report
- [ ] golden path 必须绑定到显式语义断言、FR 或 mismatch 规则，而不是只断言流程跑通
- [ ] 文档中引用的 canonical / golden 例子路径与测试夹具保持一致
- [ ] phase-2 的固定关闭集必须在文档中被明确列出，超出关闭集的新回归一律后置
- [ ] Typecheck passes
- [ ] Tests pass

## 5. Functional Requirements

- FR-1: 系统必须支持一份 authored intent contract，至少表达 `intent sequence`、可观察里程碑、required ordering、postconditions、next-cycle start conditions。
- FR-2: authored intent contract 中的节点语义必须是业务里程碑，而不是 DSL `task.step` 名称的简单镜像。
- FR-3: 系统必须先将 authored intent contract 编译为 `ExpectedBehaviorSpec` 或等价代码结构，再进行比较。
- FR-3.1: authored intent contract 必须携带 provenance/source binding，至少能追溯到真实架构来源、现有 authored asset 或 canonical source。
- FR-3.2: authored intent contract 必须经过 semantic validation；坏合同不能被编译成 `ExpectedBehaviorSpec` 后再进入 comparator。
- FR-3.3: intent-alignment 不能创建第二套独立于 IR 的语义中心；`IntentContract` / `ExpectedBehaviorSpec` 必须映射到现有 IR 级原语或被明确声明为 IR 语义视图。
- FR-4: `ExpectedBehaviorSpec` 必须显式表达 expected milestones、required edges、cycle end checks，不能把规则留给 agent 或 skill 的自然语言推断。
- FR-4.1: `ExpectedBehaviorSpec` 不能只表达裸线性序列；必须支持 allowed multiplicity、partial-order edges，以及 recovery/re-entry 的可接受路径。
- FR-5: 系统必须能从真实 trace、runtime context 或 validation 证据中抽取 `ObservedBehaviorSequence`。
- FR-6: `ObservedBehaviorSequence` 抽取结果必须保留证据来源和周期边界信息。
- FR-6.1: phase-2 v1 的 observed 输入先固定为 `trace.jsonl` 或其解析后的 `NormalizedTraceEvent`；其他证据适配后置。
- FR-6.2: 系统必须定义稳定的 observable vocabulary / event-to-milestone mapping；未知事件只能产生 gap，不能静默映射成 milestone。
- FR-6.3: observation normalization 必须定义去重、抖动折叠、pending 到 terminal 合并、同周期重复观测的稳定归并规则。
- FR-6.4: stable semantic evidence interface 必须与 trace adapter 分离；trace.jsonl 只是 observed evidence 的一种适配来源。
- FR-6.5: 系统必须为 required-step、ordering、postcondition、cross-cycle 四个维度定义最低证据门槛；证据不足时必须阻断对应维度比较并返回稳定 gap/blocking 结论。
- FR-7: comparator 必须先检查 required-step coverage，再检查 ordering conformance。
- FR-8: comparator 必须支持 `missing_required_step`、`wrong_order`、`duplicated_required_step` 三类基础 mismatch。
- FR-8.1: comparator 不能静默忽略 unexpected milestone、forbidden edge 或 illegal re-entry；未声明允许的额外行为必须导致 non-aligned verdict。
- FR-8.2: required-step、ordering、extra-behavior 判定在内部应由统一的 path-conformance / graph-matching 机制产生，mismatch taxonomy 只是报告视图。
- FR-8.3: 同一最小反例必须经过 canonical mismatch prioritization / normalization 后产出稳定主诊断与稳定关联节点。
- FR-9: comparator 必须支持 `postcondition_not_met` 和 `premature_readiness`。
- FR-9.1: postcondition 判定必须建立在闭合的 `PredicateExpr` / `ObservedFact` 代数上，而不是靠按案例增加专用谓词名字。
- FR-9.2: 至少一类 postcondition 或 recovery obligation 必须消费历史 witness / 过程事实，而不是只看终态 snapshot。
- FR-9.3: readiness / restartability 必须是 `ExpectedBehaviorSpec` 的一等状态语义，而不是 comparator 的临时推断。
- FR-9.4: `postcondition_not_met` 与 `premature_readiness` 的判定边界和归约关系必须稳定冻结，避免同一 failure 双重归类或漂移归类。
- FR-10: comparator 必须支持 `cross_cycle_drift`，且比较范围不能只限于单周期。
- FR-10.1: cycle boundary 必须由 contract 语义显式驱动，至少建模 `cycle_start`、`successful_cycle_end`、`aborted_cycle_end`、`restart_condition`，不能只靠 start-like milestone 重复出现启发式推断。
- FR-10.2: 系统必须显式验证 inter-cycle handoff invariant，即 `cycle_n` 的 terminal facts 与 `cycle_n+1` 的 start facts 的兼容性。
- FR-10.3: cross-cycle conformance 必须进入核心比较模型，不能只是单周期比较后的尾部补丁检查。
- FR-10.4: `cross_cycle_drift` 必须具备独立于单周期 mismatch 的判定价值；若失败已被单周期 mismatch 充分解释，则应作为 cross-cycle context 而非重复主诊断。
- FR-11: 当证据不足以执行比较时，系统必须返回结构化缺口，而不是默认判定 aligned。
- FR-12: 当 comparator 已成功运行时，系统必须输出稳定 `IntentAlignmentReport`，而不是只输出 blocker。
- FR-13: `IntentAlignmentReport` 必须包含 mismatch 类型、关联的 intent 节点、关联的 behavior 证据和最终判定。
- FR-13.1: `IntentAlignmentReport` 必须包含 contract identity、evidence identity、comparator/rule version、cycle window 或等价 provenance 字段。
- FR-13.2: pipeline 必须基于固定的 verdict lattice / reduction policy 将 report 归约成 aligned、mismatch、blocked 或等价最终结论，且不能降级严重 mismatch。
- FR-13.3: intent-alignment 的报告和聚合必须复用或扩展现有统一 diagnostics / self-check 模型，而不是并行再造一套私有结果系统。
- FR-13.4: library-level verdict 是 source of truth；所有 CLI / pipeline 入口只能做无信息损失的确定性归约，并保持跨入口结论一致。
- FR-14: skill 不能直接凭自然语言判断 trace 是否满足意图；所有 aligned/mismatch 结论必须来自 comparator 函数返回值。
- FR-15: 只有 required-step、ordering、postcondition、next-cycle 四维全部通过时，系统才允许输出 aligned。
- FR-15.1: phase-2 v1 的 cycle 边界必须由 contract 声明的 cycle-start milestone 切分，并在 observed 序列中显式写入 `cycle_index`。
- FR-16: canonical examples 必须进入仓库回归测试，而不是只停留在文档示意。
- FR-16.1: canonical 回归之外，至少必须有 1 条从真实 `.plc` 或等价真实 authored asset 产出 evidence 的 golden path。
- FR-16.2: 每个 canonical 例子至少应有 1 个最小 mutation 反例，避免 comparator 对固定样例过拟合。
- FR-16.3: regression 只能证明显式语义断言、FR 或 mismatch 规则；回归夹具本身不能反过来成为新的语义来源。
- FR-16.4: phase-2 必须定义固定关闭集；达到该关闭集即允许本阶段收口，新增回归需求自动后置到下一阶段而非无限扩张。

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
- 如果某个 story 实际包含多个执行切片，而单次迭代无法完整交付全部切片，则必须继续拆 story；不能用“先完成一半”冒充 Ralph 可执行粒度。

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
