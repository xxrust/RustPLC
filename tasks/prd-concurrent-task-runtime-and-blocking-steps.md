# PRD: 并发 Task 调度、阻塞 Step 语义与可验证执行模型重构

## 1. Introduction / Overview

当前 RustPLC 的 DSL 允许声明多个 `task` 和多个 `step`，但在运行时桥接阶段，所有 DSL task 会被扁平化为单个 runtime task，执行器内部也只有一个当前位置。这使现有系统更接近“单状态机跳转”，而不是真正的多 task 并发调度。

这套模型在简单顺序控制中可用，但对复杂工业控制场景存在三个结构性问题：

- `task` 不是独立调度单元，无法表达“上料与下料同时推进”这类多工位并发逻辑。
- `step` 内的长时动作目前缺少“进行中”状态，导致 `axis.move_*` 等动作不能自然阻塞当前 step，用户必须靠额外 `wait` 补语义。
- 形式验证仍以近似单状态机的 IR 为基础，无法准确覆盖未来需要的并发 task + 阻塞 step 组合语义。

本需求的目标是在不破坏 RustPLC 分层架构的前提下，同时完成三项重构并形成一个统一闭环：

- 将 runtime 从“单执行点”升级为“每个 task 一个执行上下文”的并发调度模型。
- 将 `step` 语义收敛为“完成前不可离开”的阻塞执行单元，但只对耗时、需反馈或显式延迟的场景自动阻塞。
- 将 IR、验证引擎和诊断体系同步升级，使 `safety/liveness/timing/causality` 四类验证在并发语义下仍然成立。

本 PRD 按你的边界约束制定：

- 范围包含完整实现方案、迁移方案、回归测试门禁和文档更新。
- task 调度允许同一 tick 内串联推进非阻塞 step。
- step 自动阻塞仅覆盖耗时动作、需要反馈完成的动作，以及 `delay/wait/timeout` 这类天然跨 tick 语义。
- 四类验证必须在首版设计中同时闭环，而不是只做 safety 保底。

## 2. Goals

- 将 DSL `task` 升级为 runtime 的真实并发调度单元，每个 task 持有独立执行上下文。
- 定义统一的阻塞 step 语义，使“动作已发出但尚未完成”的场景能自然停留在当前 step。
- 让 `axis.move_*` 及后续其他长时动作内建“发命令 + 挂起 + 完成/故障/超时”的闭环语义。
- 保持 DSL 心智模型稳定，优先通过 runtime/IR/verification 重构吸收复杂性，而不是把负担转嫁给 DSL 用户。
- 让 `safety/liveness/timing/causality` 在并发 task 和阻塞 step 下都可验证、可回归、可诊断。
- 提供明确迁移路径，避免现有 `.plc` 程序在语义切换后产生隐蔽行为回归。

## 3. User Stories

### US-001: 冻结并发 Task 与阻塞 Step 术语契约
**Description:** 作为架构维护者，我希望先冻结核心术语和状态机执行契约，以便 runtime、IR、验证器和文档基于同一套语义实现。

**Acceptance Criteria:**
- [ ] 文档定义并冻结 `active task`、`task context`、`blocking step`、`non-blocking step`、`pending action`、`completion condition` 等术语。
- [ ] 明确写出“同一 tick 内允许串联推进非阻塞 step，但阻塞 step 不可越过”的规则。
- [ ] 明确写出“task 并发”是指多个 task 各自拥有独立执行上下文，而不是单执行点跳转。
- [ ] 明确列出首版自动阻塞范围：`axis.move_*`、`delay`、`wait`、依赖外部反馈完成的后续扩展动作。
- [ ] 文档写明后续所有故事必须遵循本语义，不允许局部引入冲突定义。

### US-002: 扩展 IR 以表达多 Task 并发执行状态
**Description:** 作为编译器开发者，我希望 IR 能显式表达 task 集合、task 入口和 task 独立上下文，这样验证器和 runtime 可以消费统一模型。

