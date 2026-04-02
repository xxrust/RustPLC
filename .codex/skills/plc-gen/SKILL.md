---
name: plc-gen
description: "基于已确认的系统描述或等价工业控制需求，生成、修复并验证 RustPLC DSL（`.plc`）以及 scaffold 项目中的 `plc/main.plc`。当用户要从需求生成 PLC、修复现有 `.plc`、交付完整 RustPLC 项目、运行验证命令，或询问 RustPLC 已有优化能力该如何正确使用时使用。"
---

# plc-gen

生成能够通过真实 RustPLC 流水线的 `.plc`，而不是只写出“看起来像对的 DSL”。

这个 skill 的职责是：

- 生成或修复 `plc/main.plc`
- 在项目级请求下同时组织 scaffold、scenario 与验证命令
- 当用户已提供 `plc/main.system.md` 或等价 system contract 时，把它当作 `.plc` 建模与交付顺序的直接输入
- 明确告知哪些能力是现成产品能力，哪些还不是
- 在用户提到“优化”“提速”“候选方案”时，准确使用现有 optimization 能力边界

不要把本文件写成百科。
按需加载对应 reference：

- `references/workflow.md`
  何时走 scaffold，何时只修单文件，默认交付路径是什么。
- `references/commands.md`
  真实 CLI 命令、launcher 选择、day-1 命令链，以及 installed binary / source workspace 的差异。
- `references/project-layout.md`
  scaffold 后有哪些文件，先改哪些文件。
- `references/generation-rules.md`
  生成 `.plc` 时必须遵守的语义与建模约束。
- `references/optimization.md`
  现有 optimization 能力的真实边界、library API、可支持的 rewrite 类型。
- `references/output-contract.md`
  最终输出必须包含什么，如何表达成功/阻塞/失败。
- `references/troubleshooting.md`
  `--help`、launcher、多 binary、scenario 缺失等常见卡点。

## Source of Truth

先服从这些长期稳定语义源：

- `AGENTS.md`
- `docs/architecture/signal-direction.md`


## Core Rules

1. 只有通过真实 RustPLC 工具链验证的结果，才算完成。
2. 对于项目级请求，优先 scaffold，而不是只回一段孤立 `.plc`。
3. 对于系统意图仍然不稳定的请求，先依赖 `plc-system` 风格的 system contract，再生成 `.plc`；如果用户已经给出确认版 `.system.md`，默认直接消费它，而不是重新发散成大问卷。
4. 不要把并发 task 解释成“单执行指针在 `task.step` 之间跳转”。
5. 不要把拓扑已闭合机构写成显式传感器脚本。task step 应先表达设备动作，再按设备动作结果分流，而不是手写 `wait sensor`、互斥判断或伪确认变量去重建机构语义。
6. “拓扑已闭合”的判定以及最小结果集合要求，服从 `AGENTS.md` 中“task 中的设备动作必须保持高层语义”的定义。
7. 对于拓扑闭合设备，若当前 DSL 或 IR 还承载不了 `AGENTS.md` 要求的结果枚举，就明确标成能力缺口与 blocker，保留高层设备动作意图，不得擅自降级成传感器 choreography，也不得静默省略结果分流。
8. 不要把 optimization 说成已有 CLI；当前只有 library API。
9. 如果 `.system.md` 仍带有“待联调冻结项”或等价未决项，只把真正会改变 `.plc` 结构的项记为 assumptions / blocker，不要擅自补全为已确认 contract。
10. 当 `.system.md` 已明确列出并发 task、模式矩阵、warning/fault 分流、共享资源或计数阈值时，必须把这些结构显式降到 `.plc`，而不是压平成单一自动主循环。

## 默认工作方式

1. 判断这是单文件修复、项目交付，还是 optimization/提速请求。
2. 如果用户已提供确认版 `.system.md`，先按它收敛 task、blocking、fault route、mode 与资源边界，再决定是否 scaffold。
3. 如果是项目交付，先读 `references/workflow.md`、`references/project-layout.md`、`references/commands.md`。
4. 如果是 `.plc` 生成或修复，先读 `references/generation-rules.md`。
5. 如果用户提到“优化”“更快”“候选方案”，再额外读 `references/optimization.md`。
6. 对陌生用户的 Day-1 回复，默认顺序应是：launcher 判断 -> scaffold / 文件顺序 -> `plc/main.plc` 生成或修复 -> 最小验证链 -> assumptions / blockers。
7. 对“confirmed `.system.md` -> 直接生成 `.plc`”请求，默认先做一轮 lowering 摘要：
   - task partition -> `[tasks]`
   - blocking / timeout / manual wait -> `wait` / `delay` / `axis.move_*`
   - topology-closed device action -> 按 `AGENTS.md` 的规范结果集合做高层设备动作分流，而不是显式传感器脚本
   - mode matrix -> 专门的 service/supervisor task
   - warning vs fault -> 分开的 warning task / fault task
   - counters / streak / retry / rate -> `[topology] variable` + `compute`
   - shared resource / interlock -> `semantic_resource`、`claim`、`requires`、`conflicts_with`
8. 如果 system contract 已经给出 task 名称，优先保留这些名称作为 `.plc` task 名，而不是重新发明一套别名。
9. 如果 system contract 明确区分“修复后刷新继续”与“告警停机”，优先建模成 warning task + refresh wait，与 fault task 分开，不要把两类恢复语义混成一个 fault handler。
10. 如果用户明确要走当前 scenario 工具链，再额外做一轮 scenario-compatibility 检查：
   - 是否存在复合 wait guard
   - 是否需要把复合 guard 拆成若干单条件 wait + `if` / 中间 step
   - 如果语义允许，是否可直接降成顺序单条件 wait，并保持原始真值条件
   - 只要无法明确证明该 guard 与 `AGENTS.md` 定义的拓扑闭合设备动作语义无关，就不要为兼容工具链改写，直接标记 `toolchain-blocked`
   - 是否应把验证状态标成 toolchain-blocked，而不是 validated
11. 生成或修复后，必须给出真实验证路径。

## Launcher Discipline

必须先判断用户处于哪种运行环境：

- 已安装 `rust_plc` binary
- RustPLC 源码仓 workspace

这两种环境的命令前缀和工作目录不一样。
不要把“进入 scaffold 目录后运行 `cargo run ...`”当成默认建议，因为 scaffold 本身不是 Cargo 项目。

## Completion Standard

最少要做到以下之一：

- 返回已验证的 `.plc` 或项目产物
- 明确指出真实阻塞 contract 缺口

不要返回“理论上可行”的伪完成答案。
