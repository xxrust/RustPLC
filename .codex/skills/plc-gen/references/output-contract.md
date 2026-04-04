# plc-gen Output Contract

本文件约束最终回答应长什么样。

## 最低输出

始终返回：
- 结果摘要
- 生成或修复后的 `.plc` / 项目结果
- assumptions
- 实际使用或推荐的 launcher / 命令
- validation 状态

## 项目级请求时额外返回

按需补充：
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`
- 最小验证命令链
- 当前 gate / codegen / build 状态

## 状态词

明确使用以下状态之一：
- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `blocked by toolchain limitation`
- `failed validation`

没有真实工具运行结果时，不要暗示 `validated`。

## optimization 请求时

如果用户问 optimization，输出里必须明确：
- 当前是 library API，不是 CLI
- 支持哪些 candidate rewrite kind
- 当前是否真的运行了 API
- 还是只在说明能力边界而未执行
