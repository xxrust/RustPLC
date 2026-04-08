# plc-gen Output Contract

本文约束最终回答应长什么样。

## 最低输出

始终返回：
- 结果摘要
- 生成或修复后的 DSL source set / 项目结果
- assumptions
- 实际使用或推荐的 launcher / 命令
- validation 状态
- 写明哪些文件是本次由 skill 写入，哪些是工具链运行后生成

## 项目级请求时额外返回

按需补充：
- `plc/main.system.md`
- DSL source entry
- 如果采用 bundle，则补充 `.bundle.toml` 与关键 fragments
- `scenarios/nominal/normal.yaml`
- 如果用户明确要求 intent-alignment，则补充 `*.intent_alignment.contract.json`
- 最小验证命令链
- 当前 gate / codegen / build 状态

## source set 表达方式

根据实际交付形态表达结果：

### scaffold 默认布局

- system contract: `plc/main.system.md`
- DSL source entry: `plc/main.plc`
- scenario: `scenarios/nominal/normal.yaml`
- optional authored sidecar: `*.intent_alignment.contract.json` only when user explicitly asked for intent-alignment

### 单文件 source set

- DSL source entry: `<name>.plc`

### 多文件 source set

- DSL source entry: `<name>.bundle.toml`
- 关键 fragments: `topology` / `constraints` / `tasks`

## 状态词

明确使用以下状态之一：
- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `blocked by toolchain limitation`
- `failed validation`

没有真实工具运行结果时，状态应与实际执行深度一致。

## 必须额外说明的边界

如果这次交付涉及 intent-alignment，最终回答必须明确写出：
- `*.intent_alignment.contract.json` 是否被创建或修复
- 这个 contract 是 authored sidecar，不是编译默认产物
- `project-check` 是否真的跑到了 `intent_alignment` 步骤
- `intent_alignment/report.json`、`sil_trace.jsonl` 等是否是工具链产物

如果这次交付不涉及 intent-alignment，也要明确说明“未生成 intent sidecar，验证链仅覆盖基础 gate”。

## optimization 请求时

如果用户问 optimization，输出里必须明确：
- 当前是 library API，不是 CLI
- 支持哪些 candidate rewrite kind
- 当前是否真的运行了 API
- 还是只在说明能力边界而未执行
