# AGENTS.md

## 定位

本文件是 RustPLC 的项目总纲，也是新接手项目时的第一份导航文档。

它回答四类长期稳定问题：

- 这个项目的核心目标是什么
- 系统按什么分层运行
- 某类问题应先看哪些文件
- 做一类改动时，通常必须联动哪些层

它不记录阶段性方案、一次性决策、临时 workaround 或会频繁变化的实现细节。  
容易变化的内容应写入：

- `docs/*_spec.md`
- `docs/*_development_guide.md`
- 模块附近测试

## 第一性原理

RustPLC 的本质不是“写一门 PLC DSL”，而是构建一个：

- 有明确语义边界的工业控制建模系统
- 可验证的编译系统
- 可执行的 runtime 系统
- 可追踪、可诊断、可演进的工程系统

做设计或改动时，先问：

1. 这个问题属于哪一层
2. 这个语义是否被显式建模
3. 这个语义是否能进入 verification
4. 这个语义是否能稳定映射到 runtime 与 codegen
5. 这个改动是在消除例外，还是在增加例外

默认偏好：

- 明确语义，反对隐式行为
- 稳定抽象，反对局部补丁
- 可验证模型，反对只靠运行观察
- 直接重构到更优模型，反对被历史兼容默认绑架
- 防止抽象层下沉，反对把上层问题压到下层硬补

## 核心目标

项目的长期目标固定为三件事：

- 用 DSL 表达工业控制意图
- 将 DSL 收敛为统一 IR
- 在 IR 上同时支撑 runtime、verification 与 codegen

任何新增能力，如果只能“跑起来”但不能进入 IR、不能验证、不能诊断，就不算完成。

## 总体架构

编译与执行主链固定为：

`Parser -> AST -> Semantic -> IR -> Verification / Runtime Bridge / Codegen`

各层职责固定：

- `Parser`：把 DSL 文本解析为 AST，不承担语义裁决
- `AST`：忠实表示源码结构，不做语义归并
- `Semantic`：名称解析、约束检查、预处理展开、IR 降级前收敛
- `IR`：全系统唯一的规范语义模型
- `Verification`：在 IR 上验证 safety、liveness、timing、causality
- `Runtime Bridge`：把 IR 映射到 runtime-core 可执行结构，并强制执行侧约束
- `Codegen`：从 IR 输出目标代码，不负责补齐缺失语义

长期规则：

- 语法糖必须在进入 IR 前展开
- verification 与 runtime 不直接消费 AST
- runtime 不能反向发明 DSL 语义
- codegen 只能消费已经闭合的 IR 语义

## 项目地图

第一次接手项目时，优先按下面路径建立心智模型。

### 稳定入口

- `AGENTS.md`：项目总纲、分层原则、源码导航
- `docs/`：规范、开发指南、架构文档

### 核心源码目录

- `src/parser/plc.pest`：DSL PEG 语法
- `src/parser/mod.rs`：Parser 到 AST 的降级逻辑
- `src/ast/mod.rs`：AST 类型定义
- `src/semantic/mod.rs`：语义分析、预处理、IR 降级主入口
- `src/ir/mod.rs`：IR 类型定义
- `src/runtime_bridge.rs`：IR 到 runtime-core 的桥接
- `src/verification/`：四类验证引擎
- `src/codegen/st.rs`：IEC 61131-3 ST 代码生成
- `src/diagnostics.rs`：统一诊断结构

### 运行时与子项目

- `crates/runtime-core/src/lib.rs`：runtime 执行模型与动作语义
- `crates/sim/`：仿真相关能力
- `crates/codegen/`：代码生成相关子项目
- `crates/io-traits/`：I/O 抽象
- `crates/board-rp2040/`：板级目标
- `crates/web-server/`：Web 侧能力

### 测试与夹具

- `tests/examples_integration.rs`：示例 PLC 编译/回归总入口
- `tests/*.rs`：端到端、回归、契约测试
- `examples/*.plc`：DSL 示例与回归输入
- `devices/*.toml`：设备库定义
- `scenarios/*.yaml`：仿真场景

## 阅读顺序

如果此前没接触过本项目，建议按以下顺序阅读：

1. 本文件 `AGENTS.md`
2. `src/ir/mod.rs`
3. `src/semantic/mod.rs`
4. `src/runtime_bridge.rs`
5. `crates/runtime-core/src/lib.rs`
6. `src/verification/*.rs`
7. 与当前需求相关的 `docs/*_spec.md` 或 `docs/*_development_guide.md`
8. 对应的 `tests/*.rs` 与 `examples/*.plc`

