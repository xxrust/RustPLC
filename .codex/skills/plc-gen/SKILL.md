---
name: plc-gen
description: Deliver, repair, and validate RustPLC DSL source sets and scaffolded projects. Use when Codex needs to generate or repair RustPLC DSL sources from a confirmed `plc/main.system.md` or equivalent system contract, whether as a single `.plc` file or a multi-file `.bundle.toml` plus fragments, scaffold a Day-1 RustPLC project, run project or scenario validation commands, or explain the current optimization surface of RustPLC.
---

# plc-gen

交付能够通过真实 RustPLC 工具链验证的 RustPLC DSL source set 或项目。

当前 scaffold 默认入口文件是 `plc/main.plc`。
当前产品支持两类 PLC source shape：
- 单文件 `.plc`
- 多文件 `.bundle.toml` + fragments，由 loader 组装 `topology`、`constraints`、`tasks` 后再进入编译链

这个 skill 的默认目标是交付一个可验证的 RustPLC 项目或 DSL source set，并按项目的 source shape 组织 DSL sources。

这个 skill 负责：
- 交付或修复基于 scaffold 的 RustPLC 项目
- 交付或修复 RustPLC DSL source set，包括单文件 `.plc` 与多文件 `.bundle.toml` + fragments
- 在已确认的 `plc/main.system.md` 或等价 system contract 基础上，生成或修复与之匹配的 DSL sources
- 在项目级请求下同时处理 scenario、验证命令与交付路径
- 只在用户明确要求业务意图对齐时，生成或修复可选的 `*.intent_alignment.contract.json` authored sidecar
- 明确区分“当前产品已支持的能力”和“仍是能力缺口的 blocker”
- 当用户提到“优化”“提速”“候选方案”时，准确说明当前 optimization 的真实边界

不要把 `SKILL.md` 写成百科。按需加载这些 reference：
- `references/workflow.md`
  何时走 scaffold，何时只修单个 `.plc`，默认验证链是什么
- `references/multi-agent-template.md`
  复杂项目默认如何编排 architect / implementer / reviewer，以及何时并行、何时收口
- `references/public-brief-template.md`
  主 agent 在复杂项目里如何把源码相关事实压缩成可转交给子 agent 的公开 brief
- `references/commands.md`
  真实 CLI、launcher 选择、Day-1 命令链，以及 installed binary / source workspace 的区别
- `references/project-layout.md`
  scaffold 后有哪些关键文件，优先编辑哪些文件
- `references/generation-rules.md`
  生成 DSL sources 时必须遵守的语义约束
- `references/optimization.md`
  当前 optimization 的真实边界、库 API 与支持的 rewrite 类型
- `references/output-contract.md`
  最终回答必须包含什么，如何表达成功、blocker 与失败
- `references/troubleshooting.md`
  launcher、workspace、scenario、toolchain 兼容性等常见卡点
按需加载这些 agent 角色文件：
- `agents/request-architect.md`
  负责把需求收敛成 DSL lowering 决策，并在复杂项目里做任务拆分
- `agents/senior-dsl-implementer.md`
  负责实际写 `.plc` / bundle / fragments，并拥有编译修复权限
- `agents/reviewer-validator.md`
  只在实现者认为程序已收敛后出场，负责验证、回归与审核结论

## Source Of Truth

优先服从这些长期稳定语义源：
- `AGENTS.md`
- `docs/architecture/signal-direction.md`
- `docs/architecture/intent_alignment_verification.md`
  仅当用户明确要求 intent-alignment / intent gate / comparator 时才消费这份语义源

## Core Rules

