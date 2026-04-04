---
name: plc-gen
description: Generate, repair, and validate RustPLC DSL files (`.plc`) and scaffolded project deliverables such as `plc/main.plc`. Use when the user wants to generate PLC code from a confirmed system contract, repair an existing `.plc`, deliver a scaffolded RustPLC project, run project/scenario validation commands, or understand the current optimization surface of RustPLC.
---

# plc-gen

生成能通过真实 RustPLC 工具链验证的 `.plc`，而不是只写出“看起来像 DSL”的文本。

这个 skill 负责：
- 生成或修复 `plc/main.plc`
- 在项目级请求下同时处理 scaffold、scenario 与验证命令
- 当用户已提供 `plc/main.system.md` 或等价 system contract 时，把它作为 `.plc` 建模输入
- 明确区分“当前产品已支持的能力”和“仍是能力缺口/阻塞项”
- 当用户提到“优化”“提速”“候选方案”时，准确说明当前 optimization 的真实边界

不要把 SKILL.md 写成百科。按需加载 reference：
- `references/workflow.md`
  何时走 scaffold，何时只修单个 `.plc`，默认验证链是什么
- `references/commands.md`
  真实 CLI、launcher 选择、Day-1 命令链，以及 installed binary / source workspace 的区别
- `references/project-layout.md`
  scaffold 后有哪些文件，优先编辑哪些文件
- `references/generation-rules.md`
  生成 `.plc` 时必须遵守的语义约束
- `references/optimization.md`
  当前 optimization 的真实边界、库 API、支持的 rewrite 类型
- `references/output-contract.md`
  最终回答必须包含什么，如何表达成功/阻塞/失败
- `references/troubleshooting.md`
  launcher、workspace、scenario、toolchain 兼容性等常见卡点

## Source Of Truth

优先服从这些长期稳定语义源：
- `AGENTS.md`
- `docs/architecture/signal-direction.md`

## Core Rules

1. 只有通过真实 RustPLC 工具链验证的结果，才算完成。
2. 对项目级请求，优先 scaffold，而不是只回一段孤立的 `.plc`。
3. 对系统意图仍未冻结的请求，先收敛成 `plc-system` 风格的 system contract；如果用户已经给出确认版 `.system.md`，默认直接消费它。
4. 不要把并发 task 解释成“单执行点在 `task.step` 间跳转”。
5. 不要把拓扑已闭合的机构动作降级成显式传感器编排。优先保留高层设备动作语义。
6. 如果 DSL / IR 还承载不了某类高层设备动作结果，明确标成能力缺口或 blocker，不要擅自伪造中间变量或手写 `wait sensor` 闭环。
7. 对 axis 动作，默认按 blocking 长时动作处理；不要把 `axis.move_*` 写成“本 step 内立即完成”的普通即时 action。
8. 不要把 optimization 说成已有 CLI。当前公开的是 library API，不是 `rust_plc optimize ...`。
9. 如果 `.system.md` 仍含未冻结项，只把真正会改变 `.plc` 结构的内容记为 assumptions / blockers，不要擅自补全成已确认 contract。
10. 当 `.system.md` 已明确给出 task 划分、mode、warning/fault 分流、共享资源或计数门槛时，必须把这些结构显式降到 `.plc`，不要压平成单一大循环。

## 默认工作方式

1. 判断这是单文件修复、项目交付，还是 optimization / 提速请求。
2. 如果用户已提供确认版 `.system.md`，先做一轮 lowering 摘要，再决定 scaffold 与 `.plc` 结构。
3. 如果是项目交付，先读 `references/workflow.md`、`references/project-layout.md`、`references/commands.md`。
4. 如果是 `.plc` 生成或修复，先读 `references/generation-rules.md`。
5. 如果提到“优化”“更快”“候选方案”，再读 `references/optimization.md`。
6. 对 Day-1 项目交付，默认顺序应是：
   - launcher 判断
   - scaffold / 关键文件定位
   - `plc/main.plc` 生成或修复
   - `project-check` 或等价最小验证链
   - assumptions / blockers 说明
7. 对“confirmed `.system.md` -> 直接生成 `.plc`”请求，先做一轮 lowering 摘要：
   - task partition -> `[tasks]`
   - blocking / timeout / manual wait -> `wait` / `delay` / `axis.move_*`
   - topology-closed device action -> 保持高层设备动作与结果分流
   - mode matrix -> 专门的 service / supervisor task
   - warning vs fault -> 拆分 warning task 与 fault task
   - counters / streak / retry / rate -> `[topology] variable` + `compute`
   - shared resource / interlock -> `semantic_resource`、`claim`、`requires`、`conflicts_with`
8. 如果 system contract 已给出 task 名称，优先保留这些名称，不要重新发明别名。
9. 如果用户明确要求走当前 scenario 工具链，再额外做一轮 scenario 兼容性检查；若存在工具链已知限制，应明确标成 `toolchain-blocked`，不要假装 `validated`。
10. 生成或修复后，必须给出真实验证路径；不能停在“理论上可行”。

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
- 返回已验证的 `.plc` 或项目产物
- 明确指出真实 blocker / contract 缺口 / toolchain 限制

不要返回“理论上可行”的伪完成答案。
