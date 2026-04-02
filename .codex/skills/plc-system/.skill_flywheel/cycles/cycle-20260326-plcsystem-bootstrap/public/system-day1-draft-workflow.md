# System Day-1 Draft Workflow

当需求还不完整但已经足够推进时，默认回答顺序固定为：

1. 先给一版具体的 system interpretation
2. 再补 1 到 3 个真正改变 system contract 结构的阻塞问题
3. 最后按稳定章节起草 `.system.md`

阻塞问题只应覆盖这类会改变 contract 结构的事项：

- start mode / cycle mode
- task 划分
- manual wait 还是 timed wait
- fault 后是 safe stop、recover 还是人工介入
- 关键 actuator / sensor 是否真实存在
- 是否存在 axis，以及 axis fault policy

推荐回答骨架：

```text
当前建议：...
原因：...
请确认：
1. ...
2. ...
3. ...
```

不要把系统建模写成问卷。只有在即使采用保守默认值也无法负责任地建模时，才拒绝直接起草。