原因：

- 先看 AGENTS，知道分层和导航
- 再看 IR，先抓住语义汇合点
- 然后沿“语义从何而来、如何执行、如何验证”往两侧展开

## 问题类型到文件入口

遇到问题时，先判断问题属于哪一类，再进对应文件，不要盲目全仓搜索。

### 1. 语法解析问题

先看：

- `src/parser/plc.pest`
- `src/parser/mod.rs`
- `src/ast/mod.rs`

典型现象：

- 新关键字无法解析
- `true/false`、标识符、wrapper 规则冲突
- 错误文案停留在 pest 原始 expected 文案

### 2. 语义建模问题

先看：

- `src/semantic/mod.rs`
- `src/ir/mod.rs`
- `src/diagnostics.rs`

典型现象：

- DSL 语法合法但语义不合法
- 某特性没有被正确降级到 IR
- 某约束本应在语义阶段报错，却拖到 runtime 才炸

### 3. 运行时执行问题

先看：

- `src/runtime_bridge.rs`
- `crates/runtime-core/src/lib.rs`
- `tests/*runtime*`

典型现象：

- tick 调度错误
- action 执行行为错误
- timeout / fault / wait / delay / step 推进异常

### 4. 验证问题

先看：

- `src/verification/safety.rs`
- `src/verification/liveness.rs`
- `src/verification/timing.rs`
- `src/verification/causality.rs`
- `src/ir/mod.rs`

典型现象：

- 明显非法的程序没被拦住
- 合法程序被误报
- 新语义进入 runtime 了，但 verification 没跟上

### 5. 代码生成问题

先看：

- `src/codegen/st.rs`
- `src/ir/mod.rs`

典型现象：

- IR 正常但 ST 输出错误
- 不支持的结构没有被明确拒绝

### 6. 设备库、组件拓扑、I/O 映射问题

先看：

- `devices/*.toml`
- `src/component_library.rs`
- `src/component_topology.rs`
- `src/plc_port.rs`
- `src/topology_semantic_gate.rs`
- `src/runtime_bridge.rs`

## 常见改动的联动路径

下面这些是长期稳定的“改一处通常要看多处”的路径。

### 新增 DSL 动作或语法原语

通常要联动：

1. `src/parser/plc.pest`
2. `src/parser/mod.rs`
3. `src/ast/mod.rs`
4. `src/semantic/mod.rs`
5. `src/ir/mod.rs`
6. `src/runtime_bridge.rs`
7. `crates/runtime-core/src/lib.rs`
8. `src/verification/*.rs`
9. `src/codegen/st.rs`
10. `tests/` 与 `examples/`

原则：

- 不允许只在 parser 接受语法，却不定义 IR
- 不允许只在 runtime 执行特例，却不进入 verification

### 修改执行模型

通常要联动：

1. `src/ir/mod.rs`
2. `src/runtime_bridge.rs`
3. `crates/runtime-core/src/lib.rs`
4. `src/verification/*.rs`
5. `src/codegen/st.rs`
6. `tests/*runtime*`
7. `tests/*verification*`

说明：

