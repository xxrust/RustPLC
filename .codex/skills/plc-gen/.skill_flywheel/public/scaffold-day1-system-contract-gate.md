# Scaffold Day-1 System Contract Gate

`plc-gen` 的 Day-1 主路径只回答一个问题：

> 用户给出的 `.system.md` 是否已经足够让我们 scaffold 并开始生成真实 delivery asset source set，而不是只停在 scaffold 占位项目？

## 可以直接进入 delivery asset authoring 的信号

满足大部分即可：

- 已经给出一份确认版 `.system.md` 或等价 system contract
- authoritative source 已按正确编码读取；若含中文等非 ASCII 内容，默认显式按 UTF-8 读取
- task 划分已经明确
- blocking step 语义已经明确
- 关键 fault / recovery route 已经明确
- 关键共享资源或互锁边界已经明确
- 基本能判断 delivery layer 是 `module`、`station` 还是 `line`

像 `wafer_loader.system.md` 这种已经写清 task、blocking、mode、fault、startup/stop 的输入，默认应直接消费，而不是重新回退到大问卷。

对 scaffold 项目，这意味着：

- 可以先替换 root `plc/main.system.md`
- 可以先替换 delivery asset `docs/*.system.md`
- 可以开始写 delivery asset `main.bundle.toml` 与 fragments

如果 `.system.md` 出现乱码，不要继续猜 contract；先修正读取编码，再继续 authoring。

## 仍应作为 assumptions / blockers 的内容

只保留真正会改变 source set 结构或验证路径的未决项，例如：

- `待联调冻结项`
- 仍未冻结的 axis timeout / retry / route
- 缺失的关键 sensor / actuator
- 不明确的 fault 目标 task
- 无法判断 source shape 是否必须引入 workpiece / intent / multi-asset 拆分的关键边界

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

## complex delivery 的额外提醒

confirmed `.system.md` 足够进入项目生成，不代表 scaffold 默认 docs / intent sidecar 可以原样保留。

如果输入已经确认，下一步应是：

1. authoring delivery asset docs
2. 明确 source entry
3. 再写 fragments / scenario / intent sidecar

而不是把占位项目直接当结果返回。
