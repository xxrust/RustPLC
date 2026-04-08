# plc-gen Multi-Agent Template

本文给 `plc-gen` 一个默认可执行的复杂项目编排模板。

目标不是“凡事都起很多 agent”，而是当任务复杂到单人串行容易震荡时，有固定拆法。

前提：
- skill 调用者默认看不到仓库源码
- 因此多 agent 编排不能建立在“大家都去读源码自己悟”之上
- 主 agent 必须先把源码相关事实压缩成可转交的 public brief

## 1. 何时启用多 agent

满足以下任一条件时，默认启用多 agent：

- 同时涉及 `.system.md` 解释和 DSL 落地
- 同时涉及多文件 bundle、scenario、验证 gate
- 需要 authored `*.intent_alignment.contract.json`
- 需要同时改多个语义块，且天然可拆成不同 write scope
- 用户明确要求复杂项目并行推进

只有在任务明显很小的时候，才退回单实现者路径：

- 只修一个 `.plc`
- 只改一个 fragment
- 只补一个 scenario
- 只解释报错、不写代码

## 2. 默认角色

复杂项目默认使用三层角色：

1. `request-architect`
2. `senior-dsl-implementer` x N
3. `reviewer-validator`

这三个角色都不应把“去看源码”当作默认前提。

### 2.1 request-architect

只做这几件事：

- 收敛需求
- 产出 lowering 决策
- 判定 source shape
- 决定是否需要 intent sidecar
- 把任务拆成多个实现 write scope

它不负责把全部代码自己写完。

### 2.2 senior-dsl-implementer

每个实现者都必须：

- 有明确 write scope
- 有真实编译权限
- 根据编译器/工具链反馈反复修复
- 在自己的范围内把代码收敛到“实现者认为没问题”

它是资深程序员，不是静态文案生成器。

注意：
- 实现者必须有编译权限
- 但 agent 模板本身不规定命令字符串
- 具体命令由主 skill 根据当前环境、launcher 和项目形态决定

### 2.3 reviewer-validator

只在实现者完成后出场：

- 跑 `project-check` 或约定验证链
- 跑必要 tests / regressions
- 挑错、审核、给结论

不要让 reviewer 一边审一边继续承担主实现。

## 3. 默认编排顺序

### 模板 A: 复杂 DSL 项目

1. `request-architect`
   - 输入：用户需求 + 主 agent 提供的 public brief
   - 输出：
     - lowering 摘要
     - source shape 决策
     - authored artifacts 清单
     - implementer 拆分方案
2. `senior-dsl-implementer` x 2~3
   - 按 write scope 并行
   - 各自编译并修复
   - 输出各自已验证的局部结果
3. 主 agent
   - 合并实现者结果
   - 确认主链能编译
4. `reviewer-validator`
   - 跑 gate / tests
   - 输出 findings 和通过/不通过结论

### 模板 B: 含 intent-alignment 的复杂项目

1. `request-architect`
   - 额外决定是否真的需要 `*.intent_alignment.contract.json`
   - 若需要，明确 contract 的 authoritative source
2. `senior-dsl-implementer` x 2~4
   - 实现者 1：DSL source / bundle
   - 实现者 2：scenario / gate wiring
   - 实现者 3：可选 intent sidecar / canonical fixture
   - 实现者 4：仅在测试或 examples 边界足够独立时追加
3. 主 agent
   - 合并并消化边界冲突
4. `reviewer-validator`
   - 基础 gate
   - 若启用了 sidecar，再验证 `intent_alignment`

## 4. implementer 数量规则

默认不要无脑起很多实现者。

推荐规则：

- 1 个实现者：单文件修复、小规模 bundle 修复
- 2 个实现者：DSL 和 scenario/gate 可以明显分开
- 3 个实现者：DSL、验证链、intent sidecar 三块都独立
- 4 个及以上：只有在 write scope 非常清晰时才考虑

如果实现者之间频繁改同一文件，就说明人数太多或拆分错误。

## 5. write scope 模板

每个实现者任务必须明确写出：

- 负责文件
- 禁止越界的文件
- 证明义务
- 完成判据
- 实现者可依赖的 public brief

示例：

### 实现者 A

- 负责：`plc/main.plc` 或 `*.bundle.toml` + `tasks.*`
- 不负责：scenario、intent sidecar、tests
- 证明义务：其负责范围在主链上已收敛，不再存在明显 parser / semantic / lowering 断点

### 实现者 B

- 负责：`scenarios/nominal/normal.yaml`、gate 相关接线
- 不负责：主 DSL 语义结构
- 证明义务：scenario 和 gate 接线已经闭环，reviewer 可以在此基础上独立复核

### 实现者 C

- 仅在明确要求 intent-alignment 时启用
- 负责：`*.intent_alignment.contract.json`、相关 canonical fixture
- 不负责：把 sidecar 当编译产物塞进交付主链
- 证明义务：sidecar 来源、binding 和业务语义已明确，不把它伪装成编译默认产物

## 6. reviewer 入口条件

只有满足以下条件，reviewer 才能出场：

- lowering 决策已冻结
- 所有实现者都完成自己承诺的最小编译/验证
- 主 agent 已完成必要合并
- 当前剩余问题不再属于“代码还没写完”

如果这些条件不成立，reviewer 应把任务打回，而不是帮忙继续主实现。

## 7. reviewer 默认检查表

- DSL source 是否真实可编译
- bundle/source boundary 是否被错误破坏
- scenario 是否真能驱动 gate
- authored sidecar 和 toolchain artifact 是否被正确区分
- `project-check` 是否真跑到了声明的步骤
- 若声称 intent-alignment 已启用，是否真有 `intent_alignment/report.json` 或等价产物

## 8. 最终回答模板

复杂项目最终回答至少要区分：

- 哪个角色做了什么
- 哪些文件是 skill 写入
- 哪些文件是工具链产物
- 哪些证明义务由 implementer 自行闭环
- 哪些验证由 reviewer 独立复核
- 当前是否 `validated` / `validated with warnings` / `failed validation` / `blocked`
- 让看不到源码的人也能理解的摘要结论

## 9. one-shot 约束

复杂项目默认使用 one-shot 编排，而不是让 agent 在流程中反复询问彼此。

固定阶段只有三段：

1. architect 定义
2. implementer 并行实现
3. reviewer 独立审核

每一段都必须一次性交付固定产物：

- architect：lowering brief + write map + proof map
- implementer：patch summary + scope closure statement + residual risks
- reviewer：findings + verdict + residual risks

如果某一段产物不足，就退回上一层重做，不要在当前层里临时补角色职责。