- 执行模型变化不是 runtime 私事
- 只改 runtime 而不改 IR 或 verification，属于典型抽象层下沉
- `runtime-core` 处于 `no_std` 约束下，不使用动态分配；task 级执行上下文采用 `MAX_ACTIVE_TASKS + active_task_count` 的定长数组模式
- runtime 调度按 task 声明顺序（索引升序）逐 tick 遍历所有 active task；某个 task 命中 blocking step 只阻塞自身，不得阻塞同 tick 其他 task
- 并发 runtime 回归中若需同时断言多个 task 的 step 位置，优先使用 `Runtime::task_context(task_idx)` 读取 task 局部状态；`location()` 仅反映当前执行游标，不适合作为多 task 断言入口
- Task 级 pending action 元数据应从 step 语句收集，而不是仅从 transition.actions 推断；`delay/wait/timeout` 等阻塞路径常会让 transition 不携带动作
- Step 离开判定应集中到统一 completion 决策（action/delay/wait 共用规则）；避免在各指令分支散落“是否跳转”的特例判断
- 对包含 Pending 长时动作的 step，后续 tick 必须从挂起动作继续轮询，不得重放该 step 中挂起动作之前的即时 action（避免重复 side effect）
- `axis.move_relative/axis.move_absolute` 属于默认 blocking 长时动作；即使未显式编写 `wait`，也应由 `Pending -> Done/Fault` 生命周期驱动 step 离开，回归测试至少覆盖 Pending->Done 与 Pending->Fault
- axis 动作的 `timeout/on_reject/on_motion_fault/on_safety_fault` 与细分 route 必须在 bridge 阶段降级成 runtime 可执行的 `StepId` 元数据；runtime 在 Pending 轮询阶段按“先专用 route、后主桶 fallback”执行分流，避免回退为裸 `RuntimeError::AxisFault`
- `runtime_bridge` 构建 runtime task 时优先保留“无跨 task 入边”的 root task 边界；若全量 task 都存在跨 task 入边，则回退到 IR 初始 task 作为 active root，避免旧流程直接退化为并发全激活副作用
- runtime transition budget 口径固定为“per-task-per-tick”：单 task 同 tick 转移上限为 `MAX_TRANSITIONS_PER_TASK_PER_TICK`；报告中的全局上界应按 `active_task_count * per_task_cap` 计算，并在告警/错误中带上 task 与 active_task_count 上下文
- 构造 runtime budget 循环告警夹具时，若使用 `on_complete: goto <self>` 形成 SCC，必须同时提供 `timeout` 或 `allow_indefinite_wait: true`，否则会先被 liveness 门禁拦截而无法触发预算分析断言

### 修改语义门禁或诊断

通常要联动：

1. `src/semantic/mod.rs`
2. `src/diagnostics.rs`
3. 相关 spec / development guide
4. `tests/*diagnostic*`

原则：

- 错误码、payload、修复建议应稳定
- 诊断变更要同步测试，不要只改文案
- 扩展诊断/告警 payload 字段时优先走可选字段（`serde default + skip_serializing_if`）做向后兼容；并补“旧 payload 解析新结构、新 payload 解析旧结构”的回归测试

### 修改 verification 规则

通常要联动：

1. `src/verification/*.rs`
2. `src/ir/mod.rs`
3. `src/semantic/mod.rs`
4. `tests/*verification*`

原则：

- verification 不是“后置插件”
- 新规则必须有正例、反例、边界例
- safety 并发建模应与 runtime active-task 口径对齐：优先从“无跨 task 入边”的 root task 构造初始全局状态；若无 root task，再回退 IR 初始 task，避免验证与执行的 task 激活集合漂移
- safety 全局状态应显式携带 task 级当前位置与 pending action 标记，跨 task `conflicts_with/requires` 断言必须在该组合状态空间中检查
- liveness 夹具若覆盖 `axis.move_*` Pending 语义，必须先满足 AXIS 语义门禁（`timeout` + `on_reject/on_motion_fault/on_safety_fault`）；否则会在 `build_state_machine` 阶段失败，无法进入 liveness checker
- timing 并发分析应同时报告 task 局部完成时间与并发全局完成时间（active task 的 `max`），并保留顺序 `sum` 作为对照基线；`must_complete_within` 走局部 nominal 口径，`must_complete_within_worst_case` 需纳入 pending 动作上界与 timeout 上界
- causality 跨 task 链路建模应把 `[topology] variable`、`compute`、`set_analog_expr` 与纯 extern 调用统一纳入 dataflow 边；缺失数据依赖时必须显式报链路断裂，不能默认放行
- causality 对 `parallel` 分支组合或 `on_complete` 跳转导出的推断链路，只在 `action -> wait` 已可达时执行断链检查，避免跨分支偶然组合造成误报

### 修改设备模型、组件模型或 I/O 映射

通常要联动：

1. `devices/*.toml`
2. `src/component_library.rs`
3. `src/component_topology.rs`
4. `src/plc_port.rs`
5. `src/topology_semantic_gate.rs`
6. `src/runtime_bridge.rs`
7. 相关 tests

## 长期工程原则

### 1. IR 是唯一语义汇合点

- 跨 parser、semantic、runtime、verification 的概念，必须在 IR 中有唯一表示
- 不允许不同层各自维护近似同义但结构不同的版本

### 2. 语义先于实现

- 先定义语义边界，再实现 parser、runtime、verification 和 tests
- 测试负责锁定语义，不负责反向发明语义

### 3. 错误必须尽早暴露

- 语法错误在 parser 暴露
- 合法语法下的非法语义在 semantic 暴露
- runtime bridge 只做可执行性校验，不替上游兜底

### 4. verification 是主路径

- 新能力必须同时考虑 safety、liveness、timing、causality
- 如果某能力不能进入验证模型，它还没有设计完成

