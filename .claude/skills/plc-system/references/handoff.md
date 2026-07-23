# plc-system Handoff

`.system.md` 起草完成后，用本文件确认交给 `plc-gen` 的信息是否闭合。

## handoff 前必须明确的内容

- topology shape
- process operation scheduling intent, if the process moves discrete workpieces or needs pipelined admission/resource policy
- safety constraints
- task structure
- blocking 预期
- timeout strategy
- fault / recovery tasks
- scenario 与 validation baseline
- 若存在 axis，则明确 axis parameter 与 fault policy

## process operation handoff

如果存在工艺操作调度意图，handoff 必须要求 `plc-gen` 在生成 task/step 之前先将其物化为源侧 TOML：

```text
process_model/process_operation_model.toml
```

不要默认交付 `out/process_operation_model.json`。`out/` 表示可重建产物目录，而工艺操作模型是拓扑之后、程序流之前的源侧调度知识，应放在 `process_model/`，不要伪装成编号编译片段。

正确顺序是：

```text
system contract -> process_model/process_operation_model.toml -> task/step -> process-model-check
```

handoff 必须要求下游运行：

```bash
rust_plc process-model-check <source.plc|source.bundle.toml> --model process_model/process_operation_model.toml
```

若出现 `OP-002`，说明 task/step 把候选工艺操作无依据串行化，应回到调度意图层修正，而不是只改 step 名称或注释。

`operation-model` 只能服务于已有 task/step 的迁移、审计或差异对照；它从当前 task/step 反推模型，不能作为新项目的默认前置生成步骤，也不能替代人工确认后的源侧调度契约。

## handoff 句式

结尾用一句简短、明确的话收口：

```text
系统 contract 已确认。继续进行 `.plc` generation。
```

如果以上关键项还没明确，就不要使用这句 handoff。
