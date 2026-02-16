# PRD：Ralph 使用规范（无开发板阶段）

日期：2026-02-16

本 PRD 的目的不是描述某一个功能点，而是把“让 Ralph/Codex 作为工程代理在本仓库持续迭代”的**所有必需格式与工作流**固化下来：

- 只做一条 User Story（从 `prd.json` 里挑 `passes: false` 且 `priority` 最小的那条）
- 必须跑质量门禁
- 必须更新 `prd.json`（把已完成 story 的 `passes` 置为 true）
- 必须把关键经验写入 `progress.txt` 顶部的 `## Codebase Patterns`

本 PRD 同时定义“无实体开发板”的下一阶段能力补齐（7 条基础能力）。

---

## 0. Ralph 执行合同（必须遵守）

### 0.1 输入文件（Ralph 每次迭代必须读取）

- `prd.json`
  - `branchName`：目标分支
  - `userStories[]`：待实现故事列表
- `progress.txt`
  - **先读最顶部 `## Codebase Patterns`**，再开始动手

### 0.2 选择 Story 规则

- 只实现 1 条 story
- 选择规则：
  1) 只在 `passes: false` 的 stories 里选
  2) 选 `priority` 数值最小的那条
  3) 不要并行做多条，不要“顺手修复”无关问题

### 0.3 分支规则

- 开始前检查当前分支是否等于 `prd.json.branchName`
- 不等于则切到该分支；若不存在则从 main 创建

### 0.4 质量门禁（每条 Story 必须通过）

- 最低要求：`cargo test --workspace` 通过
- 若 story 影响 CLI 产物/报告：必须增加或更新集成测试（使用 `env!("CARGO_BIN_EXE_rust_plc")`）
- 若 story 引入新命令：必须在文档中写清楚输入/输出文件的用途

### 0.5 交付与记录（每条 Story 必须做）

- 更新 `prd.json`：将该 story 的 `passes` 改为 `true`
- 追加 `progress.txt`：按既定模板写实现内容 + learnings
- 如果发现可复用规律：把它提升到 `progress.txt` 顶部 `## Codebase Patterns`

### 0.6 提交规范（如执行 commit）

- commit message：`feat: [Story ID] - [Story Title]`
- 不提交 broken code

---

## 1. 背景：无实体开发板阶段的问题

当前没有 RP2040 实体板时，仍然存在三类“工程风险缺口”：

1) 板级 IO 与时钟的不确定性无法在真实硬件上验证
2) 证明/验证结果的**能力边界**容易让用户误解（例如“通过”不等于“完备”）
3) 缺少可交付、可追溯、可回滚的发布工件包

因此，本阶段目标是：在不依赖实板的条件下，用“虚拟板级闭环 + CI 门禁 + 可解释报告 + 可追溯发布包”把基础能力补齐。

---

## 2. 目标（对应 7 条基础能力）

> 这里的“完成”指：在没有实体开发板的前提下，尽可能把能力做到工程可交付；需要实板的部分用虚拟替代并明确边界。

1) **回路/控制**：具备最小 PID 子集 + 仿真对象模型 + KPI 回归
2) **顺控**：异常/恢复模板可复用，关键 wait 可恢复（timeout+goto）
3) **实时/确定性**：tick 合同一致；给出结构化上界预算并可门禁
4) **报警/诊断**：统一结构化验证报告，warnings 分级，可 deny
5) **变更/可回滚**：release-bundle + sha 清单 + git 元信息
6) **安全**：模拟量抽象验证的覆盖与边界透明化；规则绑定率可见
7) **数据闭环**：虚拟板级 trace 与 SIL 可对比；trace-diff 可门禁

---

## 3. 范围（In Scope）

- 不依赖实体开发板，补齐上述目标的“软件可验证/可门禁/可交付”部分
- 引入必要的 CLI、报告格式、测试与文档

## 4. 非目标（Out of Scope）

- 真实 GPIO 电气噪声、ADC 精度/温漂等需要实板的物理验证
- EtherCAT/Modbus 等大型硬件抽象层扩展（可作为后续里程碑）

---

## 5. 用户故事（以 prd.json 为准）

本文件只描述工作流与目标；具体 story 列表以 `prd.json.userStories` 为唯一事实来源。

执行顺序：按 `priority` 从小到大实现全部 `passes: false` 的故事。

---

## 6. 设计原则（Ralph 实施时的约束）

- **向后兼容优先**：默认 CLI 行为不破坏既有用法；新增功能通过新 flag/新命令启用
- **报告先于功能**：任何“能力增强”必须有结构化报告体现覆盖范围与边界
- **确定性优先**：仿真/虚拟板级必须可重复、可回归
- **可门禁优先**：能 deny warnings、能 trace-diff fail、能 release bundle 校验

---

## 7. 成功指标

- 在无实板情况下，用户可一条命令跑通：compile/verify -> sim -> virtual-board -> trace-diff -> release-bundle
- CI 能阻断：warnings 风险、trace 不一致、产物缺失或不可追溯
- 安全验证对模拟量抽象的覆盖与边界可被审计（不再“黑盒通过”）