### 5. 文档、示例、生成器必须同步

- DSL 契约变更后，文档、示例、tests、skills 必须同步更新
- 不允许编译器契约和生成器提示长期漂移

### 6. 遇到显著更优方案时，优先直接重构

- 显著更优、更一致、长期维护成本更低的方案，应优先直接重构
- 历史兼容不是默认要求
- 回退策略不是默认要求
- 兼容层如果存在，必须被视为额外成本并单独论证

### 7. 必须防止抽象层下沉

- 上层语义问题不得下沉到 runtime、bridge、codegen 或测试里靠特例修补
- 如果多个下层模块重复理解同一语义，说明抽象位置已经错了
- 发现下沉时，应优先上提抽象，而不是继续补丁

## 典型专题的稳定入口

以下专题是项目中的长期主线，遇到相关需求时应优先进入对应文件。

### 外部函数

先看：

- `src/semantic/mod.rs`
- `src/extern_functions.rs`
- `src/runtime_bridge.rs`
- `crates/runtime-core/src/lib.rs`
- `src/verification/causality.rs`
- `docs/extern_function_mvp_spec.md`
- `docs/extern_function_development_guide.md`

### 操作合约 EXOP

先看：

- `src/parser/plc.pest`
- `src/parser/mod.rs`
- `src/semantic/mod.rs`
- `src/runtime_bridge.rs`
- `tests/operation_contract_diagnostics_us012.rs`
- `docs/*operation*`

### 组件拓扑与设备库

先看：

- `src/component_topology.rs`
- `src/component_library.rs`
- `devices/*.toml`
- `src/semantic/mod.rs`

### 轴资源与运动参数

先看：

- `src/axis_profile.rs`
- `src/parser/plc.pest`
- `src/parser/mod.rs`
- `src/semantic/mod.rs`
- `src/runtime_bridge.rs`
- `crates/runtime-core/src/lib.rs`
- `tests/*axis*`

## 测试与回归原则

测试结构长期固定为：

- `cargo test --lib`：模块级单元测试
- `tests/*.rs`：集成与回归测试
- `tests/examples_integration.rs`：示例程序回归总入口

做改动时至少回答：

1. 这个变化需要单元测试还是集成测试
2. 是否需要新增示例作为长期回归输入
3. 是否需要新增诊断快照或验证反例

新能力的理想闭环是：

- 一个最小 parser/semantic 单测
- 一个 runtime 或 bridge 测试
- 一个 verification 测试
- 一个 examples 回归输入
- 若语义由示例承载（尤其是 blocking/pending 行为），优先同时补 `tests/examples_integration.rs`（编译回归）与 `tests/runtime_bridge_us006.rs`（运行时行为回归）

## 文档分层

文档职责长期固定：

- `AGENTS.md`：架构原则、目录导航、改动路径
- `docs/*_spec.md`：冻结语法与契约
- `docs/*_development_guide.md`：实现指南、迁移说明、落地建议

如果某内容的主要价值是“帮助未来新人知道去哪里看、改哪些层”，它适合写在 AGENTS。  
如果某内容的主要价值是“描述某一具体能力的细节字段和边界”，它应写入 spec 或 development guide。

## 协作原则

### 先判定问题类型，再决定是否并行

复杂问题先区分它主要是：

- 语法问题
- 语义建模问题
- 执行模型问题
- 验证模型问题
- 文档与迁移问题

### 能拆就拆

如果任务可以拆成边界清晰、接口明确、弱耦合的子任务，应优先拆分。

典型拆分方式：

- `parser / ast`
- `semantic / ir`
- `runtime / runtime_bridge`
- `verification`
- `docs / examples / tests`

### 可独立时优先多 agent 并行

满足以下条件时，优先多 agent 并行：

- 契约已冻结
- 接口边界明确
- 共享数据结构基本稳定
- 各子任务可独立验证

以下情况不适合并行：

- 语义尚未冻结
- 关键 IR 结构仍在变化
- 多个模块会同时修改同一个语义源头

## 对 AGENTS.md 本身的要求

本文件必须同时满足四点：

- 稳定：不追逐阶段性实现细节
- 可导航：新人读完知道去哪里看
- 可行动：知道一类改动通常要动哪些层
- 可裁决：出现分歧时能回到统一原则

如果一条内容经常随版本变动，它大概率不该写在这里。  
如果一条内容能长期帮助新人快速定位源码入口，它就应该写在这里。