**Acceptance Criteria:**
- [ ] `StateMachine` 或等价 IR 结构新增 task 级执行上下文模型，而不是只保留全局单一 `initial`。
- [ ] IR 可表达每个 task 的入口状态、当前阻塞状态、定时器和挂起动作元数据。
- [ ] IR 保留现有 `task_name.step_name` 可读性，避免调试和诊断回退到匿名索引。
- [ ] IR 变更不会绕过现有 preprocess 机制，repeat/operation-contract/device 注入仍在 IR 生成前完成。
- [ ] `cargo test --lib` 中新增并发 IR 结构单元测试。

### US-003: Runtime 为每个 Task 建立独立执行上下文
**Description:** 作为 runtime 维护者，我希望每个 task 拥有独立的当前位置、进入时间和挂起状态，这样某个 task 阻塞时不会卡死其他 task。

**Acceptance Criteria:**
- [ ] runtime 内部不再只保存单个 `Location`，而是保存每个活跃 task 的执行上下文。
- [ ] 每个 task context 至少包含：当前 step、step_entered_at、等待/超时状态、挂起动作状态。
- [ ] 同一 tick 内调度器可遍历所有活跃 task，并按固定、可文档化的顺序执行。
- [ ] 任何一个 task 进入阻塞 step 时，其他 task 仍可在同一 tick 或后续 tick 继续推进。
- [ ] 新模型具备单元测试，覆盖两个 task 同时活跃、一个阻塞一个继续前进的场景。

### US-004: 定义“step 完成前不可离开”的统一执行规则
**Description:** 作为 DSL 用户，我希望 step 的离开条件统一而可预测，这样不用猜哪些 action 会自动推进，哪些需要手工补 wait。

**Acceptance Criteria:**
- [ ] step 进入时执行其 action 序列，但是否离开 step 取决于 step completion，而不是单纯取决于 action 已调用。
- [ ] 即时动作如 `set/log/compute` 在无额外等待条件时可在当前 tick 内完成 step。
- [ ] `delay`、`wait` 和带挂起动作的 step 在完成条件未满足前必须留在当前 step。
- [ ] step completion 规则形成单独文档，并在运行时和验证器中使用同一术语。
- [ ] 为“即时 step”“延迟 step”“反馈等待 step”“挂起动作 step”提供最小正反例测试。

### US-005: 为长时动作引入 Pending 生命周期
**Description:** 作为运动控制开发者，我希望 `axis.move_*` 具备 `Pending` 生命周期，以便动作发出后能阻塞当前 step，直到真正完成、故障或超时。

**Acceptance Criteria:**
- [ ] 运行时动作结果从二态扩展到至少三态：`Pending`、`Done`、`Fault`。
- [ ] `axis.move_*` 首次进入 step 时只发起一次命令，后续 tick 轮询同一挂起动作，不重复下发命令。
- [ ] 当动作返回 `Pending` 时，当前 task 必须停留在当前 step。
- [ ] 当动作返回 `Done` 时，当前 task 才能进入后续 step。
- [ ] 当动作返回 `Fault` 时，必须沿现有 `on_reject/on_motion_fault/on_safety_fault` 路由跳转。
- [ ] 对 `axis.move_*` 增加 runtime-core 单元测试，覆盖 `Pending -> Done`、`Pending -> Fault`、`Pending + timeout`。

### US-006: 保持 DSL `axis.move_*` 契约稳定并吸收阻塞复杂度
**Description:** 作为 PLC 程序作者，我希望不用在每个 `axis.move_*` 后手工补大量样板 `wait`，而是由 DSL 原语自身承担运动完成语义。

**Acceptance Criteria:**
- [ ] 现有 `axis.move_relative/absolute` DSL 语法保持兼容，除非存在必须破坏兼容的理由并有迁移说明。
- [ ] `timeout`、`on_reject`、`on_motion_fault`、`on_safety_fault` 继续作为动作级契约保留。
- [ ] 编译器文档明确说明 `axis.move_*` 现在是“阻塞 step 的长时动作”。
- [ ] 现有示例程序中依赖“move 立即返回”的用法被审计并列出迁移策略。
- [ ] `tests/examples_integration.rs` 补充至少一个展示阻塞 move 的示例。

