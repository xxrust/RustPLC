# plc-gen Output Contract

用本文件约束最终响应形式。

## 最低交付要求

始终返回：

- 生成或修复后的 `.plc`
- 简短的 assumptions 列表
- 实际使用或推荐的 launcher 与命令
- validation 结果

## 项目级请求时

按需返回：

- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`
- 最小可执行验证命令链
- 当前 validation 状态

## Validation 状态

明确使用以下状态之一：

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `failed validation`

没有真实 tool 运行结果时，不要暗示成功。
