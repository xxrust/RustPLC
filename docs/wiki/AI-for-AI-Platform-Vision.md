# AI for AI 平台愿景

RustPLC 不是 "AI 帮人写 PLC 程序" 的工具，也不是传统 PLC 编辑器外面套一层聊天助手。

传统 PLC 编辑器的设计对象是人：让工程师更方便地写梯形图、查变量、点设备、在线调试。RustPLC 的设计对象是 agent：让 agent 能从需求、专利或设计意图出发，完成文档创建、工程规划、串行或并行写代码、编译器验证、自主推理修复和证据化交付。

---

## 定位

AI for AI 的含义不是“AI 帮人点 PLC 编辑器”，而是“PLC 工程系统本身按 agent 可执行任务来设计”。

大多数 "AI for 软件" 产品止步于文本生成。对工业控制来说，这远远不够。缺失的层次是：

- 需求收敛 — 专利、需求、设计意图必须能收敛成 `main.system.md`
- 结构化规划 — 拓扑、设备语义、工件模型、front-door、`process_model` 必须先于 task/step
- 可分工实现 — agent 可以按结构化目录串行或并行生成不同层的源码与场景
- 形式化验证 — 四引擎并行证明 safety / liveness / timing / causality
- 自主修复 — 结构化诊断、report、trace 和 gate 结果必须能被 agent 继续消费
- 可复现交付 — 发布包、manifest、git 元数据和证据文件必须可审计

---

## Agent-native 工程链路

```
人类输入需求 / 专利 / 设计意图
    ↓
agent 起草 main.system.md、边界、验收点
    ↓
agent 建立 topology / device semantics / workpiece / front-door / process_model
    ↓
agent 串行或并行生成 task/step、fault、scenario、config、docs
    ↓
RustPLC 编译：Parser → AST → Semantic → IR
    ↓
verification / runtime bridge / codegen / no-board gate
    ↓
不通过 → 结构化诊断 + trace/report → agent 自主推理修复
    ↓
通过 → release bundle + evidence → 人类审查批准
```

关键：AI 的输出不是一段“看起来像 PLC 的代码文本”，而是一个按工程层级组织、能被编译器验证、能被 runtime/codegen 消费、能被 trace/report 追责的项目。

这才是 RustPLC 和 "prompt wrapper" 或传统 PLC 编辑器插件的本质区别。

---

## 不可妥协的契约

| 契约 | 含义 |
|------|------|
| Agent-first 输入 | 人给需求、专利或设计意图，不要求人先写 PLC 程序 |
| 源侧结构化 | AI 生成物必须落到 system / topology / process_model / task / scenario 等层 |
| 统一语义入口 | 最终控制语义必须经过 Parser → AST → Semantic → IR，不能绕过 |
| 验证是主路径 | 验证不是可选插件，是编译流水线的一等公民 |
| 运行时不发明语义 | runtime-core 只执行 IR 定义的语义，不能自行补充 |
| 代码生成显式擦除 | Codegen 必须明确哪些语义被保留、哪些被擦除，不能静默丢弃 |
| 反馈可机器消费 | diagnostics / verification_report.json / trace.jsonl / manifest.json 必须能反哺 agent 推理 |

---

## Agent 工程接口

RustPLC 的资产不只是代码，而是一组供 agent 执行工程任务的接口：

| 资产 | 用途 |
|------|------|
| `main.system.md` | 从需求/专利/设计意图收敛出的系统语义合同 |
| `00_topology/` | 设备、连接、工件位置、容量、资源边界 |
| `process_model/` | task/step 之前的调度意图，便于 agent 避免错误串行化 |
| `02_process/` / `04_faults/` | agent 可分工生成的执行流和故障流 |
| diagnostics | 结构化错误，agent 可据此自动修复 |
| verification_report / timing_report / trace | agent 可据此回归、优化和解释 |
| release-bundle/ | 完整证据包，可被人或审计 agent 复核 |

---

## MCP 集成

RustPLC 提供 MCP Server（`rustplc-mcp/`），AI Agent 可直接调用编译器：

```json
{
  "mcpServers": {
    "rustplc": {
      "command": "python",
      "args": ["-m", "server"],
      "cwd": "rustplc-mcp"
    }
  }
}
```

可用工具：
- `validate_plc` — 验证 .plc 文件，返回结构化诊断
- `compile_plc` — 编译并获取 IR
- `get_rustplc_skill_guide` — 获取 DSL 编写指南

可用 Prompt 模板：
- `two_cylinder` — 双气缸基础示例
- `extern_function` — 外部函数示例
- `process_device` — 过程设备语义动作示例
- `generate_from_requirements` — 从需求生成 .plc

---

## 底线

RustPLC 的差异化不是 "又一个生成器"。

差异化是：RustPLC 的语言、目录、验证、诊断、场景和交付物都围绕 agent 执行工程任务设计。

这是工控领域让 agent 真正接管工程执行的前提，也是 RustPLC 存在的理由。
