# PRD: `plc-gen` 的意图对齐 / expected-path 边界与工具链分工

## 1. Introduction / Overview

当前仓库已经有三类相邻但不同的材料：

- [intent_alignment_verification.md](/E:/personal_project/rust_plc/docs/architecture/intent_alignment_verification.md) 定义“判什么叫意图不对齐”
- [expected-path-simulation.md](/E:/personal_project/rust_plc/docs/architecture/expected-path-simulation.md) 定义“如何用小资产 + trace 比对覆盖关键路径，并控制组合爆炸”
- [深度解析：PLC程序形式化驗證中的狀態空間爆炸與人類認知策略.md](/E:/personal_project/rust_plc/docs/%E6%B7%B1%E5%BA%A6%E8%A7%A3%E6%9E%90%EF%BC%9APLC%E7%A8%8B%E5%BA%8F%E5%BD%A2%E5%BC%8F%E5%8C%96%E9%A9%97%E8%AD%89%E4%B8%AD%E7%9A%84%E7%8B%80%E6%85%8B%E7%A9%BA%E9%96%93%E7%88%86%E7%82%B8%E8%88%87%E4%BA%BA%E9%A1%9E%E8%AA%8D%E7%9F%A5%E7%AD%96%E7%95%A5.md) 解释“为什么不能把意图对齐做成全状态枚举”

问题不在于这些文档彼此冲突，而在于 `plc-gen` 还没有把它们消化成清晰的三层分工：

1. 哪些是要写进 skill 的稳定知识和工作流规则
2. 哪些是 RustPLC 编译链 / 场景工具链真实产出的证据或工件
3. 哪些命令只是 skill 在特定任务下可选择调用，而不是默认主路径

如果这个边界不冻结，`plc-gen` 很容易犯三类错误：

- 把“意图对齐”误写成“生成全量状态表”
- 把本应来自真实 trace / report 的行为证据写成 skill 自己脑补的内容
- 把所有存在的 CLI 都当作默认交付路径，或者反过来遗漏已存在但应按需调用的命令

本 PRD 的目标不是新增一套编译器能力，而是把 `plc-gen` 对意图对齐 / expected-path / scenario 覆盖问题的职责边界冻结下来，并转成 Ralph 可执行的、单轮可完成的文档与 skill 改造任务。

## 2. Goals

- 冻结 `plc-gen` 在“意图对齐 / expected-path / scenario 覆盖”问题上的三层分工。
- 明确 `plc-gen` 只负责作者化的合同、收敛策略和调用策略，不伪造运行证据。
- 明确哪些结果必须来自 RustPLC 真实工具链，例如 trace、validation report、scenario skeleton、scenario suite summary。
- 明确哪些命令是 `plc-gen` 的默认路径，哪些只是可选择调用的附加命令。
- 防止 skill 把 `pairwise`、边界值、近似 MC/DC 误用成“对全 PLC 全变量做全量组合”。
- 保持 `plc-gen` 与架构文档、CLI 实际表面、现有 toolchain blocker 描述同源。

## 3. Responsibility Split

### 3.1 应写入 `plc-gen` skill 的内容

以下内容属于 skill 应长期吸收的稳定知识：

- 意图对齐不是全状态空间枚举，而是围绕业务里程碑、预期路径和关键窗口做作者化合同。
- `intent sequence` / `expected-path` / “哪些变量可视为 don't care” / “哪些条件要做独立影响检查” 属于 skill 生成或修复的资产规划知识。
- 等价类、边界窗口、正交拆分、pairwise、近似 MC/DC 只作为 skill 的覆盖收敛策略，不是要写进编译器输出的自然语言解释。
- `behavior sequence` 必须来自真实 trace / task context / 设备动作记录 / 周期末状态证据，而不是 skill 脑补。
- 当 scenario 工具链遇到兼容性限制时，skill 必须把它报告为 `toolchain compatibility blocker`，而不是误报为 DSL 一定错误。

### 3.2 应由编译链 / 场景工具链真实产出的内容

以下内容不应被写死在 skill 里，而应来自工具链运行结果：

