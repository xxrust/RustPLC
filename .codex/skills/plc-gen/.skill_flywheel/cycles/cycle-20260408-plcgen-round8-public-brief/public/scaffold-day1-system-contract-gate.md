# Scaffold Day-1 System Contract Gate

`plc-gen` 的 Day-1 主路径只回答一个问题：

> 用户给出的 `.system.md` 是否已经足够让我们 scaffold 并开始生成 `plc/main.plc`？

## 可以直接进入 `.plc` 生成的信号

满足大部分即可：

- 已经给出一份确认版 `.system.md` 或等价 system contract
- task 划分已经明确
- blocking step 语义已经明确
- 关键 fault / recovery route 已经明确
- 关键共享资源或互锁边界已经明确

像 `wafer_loader.system.md` 这种已经写清 task、blocking、mode、fault、startup/stop 的输入，默认应直接消费，而不是重新回退到大问卷。

## 仍应作为 assumptions / blockers 的内容

只保留真正会改变 `.plc` 结构或验证路径的未决项，例如：

- `待联调冻结项`
- 仍未冻结的 axis timeout / retry / route
- 缺失的关键 sensor / actuator
- 不明确的 fault 目标 task

这些项要明确标成：

- `assumptions`
- 或 `blocked by missing contract`

不要假装它们已经确认。

## 不该重新追问的内容

以下内容通常不该在确认版 `.system.md` 面前重新发散：

- 占位 I/O 名称
- 中性的 device 名称
- 保守的初始 timeout 数值
- nominal scenario 的起始 timing

默认做法是先交付 Day-1 版本，再把这些作为 assumptions 写清。
