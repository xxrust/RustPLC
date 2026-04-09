# Delivery Status Contract

## Additional Status Rule

Do not report `validated` when the sidecar still contains:
- `replace_me_after_authoring`
- unresolved source binding
- starter anchors such as `replace_after_intent_doctor`

这个工件只回答一个问题：

> `plc-gen` 的最终回答至少应包含什么，以及状态词、写入物、工具链产物应如何区分？

## 最低输出

始终返回：

- 结果摘要
- 生成或修复后的 DSL source set / 项目结果
- assumptions
- 实际使用或推荐的 launcher / 命令
- validation 状态
- 哪些文件是本次由 skill 写入
- 哪些文件是工具链运行后生成

## 状态词

只使用这 5 个稳定状态词：

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `blocked by toolchain limitation`
- `failed validation`

没有真实工具运行结果时，不要写成 `validated`。

## authored artifacts

常见 authored artifacts：

- `plc/main.system.md`
- `plc/main.plc`
- `<name>.bundle.toml`
- fragments
- `scenarios/nominal/normal.yaml`
- 可选 `*.intent_alignment.contract.json`

## toolchain artifacts

常见 toolchain artifacts：

- `verification_report.json`
- `sil_trace.jsonl`
- `project_check_report.json`
- `intent_alignment/report.json`
- 其他 gate / codegen / build / release 产物

## 项目级请求时额外说明

- DSL source entry 是什么
- 如果采用 bundle，关键 fragments 是哪些
- scenario 是否被创建或修改
- 是否生成可选 intent sidecar
- 当前最小验证链是什么

## 不要做的事

- 不要把 authored sidecar 写成编译器默认产物
- 不要把工具链产物说成“本次写入的源文件”
- 不要在没有真实运行结果时夸大验证状态