1. 只有通过真实 RustPLC 工具链验证的结果，才算完成。
2. 对项目级请求，默认目标是 scaffold 项目或 DSL source set 交付，而不是只回一段孤立的 `.plc`。
3. 对单文件修复请求，才把“只修现有 `.plc`”视为主路径；它是本 skill 的子场景，不是总定位。
4. 对多文件 bundle 请求，优先保持现有 source boundary，不要无故回退到单文件。
5. 对系统意图仍未冻结的请求，先收敛成 `plc-system` 风格的 system contract；如果用户已经给出确认版 `.system.md`，默认直接消费它。
6. 不要把并发 task 解释成“单执行点在 `task.step` 间跳转”。
7. 不要把拓扑已闭合的机构动作降级成显式传感器编排；优先保留高层设备动作语义。
8. 如果 DSL 或 IR 还承载不了某类高层设备动作结果，明确标成 blocker，不要擅自发明中间变量或手写 `wait sensor` 闭环。
9. 对 `axis.move_*`，默认按 blocking 长时动作处理，不要写成“本 step 内立刻完成”的普通即时 action。
10. 不要把 optimization 说成已有 CLI；当前公开的是 library API，不是 `rust_plc optimize ...`。
11. 如果 `.system.md` 仍含未冻结项，只把真正会改变 DSL source shape 或结构的内容记为 assumptions 或 blockers，不要擅自补全成已确认 contract。
12. 当 `.system.md` 已明确给出 task 划分、mode、warning 或 fault 分流、共享资源或计数门槛时，必须把这些结构显式降到 DSL sources，不要压平成单一大循环。
13. 如果现有项目已经采用 `.bundle.toml` + fragments，或需求本身更适合按 `topology`、`constraints`、`tasks` 分拆，就保持或建立这种多文件边界。
14. `*.intent_alignment.contract.json` 是 authored sidecar，不是编译器产物，也不是 scaffold 默认必交付物。只有用户明确要求业务意图对齐、phase-2 comparator、或要让 `project-check` 带 intent-alignment gate 时才生成或修复它。
15. 不要把 DSL `task.step` 名字直接抄成 intent contract milestone；milestone 必须表达业务里程碑，并绑定显式 observation bindings，来源必须是已确认的 intent source，而不是从 trace 或代码反推硬编。
16. 明确区分“skill 写入的源文件”和“工具链跑出来的产物”：`*.plc`、`.bundle.toml`、fragments、`plc/main.system.md`、scenario、可选 intent sidecar 属于前者；`verification_report.json`、`sil_trace.jsonl`、`project_check_report.json`、`intent_alignment/report.json` 属于后者。
17. 复杂项目默认不是单 agent 一把梭，而是三层分工：需求/拆分、实现、审核。先冻结 lowering 和 write split，再并行实现，最后独立审核。
18. “实现 agent”必须拥有真实编译权限，并以编译器/工具链反馈驱动修复；没有这层闭环，就不算资深实现者。
19. “审核 agent”不得在需求未冻结、实现未收敛时提前出场；它的职责是验证和挑错，不是替实现者一边写一边猜。
20. agent 角色文件不要教 agent 具体用什么命令。agent 应拿到的是职责、输入、输出、证明义务；具体命令由主 skill 按当前环境决定。
21. 复杂项目优先走 one-shot 编排：先冻结 lowering 和拆分，再一次性交付给实现者并行推进，最后一次性交给 reviewer 审核；不要把 skill 写成多轮对话脚本。
22. 调用这个 skill 的人默认看不到仓库源代码。skill 本身必须提供足够的公开工作协议，不能把关键定义藏在“去读源码再理解”里。
23. skill 的主入口必须先把当前任务压成一份可转交的 public brief，再交给子 agent；不要让子 agent 依赖调用者自行查看源码。
24. 只有主 agent 可以按需读取仓库、命令参考和实现细节；子 agent 默认只消费主 agent 明确下发的 brief、边界和证明义务。
25. 如果 brief 不足以支持拆分或审核，正确动作是补 brief，而不是让子 agent 越权去读源码。

## 默认工作方式

1. 先判断这是单文件修复、多文件 source bundle 修复、项目级交付，还是 optimization 请求。
2. 如果用户已提供确认版 `.system.md`，先做一轮 lowering 摘要，再决定是单文件 `.plc`、多文件 `.bundle.toml` + fragments，还是 scaffold 默认布局。
3. 如果是项目交付，优先读取 `references/workflow.md`、`references/project-layout.md`、`references/commands.md`。
4. 如果是 DSL source 生成或修复，优先读取 `references/generation-rules.md`。
5. 如果提到“优化”“更快”“候选方案”，再读取 `references/optimization.md`。
6. 对 Day-1 项目交付，默认顺序应是：
   - launcher 判断
   - scaffold 或关键文件定位
   - `plc/main.system.md` 确认
   - DSL source entry 与 source shape 判断
   - scaffold 默认入口 `plc/main.plc`，或现有 bundle/fragments 的生成或修复
   - `scenarios/nominal/normal.yaml` 对齐
   - 如用户明确要求 intent-alignment，则补可选 `*.intent_alignment.contract.json` sidecar
   - `project-check` 或等价最小验证链
   - assumptions 或 blockers 说明
