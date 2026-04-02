# System Day-1 Handoff Gate

只有在下面这些关键项都明确后，才允许 handoff 给 `plc-gen`：

- topology shape
- safety constraints
- task structure
- blocking 预期
- timeout strategy
- fault / recovery tasks
- scenario 与 validation baseline
- 若存在 axis，则 axis parameter 与 fault policy

收口句式固定为：

```text
系统 contract 已确认。继续进行 `.plc` generation。
```

如果以上关键项还没明确，就不要使用这句 handoff。
