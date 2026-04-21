# AI for AI 平台愿景

RustPLC 不是 "AI 帮人写 PLC 程序" 的工具。它是一个让 AI 系统之间能可靠协作的工控工程平台。

---

## 定位

一个 AI Agent 生成控制意图，另一个 AI Agent 能可靠地消费、验证、批评、变换或交付这些意图 — 这就是 "AI for AI" 的含义。

大多数 "AI for 软件" 产品止步于文本生成。对工业控制来说，这远远不够。缺失的层次是：

- 语义闭合 — 生成物必须进入统一模型，不能停留在 prompt 文本
- 形式化验证 — 四引擎并行证明安全性/活性/时序/因果性
- 确定性执行 — no_std 运行时，tick 级确定性
- 可追溯性 — 从 .plc 到固件到 trace 的完整证据链
- 可复现发布 — SHA256 manifest + git 元数据，任何人可复现

---

## 工程闭环

```
AI Agent 生成 .system.md（需求描述）
    ↓
AI Agent 生成 .plc + 场景 + I/O 映射 + 发布元数据
    ↓
RustPLC 编译：Parser → AST → Semantic → IR
    ↓
四引擎验证：Safety / Liveness / Timing / Causality
    ↓
不通过 → 结构化错误（行号 + 修复建议）→ Agent 自动修复 → 重新编译
    ↓
通过 → 仿真 / 运行时 / 代码生成 / 发布包
    ↓
人类工程师：定义边界、审查证据、批准发布
```

关键：AI 的输出不是 "代码文本"，而是进入了一个有语义闭合、有形式化证明、有确定性执行的工程管线。这是 RustPLC 和 "prompt wrapper" 的本质区别。

---

## 不可妥协的契约

| 契约 | 含义 |
|------|------|
| 统一语义入口 | AI 生成物必须经过 Parser → AST → Semantic → IR，不能绕过 |
| 验证是主路径 | 验证不是可选插件，是编译流水线的一等公民 |
| 运行时不发明语义 | runtime-core 只执行 IR 定义的语义，不能自行补充 |
| 代码生成显式擦除 | Codegen 必须明确哪些语义被保留、哪些被擦除，不能静默丢弃 |
| 输出可机器消费 | verification_report.json / trace.jsonl / manifest.json 都是结构化的 |

---

## Agent 协作接口

RustPLC 的输出不只是代码，而是一组 Agent 之间的协作接口：

| 输出 | 用途 |
|------|------|
| IR JSON | 语义模型，可被下游 Agent 分析 |
| verification_report.json | 验证结果，Agent 可据此决定是否修复 |
| timing_report.json | 时序统计，Agent 可据此优化 |
| trace.jsonl | 仿真轨迹，Agent 可据此回归 |
| diagnostics | 结构化错误，Agent 可据此自动修复 |
| release-bundle/ | 完整证据包，可被审计 Agent 消费 |

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
- `pid_control` — PID 控制示例
- `generate_from_requirements` — 从需求生成 .plc

---

## 底线

RustPLC 的差异化不是 "又一个生成器"。

差异化是：AI 的输出经过了一个工程闭环，这个闭环保证输出是 **可验证、可执行、可审计、可复现** 的。

这是工控领域对 AI 生成代码的最低要求，也是 RustPLC 存在的理由。