- `scenario-init` 产出的 scenario skeleton
- `scenario-validate` / `project-check` / `sequence-lint` / `scenario-doctor` / `no-board-gate` 产出的验证报告与诊断
- `sim-plc` 产出的 trace、运行日志、IO 快照及可观测行为证据
- runtime / trace / `task_context` 提供的 behavior evidence
- `scenario-gen` 产出的 scenario suite、`summary.json`、`coverage_mode` 和模板选择结果
- expected-path 比对结果，但前提是仓库内已有真实比对器；若当前没有公开稳定入口，skill 不能伪造这类“编译产物”

### 3.3 可供 skill 选择性调用的内容

以下命令属于“工具链能力”，可以被 `plc-gen` 按需调用，但不应一律视为默认主路径：

- 默认优先：`project-check`
- 定点排查：`scenario-validate`、`sequence-lint`、`scenario-doctor`、`no-board-gate`
- 取证仿真：`sim-plc`
- skeleton / 展开：`scenario-init`、`scenario-expand`
- 覆盖生成：`scenario-gen --coverage-mode pairwise|boundary-first|risk-first`

调用原则：

- 只有当用户目标确实需要相应工件时，skill 才调用对应命令。
- `scenario-gen` 是按需覆盖生成工具，不是每个 `plc-gen` 请求都应默认跑的主路径。
- 在 expected-path phase-1 尚未形成稳定公开入口前，skill 不应编造“expected-path compare CLI”。

## 4. User Stories

### US-001: 新增 `plc-gen` 的意图对齐边界 reference
**Description:** 作为 `plc-gen` 维护者，我希望 skill 有一份专门的边界 reference，把“skill 内知识、工具链产物、可选命令”三层分开，这样后续不会再把它们混写。

**Acceptance Criteria:**
- [ ] 在 `.codex/skills/plc-gen/references/` 下新增一份面向意图对齐 / expected-path 的边界 reference。
- [ ] 文档明确区分“写入 skill 的知识”“编译链 / 场景工具链产物”“按需调用命令”三类。
- [ ] 文档明确写出 `intent sequence` / `expected-path` 是作者化合同，不是编译器自动生成的行为证据。
- [ ] 文档明确写出 `behavior sequence` 必须来自真实 trace / runtime evidence。
- [ ] Typecheck passes.

### US-002: 更新 `plc-gen` 主 skill 入口以路由意图对齐请求
**Description:** 作为使用 `plc-gen` 的 agent，我希望 `SKILL.md` 能在用户提到意图对齐、expected-path、pairwise 覆盖或状态爆炸时，自动路由到正确 reference，而不是把这些需求误当成普通 `.plc` 修复。

**Acceptance Criteria:**
- [ ] `.codex/skills/plc-gen/SKILL.md` 明确列出意图对齐 / expected-path / scenario 覆盖相关请求的触发和读取顺序。
- [ ] 主 skill 文案明确写出“不要把意图对齐退化成全状态表穷举”。
- [ ] 主 skill 文案明确写出“不要把行为证据写成推断结果，必须消费真实工具链输出”。
- [ ] 主 skill 文案引用 `intent_alignment_verification.md` 与 `expected-path-simulation.md` 作为长期语义源。
- [ ] Typecheck passes.

### US-003: 在 workflow 中加入“作者化资产 vs 工具链证据”的默认路径
**Description:** 作为 `plc-gen` 使用者，我希望 workflow 文档能说明：何时应先写 scenario / expected-path 合同，何时应跑 trace 与 gate，何时必须停在 blocker，而不是所有请求都直接跑同一组命令。

**Acceptance Criteria:**
- [ ] `references/workflow.md` 新增意图对齐相关路径，明确“先写作者化合同，再取证，再比对”的顺序。
- [ ] workflow 明确区分 `scenario`、`expected-path`、trace、validation report 的角色。
- [ ] workflow 明确写出在 comparator 缺失时的保守输出口径，不得伪装成“已完成完整意图对齐验证”。
- [ ] workflow 明确写出 scenario toolchain blocker 的升级条件与 fallback。
- [ ] Typecheck passes.