### US-007: 调度器支持“每 tick 可串联非阻塞 step”
**Description:** 作为架构设计者，我希望保留同一 tick 内串联推进非阻塞 step 的能力，以避免把所有简单控制都退化成低吞吐的一步一 tick。

**Acceptance Criteria:**
- [ ] 调度器明确定义：单个 task 在同一 tick 内可串联推进多个已完成或非阻塞 step。
- [ ] 调度器明确定义：一旦某个 task 遇到阻塞 step，该 task 在本 tick 内停止推进。
- [ ] 其他 task 是否继续推进仅受其自身上下文影响，不受某个 task 阻塞影响。
- [ ] 保留现有每 tick 最大转换数护栏，且在并发语义下重新定义其统计口径。
- [ ] 增加回归测试，覆盖同 tick 串联 3 个即时 step 与另一个 task 同 tick 遇到阻塞 step 的组合场景。

### US-008: Safety 验证器支持并发 Task 组合状态
**Description:** 作为验证引擎维护者，我希望 safety 验证能在多 task 并发状态下检查资源冲突、互斥和设备约束，而不是只看单路径顺序执行。

**Acceptance Criteria:**
- [ ] Safety 引擎可在全局状态中组合多个 task 的当前位置和挂起动作状态。
- [ ] `conflicts_with` / `requires` 在并发 task 下仍能检测同 tick 或重叠时窗的冲突。
- [ ] 至少有一个反例测试能捕获“task A 上料占用资源，task B 下料同时争用同资源”的违规。
- [ ] 至少有一个正例测试能证明两个 task 使用不同资源时可并发通过。
- [ ] 文档更新说明 safety 在并发模型下的状态空间扩展与边界。

### US-009: Liveness 验证器支持阻塞 Step 与并发等待
**Description:** 作为验证引擎维护者，我希望 liveness 能区分“合理等待”和“永远无法完成的阻塞”，避免并发模型下误报或漏报死锁。

**Acceptance Criteria:**
- [ ] Liveness 引擎能区分 `Pending` 长时动作、显式 `wait` 和 `delay` 的等待语义。
- [ ] 可检测“两个 task 相互等待对方释放资源”这类并发死锁。
- [ ] 对带 `allow_indefinite_wait` 的合法等待点保留豁免语义。
- [ ] 增加死锁、活锁和合法等待三类回归测试。
- [ ] 文档说明并发 task 下 liveness 分析的规则和已知限制。

### US-010: Timing 验证器支持并发调度下的最坏执行界
**Description:** 作为时序分析维护者，我希望 timing 能在 task 并发和阻塞动作存在时，继续给出 defendable 的最坏情况界限。

**Acceptance Criteria:**
- [ ] `must_complete_within` 与 `must_complete_within_worst_case` 在并发模型下有明确且可实现的解释。
- [ ] 长时动作的 `Pending` 持续时间和 `timeout` 上界可纳入最坏情况分析。
- [ ] timing 报告明确区分“单 task 本地时长”和“受并发调度影响的全局完成时长”。
- [ ] 增加至少一个“两个 task 并发导致全局完成时间增加”的 timing 回归测试。
- [ ] 文档写明 timing 分析采用的调度假设。

### US-011: Causality 验证器支持跨 Task 因果链
**Description:** 作为因果性验证维护者，我希望 causality 能识别不同 task 间通过设备、变量或事件形成的因果链，而不是假设链都落在同一顺序路径上。

**Acceptance Criteria:**
- [ ] 因果链节点可跨 task 建模，并能表达共享设备/共享变量的依赖。
- [ ] `axis.move_*` 的 `Pending/Done/Fault/timeout` 路径都能纳入因果链分析。
- [ ] 至少有一个反例测试能捕获“下料 task 缺少上料完成信号因果链”的问题。
- [ ] 至少有一个正例测试能证明跨 task 因果链完整。
- [ ] 文档说明并发 task 下 causality 的边构建策略。

