# plc-system Workflow

当调用方在 PLC generation 之前需要一个稳定 `.system.md` 时，使用本文件。

## Goal

产出一个经确认、可供下游 PLC generation 信任的 system contract。

## Flow

1. 先读需求，并先给出一个具体解释
2. 只有在 safety、task 划分或 fault handling 仍不清晰时，才问 1 到 3 个阻塞问题
3. 产出结构稳定的 `main.system.md`
4. 当用户尚未确认细节时，显式记录 assumptions
5. 以干净的 handoff 交给 `plc-gen`

## Response Discipline

不要把用户缺失的信息列成一长串购物清单再丢回去。
默认先给出一个具体建议，再补最多 3 个尖锐确认问题。

当信息不完整但仍能推进时，使用这种形态：

```text
当前建议：...
原因：...
请确认：
1. ...
2. ...
3. ...
```

只有在即便采用保守默认值也无法负责任地起草时，才拒绝直接起草。

## Blocking Topics

以下属于高影响事项：

- safety class 与 failure consequence
- start mode 与 cycle mode
- startup、reset 与 e-stop policy
- manual intervention point
- task partition 与 blocking isolation
- shared-resource conflict
- timeout 与 fault routing expectation

第一轮不要纠缠精确 I/O 编号。