### US-004: 在 commands reference 中冻结“默认命令”和“可选命令”边界
**Description:** 作为 `plc-gen` 维护者，我希望 commands reference 对意图对齐相关命令的地位有清晰说明，这样 skill 不会默认乱跑 `scenario-gen`，也不会遗漏真实存在的 `scenario-expand` / `scenario-gen`。

**Acceptance Criteria:**
- [ ] `references/commands.md` 明确标注哪些命令是默认主路径，哪些是可选调用。
- [ ] 文档补上 `scenario-expand` 与 `scenario-gen` 的存在及其适用场景，或显式说明为何仍不对用户默认暴露。
- [ ] 对 `scenario-gen` 明确写出 `pairwise|boundary-first|risk-first` 是覆盖模式，不是“对整机全变量做全排列”。
- [ ] 文档不引入仓库里不存在的命令名或 CLI flag。
- [ ] Typecheck passes.

### US-005: 更新输出契约，要求 skill 明确区分“我写的”和“工具链产出的”
**Description:** 作为调用 `plc-gen` 的用户，我希望最终回答能清楚告诉我哪些文件/合同是 agent 生成的，哪些证据是工具链跑出来的，哪些命令只是本次按需调用，这样结果才可审计。

**Acceptance Criteria:**
- [ ] `references/output-contract.md` 要求最终回答按“作者化资产 / 工具链产物 / 调用命令 / blocker”分组汇报。
- [ ] 输出契约明确要求：若只有 scenario / trace / report，没有比对器结论，不得宣称“intent alignment passed”。
- [ ] 输出契约明确要求：若使用 `scenario-gen`，要报告 `coverage_mode` 与产出的 suite/summary，而不是只说“已做 pairwise”。
- [ ] 输出契约明确要求：toolchain blocker 要与 DSL / semantic blocker 分开。
- [ ] Typecheck passes.

### US-006: 在 troubleshooting 中固化 scenario toolchain compatibility blocker 规则
**Description:** 作为维护者，我希望 `plc-gen` 在复合 guard 或其他 scenario 工具链限制出现时，有一致的故障分流规则，不会重复建议已知会失败的命令。

**Acceptance Criteria:**
- [ ] `references/troubleshooting.md` 或等价公共说明中纳入当前已知的 scenario toolchain compatibility blocker。
- [ ] 文档明确写出：遇到 `unsupported guard expression` 时，不得把问题直接归咎为 PLC 语义错误。
- [ ] 文档明确写出：若必须兼容 scenario 链路，可采用 scenario-friendly lowering；若不能改 DSL，则报告 blocker。
- [ ] 文档与现有 `scenario-toolchain-limitations.md` 保持同义，不出现相互冲突的建议。
- [ ] Typecheck passes.

### US-007: 增加一个端到端 worked example，展示三层分工
**Description:** 作为 `plc-gen` 使用者，我希望 skill 文档里有一个简短 worked example，展示从“用户提意图对齐需求”到“写合同、跑工具、报告证据”的完整路径。

**Acceptance Criteria:**
- [ ] 新增一个 worked example，输入包含 intent mismatch / expected-path / 覆盖收敛诉求。
- [ ] 例子中明确列出：作者化资产、工具链产物、可选命令调用、最终 blocker/完成口径。
- [ ] 例子中展示 pairwise / 边界窗口 / don't care 是 skill 的收敛策略，而不是 trace 输出字段。
- [ ] 例子不依赖仓库内尚未公开稳定的 expected-path CLI。
- [ ] Typecheck passes.

## 5. Functional Requirements

1. FR-1: `plc-gen` 必须明确区分作者化合同、工具链证据和按需调用命令三类职责。
2. FR-2: `plc-gen` 必须把意图对齐定义为场景化里程碑 / expected-path 合同，而不是全局状态空间穷举。
3. FR-3: `plc-gen` 必须把等价类、边界值、正交拆分、pairwise、近似 MC/DC 作为覆盖收敛策略，而不是作为“编译输出说明”。
4. FR-4: `plc-gen` 必须把 behavior evidence 绑定到真实 trace / runtime / validation report，不得人工伪造。
5. FR-5: `plc-gen` 必须明确哪些命令是默认主路径，哪些命令只是按需调用。
6. FR-6: `plc-gen` 必须在 scenario toolchain 不兼容时报告 `toolchain compatibility blocker`，不得误报 DSL 语义错误。
7. FR-7: `plc-gen` 的最终输出契约必须能审计“本次由 agent 写了什么、跑出了什么、还有什么没法证实”。
8. FR-8: `plc-gen` 对意图对齐相关请求的文档更新必须与现有 architecture 文档和 CLI 实际表面保持同源。