### US-012: Runtime Bridge 不再扁平化 DSL Task
**Description:** 作为桥接层维护者，我希望 bridge 保留 DSL 的 task 边界并生成并发运行所需结构，而不是提前塌缩成单 task。

**Acceptance Criteria:**
- [ ] bridge 不再把所有状态塞进单个 runtime task。
- [ ] bridge 输出能保留 task 边界、入口和 task 局部 step 图。
- [ ] bridge 继续校验 tick 对齐、I/O 可解析性和动作支持能力。
- [ ] bridge 新增并发语义相关错误类型或诊断上下文。
- [ ] 现有 runtime bridge 回归测试扩展到多 task 并发场景。

### US-013: 诊断与迁移方案稳定可执行
**Description:** 作为项目维护者，我希望现有用户能明确知道哪些程序行为会改变、如何迁移、出现问题时如何定位。

**Acceptance Criteria:**
- [ ] 新增迁移指南，列出“旧语义 vs 新语义”的行为差异。
- [ ] 针对依赖旧 `axis.move_*` 即时推进的案例给出明确迁移建议。
- [ ] 诊断 payload 保持结构化，新增字段优先采用向后兼容方式。
- [ ] 为关键破坏性语义变化定义稳定错误码或警告码。
- [ ] 增加诊断回归测试，确保新旧字段兼容解析。

### US-014: 示例、文档与 CI 门禁形成闭环
**Description:** 作为团队成员，我希望核心示例、文档和 CI 都使用新语义，防止后续实现偏离设计。

**Acceptance Criteria:**
- [ ] 新增至少两个完整示例：`双 task 并发` 和 `阻塞 axis.move`。
- [ ] 更新 AGENTS、相关 spec/development guide 与 skills 文档，避免生成器提示漂移。
- [ ] CI 增加并发 runtime、四类验证和性能门禁。
- [ ] `cargo test --lib`、关键集成测试和 examples 回归在新模型下通过。
- [ ] 文档中明确列出“不支持的并发模式”和未来扩展边界。

## 4. Functional Requirements

1. FR-1: 系统必须把 DSL `task` 作为 runtime 的独立调度单元，而不是在桥接阶段扁平化为单 task。
2. FR-2: 系统必须为每个活跃 task 维护独立执行上下文，至少包含当前位置、进入时间、等待状态和挂起动作状态。
3. FR-3: 系统必须允许同一 tick 内串联推进已完成或非阻塞 step，但不得越过阻塞 step。
4. FR-4: 系统必须把 `delay`、`wait` 和长时动作视为阻塞 step 的合法来源。
5. FR-5: 系统必须为长时动作提供 `Pending` 生命周期，并支持“首次发起 + 后续轮询 + 完成/故障/超时”语义。
6. FR-6: `axis.move_*` 必须在不要求用户额外补样板 `wait` 的前提下实现阻塞 step 语义。
7. FR-7: `timeout`、`on_reject`、`on_motion_fault`、`on_safety_fault` 必须在新运行模型下保持明确且可验证的路由行为。
8. FR-8: IR 必须可表达多 task 并发状态及阻塞 step 所需的挂起信息。
9. FR-9: Safety 验证必须在多 task 组合状态下检查冲突、依赖和设备约束。
10. FR-10: Liveness 验证必须在阻塞 step 与并发等待场景下识别死锁、活锁和合法等待。
11. FR-11: Timing 验证必须在并发调度假设下计算 task 局部和系统全局的时序约束。
12. FR-12: Causality 验证必须支持跨 task 的设备、变量和动作因果链。
13. FR-13: 诊断系统必须为并发/阻塞语义变化提供稳定、结构化且可迁移的提示。
14. FR-14: 示例、回归测试和 CI 必须覆盖并发 task、阻塞 step 和四类验证的最小闭环。

## 5. Non-Goals