7. 对“confirmed `.system.md` -> 直接生成 DSL sources”请求，先做一轮 lowering 摘要：
   - task partition -> `[tasks]`
   - blocking / timeout / manual wait -> `wait` / `delay` / `axis.move_*`
   - topology-closed device action -> 保持高层设备动作与结果分流
   - mode matrix -> 专门的 service 或 supervisor task
   - warning vs fault -> 拆分 warning task 与 fault task
   - counters / streak / retry / rate -> `[topology] variable` + `compute`
   - shared resource / interlock -> `semantic_resource`、`claim`、`requires`、`conflicts_with`
8. 如果 system contract 已给出 task 名称，优先保留这些名称，不要重新发明别名。
9. 如果用户明确要求走当前 scenario 工具链，再额外做一轮 scenario 兼容性检查；若存在工具链已知限制，应明确标成 `toolchain-blocked`，不要假装 `validated`。
10. 生成或修复后，必须给出真实验证路径，不能停在“理论上可行”。
11. 如果生成了 intent sidecar，要明确说明它绑定的 authoritative intent source、它是 authored artifact，以及 `project-check` 是否实际跑到了 `intent_alignment` 这一步；不要把“可运行 sidecar”说成“编译默认产物”。
12. 如果任务复杂到同时涉及 `.system.md` 解释、DSL 生成、scenario/gate、intent sidecar 或多文件 bundle，先读取 `agents/request-architect.md` 产出 lowering 决策和任务拆分，再让多个实现 agent 并行作业。
13. 每个实现 agent 都应拥有明确 write scope；如果多个实现 agent 可能改同一文件，说明拆分还没做对，应退回需求/拆分层重切边界。
14. 审核/测试 agent 只在实现 agent 给出“程序已能稳定编译、主链无明显结构问题”的结论后才介入；它优先跑 `project-check`、相关 tests 和最小必要回归。
15. 对复杂项目，默认直接套用 `references/multi-agent-template.md` 的编排模板；只有当任务明显更简单时，才退化成单实现者路径。

## One-Shot Protocol

复杂项目默认按下面这一个固定协议执行，而不是临场发明流程：

1. 主 agent 读取需求与现有上下文。
2. 主 agent 先整理一份 `public brief`，至少包含：
   - 任务目标
   - 当前 source shape
   - 已冻结的 system contract / lowering facts
   - 当前已有文件与期望写入物
   - authored artifacts 范围
   - 不允许改变的边界
   - 当前已知 blocker / assumptions
3. `request-architect` 基于这份 brief 一次性输出：
   - source shape 决策
   - lowering 决策
   - authored artifacts 清单
   - write scope 拆分
   - 每个实现者需要提交的证明义务
4. 主 agent 审核 architect 输出；若拆分仍冲突，就在这一层修正，不让实现者带着模糊边界开工。
5. `senior-dsl-implementer` x N 并行执行，各自只处理自己的 write scope，并提交：
   - 已修改文件
   - 已完成的局部收敛说明
   - 剩余 blocker / 风险
   - 对“已满足证明义务”的声明
6. 主 agent 合并实现结果，只做必要整合，不重做子任务。
7. `reviewer-validator` 一次性审查并给出：
   - findings
   - 是否允许交付
   - 剩余风险

one-shot 的关键不是“每个 agent 只说一句话”，而是每层都有固定输入输出，不靠来回聊天补定义。

## Launcher Discipline

先判断用户处于哪种环境：
- 已安装 `rust_plc` binary
- RustPLC 源码 workspace

这两种环境的命令前缀和工作目录不同：
- `rust_plc ...` 可以在 scaffold 项目目录内直接运行
- `cargo run --release --bin rust_plc -- ...` 必须在 RustPLC 仓库根目录运行
- scaffold 本身不是 Cargo 项目，不要让用户 `cd` 进 scaffold 目录后再跑 `cargo run ...`

## Completion Standard

至少做到以下之一：
- 返回已验证的 DSL source entry，例如单文件 `.plc` 或 `.bundle.toml`
- 返回已验证的 scaffold 项目产物
- 明确指出真实 blocker、contract 缺口或 toolchain 限制

不要返回“理论上可行”的伪完成答案。