## 6. Non-Goals

- 本期不实现新的 expected-path comparator、expected-path CLI 或新的 formal verification 引擎。
- 本期不把 pairwise / boundary-first / risk-first 提升为所有 `plc-gen` 请求的默认路径。
- 本期不要求编译器自动生成完整 `intent sequence` 或完整 `expected-path` 资产。
- 本期不把 runtime trace、scenario summary 或 validation report 重新发明成 skill 内置静态模板。
- 本期不重新设计 `intent_alignment_verification.md` 或 `expected-path-simulation.md` 的架构语义，只消费并转述其稳定边界。

## 7. Design Considerations

- `plc-gen` 的价值在于“收敛 + 路由 + 交付”，不是替代编译链做证据生产。
- skill 里应保留的是“如何拆问题、如何选资产、何时调用什么命令、何时报告 blocker”的规则。
- trace、report、summary、scenario skeleton 之类工件必须保留为工具链产物，否则后续无法审计真假来源。
- 对用户来说，最重要的不是听到一堆方法名，而是看到清楚的责任分层：什么是 agent 写的，什么是工具链跑出来的，什么只是本次额外调用。

## 8. Technical Considerations

- 主要改动文件预计包括：
  - [SKILL.md](/E:/personal_project/rust_plc/.codex/skills/plc-gen/SKILL.md)
  - [workflow.md](/E:/personal_project/rust_plc/.codex/skills/plc-gen/references/workflow.md)
  - [commands.md](/E:/personal_project/rust_plc/.codex/skills/plc-gen/references/commands.md)
  - `E:/personal_project/rust_plc/.codex/skills/plc-gen/references/output-contract.md`
  - `E:/personal_project/rust_plc/.codex/skills/plc-gen/references/troubleshooting.md`
  - 新增 `references/intent-alignment-boundary.md` 或等价命名的 reference
- 语义源必须优先服从：
  - [intent_alignment_verification.md](/E:/personal_project/rust_plc/docs/architecture/intent_alignment_verification.md)
  - [expected-path-simulation.md](/E:/personal_project/rust_plc/docs/architecture/expected-path-simulation.md)
  - [signal-direction.md](/E:/personal_project/rust_plc/docs/architecture/signal-direction.md)
- `scenario-gen` 已在 CLI 帮助和源码中存在，但当前 `plc-gen` commands reference 尚未把它纳入稳定主路径；本期必须明确其地位，而不是继续模糊处理。
- 现有 `scenario-toolchain-limitations.md` 已记录复合 guard blocker；新文档不得与其冲突。

## 9. Success Metrics

- `plc-gen` 文档能让读者在一次阅读后说清三件事：
  - 哪些内容应该内化到 skill
  - 哪些证据必须由 RustPLC 工具链产生
  - 哪些命令只是按需调用
- 后续处理意图对齐请求时，不再出现“把全状态表当默认交付”的建议。
- 后续处理意图对齐请求时，不再出现“没有真实 trace / report 也宣称通过验证”的答案。
- `plc-gen` 对 `scenario-gen`、`scenario-expand`、`scenario-validate`、`sim-plc` 的使用边界可直接从文档中审计。

## 10. Open Questions

- `scenario-gen` 是否应在本期直接进入 `plc-gen` 的稳定 commands reference，还是保留为“存在但非默认”的附加命令。
- `plc-gen` 是否应在用户未显式要求时主动生成 `.expected_path.yaml`，还是只在明确提到意图对齐 / expected-path 时进入该路径。
- 在 expected-path comparator 尚未形成稳定 CLI 前，`plc-gen` 的“已验证”口径是否统一收敛为“合同已写 + trace 已取证 + 比对入口未稳定”。
