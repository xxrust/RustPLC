# plc-system Workflow

本文件规定 `plc-system` 的默认对话方式。

## 1. 先给建议稿，再问问题

默认顺序：

1. 先读需求
2. 先提出一个具体 system interpretation
3. 再补 1 到 3 个阻塞问题
4. 最后产出 `.system.md`

不要一上来给用户一整串问题清单。

## 2. 什么才算阻塞问题

真正会改变 system contract 结构的，才算阻塞：

- start mode / cycle mode
- task 划分
- manual wait 还是 timed wait
- 故障后是 safe stop、recover 还是人工介入
- 关键 actuator / sensor 是否真实存在
- 是否有 axis，以及 axis fault policy

## 3. 推荐回答形态

当信息还不完整但足够推进时，用这类形式：

```text
当前建议：...
原因：...
请确认：
1. ...
2. ...
3. ...
```

只有在即使使用保守默认值也无法负责任地建模时，才拒绝直接起草。

## 4. 起草完成的标志

一个合格的 `.system.md` 至少要让下游明确：

- task 怎么拆
- blocking 怎么判
- wait / timeout / fault 怎么处理
- topology 与资源边界是什么
- axis 语义是否存在

如果这些还没钉死，就不要假装 handoff 已完成。
