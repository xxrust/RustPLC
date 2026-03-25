# plc-system Handoff

当 `.system.md` 起草完成后，用本文件保证交给 `plc-gen` 的信息足够闭合。

## handoff 前必须明确的内容

- topology shape
- safety constraints
- task structure
- blocking 预期
- timeout strategy
- fault / recovery tasks
- scenario 与 validation baseline
- 若存在 axis，则 axis parameter 与 fault policy

## handoff 句式

结尾用一句简短、明确的话收口：

```text
系统 contract 已确认。继续进行 `.plc` generation。
```

如果以上关键项还没明确，就不要使用这句 handoff。