- 本期不追求通用抢占式实时调度器，不引入复杂优先级抢占或 EDF/RTOS 级调度策略。
- 本期不要求所有 action 都自动变成阻塞动作，只覆盖耗时、需反馈或显式等待的语义单元。
- 本期不要求一次性把所有设备原语都升级为 `Pending`，首版以 `axis.move_*` 为核心闭环入口。
- 本期不引入新的 UI 范围，也不把前端作为主交付目标。
- 本期不解决分布式 PLC、多控制器网络协同等更高层并发问题。

## 6. Design Considerations

- 语义一致性优先于局部性能优化。DSL、IR、runtime、verification 必须使用同一套“阻塞 step / 并发 task”定义。
- 应优先保持 DSL 心智模型稳定，把复杂性吸收到运行时和验证层，而不是让用户到处手工补 `wait`。
- 需要严格区分“task 并发”与“单 task 同 tick 串联推进”，避免实现阶段再次混淆这两个概念。
- 对现有用户最敏感的变化是 `axis.move_*` 从即时动作变为阻塞动作，文档和诊断必须正面处理这一行为变化。

## 7. Technical Considerations

- 预期改动层级至少覆盖 [src/ir/mod.rs](E:/personal_project/rust_plc/src/ir/mod.rs)、[src/runtime_bridge.rs](E:/personal_project/rust_plc/src/runtime_bridge.rs)、[crates/runtime-core/src/lib.rs](E:/personal_project/rust_plc/crates/runtime-core/src/lib.rs)、[src/verification/safety.rs](E:/personal_project/rust_plc/src/verification/safety.rs)、[src/verification/liveness.rs](E:/personal_project/rust_plc/src/verification/liveness.rs)、[src/verification/timing.rs](E:/personal_project/rust_plc/src/verification/timing.rs)、[src/verification/causality.rs](E:/personal_project/rust_plc/src/verification/causality.rs)。
- `TransitionAction`、runtime action 结果和调度状态需要同步演进，不能只在单层补 `Pending`。
- 现有“每 tick 最多 64 个转换”护栏需要重新定义为全局口径还是每 task 口径，并在 runtime budget 与文档中统一。
- 需要提前决定并发调度的公平性规则，否则 liveness/timing 会失去稳定假设。
- 需要对示例和测试夹具进行分类审计：哪些程序本来就是顺序语义，哪些隐含依赖旧的非阻塞 move 行为。

## 8. Implementation Phasing / Gates

1. Gate-A 语义冻结
   产出术语契约、调度规则、阻塞 step 定义和 DSL 行为差异说明。US-001 通过前，后续实现不得启动。
2. Gate-B IR + Runtime 最小闭环
   完成并发 task 上下文、阻塞 step 执行规则和 `axis.move_*` `Pending` 生命周期。US-002 ~ US-007 全部通过后，才可推进验证器改造。
3. Gate-C 四类验证闭环
   并发语义下同时补齐 safety/liveness/timing/causality。US-008 ~ US-011 全部通过后，才可视为核心能力完成。
4. Gate-D Bridge / Migration / Docs / CI
   完成 bridge 保 task、迁移指南、示例、门禁和文档同步。US-012 ~ US-014 通过后，方可宣布需求落地。

## 9. Success Metrics

- 至少一个“上料 task 与下料 task 并发推进”的示例可在新 runtime 中稳定运行。
- 至少一个 `axis.move_*` 阻塞 step 示例可在无额外样板 `wait` 的情况下按预期完成或超时故障。
- 四类验证在并发语义下均具备正例、反例和回归门禁。
- 现有关键示例在迁移后无未解释的行为变化；若有变化，迁移指南中可明确定位到原因。
- CI 可稳定捕获并发调度回归、验证器回归和关键性能退化。

## 10. Open Questions

- 并发调度首版是否需要任务优先级，还是统一采用固定顺序轮询即可。
- `Pending` 动作是否只允许一个 step 内一个挂起动作，还是未来要支持同 step 多个挂起动作组合。
- fault task 被唤起时是作为普通 task 加入调度，还是具备更高优先级的即时接管语义。
- timing 的“全局完成时间”是否以所有活跃 task 都完成为界，还是允许按 task 级单独声明完成约束。
