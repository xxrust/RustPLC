# plc-gen Output Contract

本文约束最终回答应长什么样。

## 最低输出

始终返回：
- 结果摘要
- 生成或修复后的 DSL source set / 项目结果
- assumptions
- 实际使用或推荐的 launcher / 命令
- validation 状态

## 项目级请求时额外返回

按需补充：
- `plc/main.system.md`
- DSL source entry
- 如果采用 bundle，则补充 `.bundle.toml` 与关键 fragments
- `scenarios/nominal/normal.yaml`
- 最小验证命令链
- 当前 gate / codegen / build 状态

## source set 表达方式

根据实际交付形态表达结果：

### scaffold 默认布局

- system contract: `plc/main.system.md`
- DSL source entry: `plc/main.plc`
- scenario: `scenarios/nominal/normal.yaml`

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

## optimization 请求时

如果用户问 optimization，输出里必须明确：
- 当前是 library API，不是 CLI
- 支持哪些 candidate rewrite kind
- 当前是否真的运行了 API
- 还是只在说明能力边界而未执行
