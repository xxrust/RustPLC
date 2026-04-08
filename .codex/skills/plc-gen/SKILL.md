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
