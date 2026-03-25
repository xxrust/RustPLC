# plc-system Handoff

当把已确认的 contract 交给 `plc-gen` 时，使用本文件。

## 完成后的 `.system.md` 必须让 PLC generation 明确决定：

- topology shape
- safety constraints
- task structure
- timeout strategy
- failure tasks
- scenario 与 validation baseline

## 结束状态

结尾附上这句简短说明：

```text
系统 contract 已确认。继续进行 `.plc` generation。
```
