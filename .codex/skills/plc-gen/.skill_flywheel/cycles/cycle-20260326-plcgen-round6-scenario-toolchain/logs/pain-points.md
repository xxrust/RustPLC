# 痛点记录

任务：
# PLC Scaffold Day-1

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `plc/main.system.md`，而且这份 contract 的复杂度接近 `wafer_loader.system.md`，应该如何先指导一个不接触 RustPLC 源码的新手完成 Day-1 scaffold、`plc/main.plc` 生成或修复，以及最小验证？

观察点：

- 是否先判断 launcher，而不是直接甩一串 `cargo run` 命令
- 是否把 `.system.md` 当作主输入，而不是重新发散成大问卷
- 是否先给出一轮从 `.system.md` 到 `.plc` 的 lowering 摘要
- 是否能说清 scaffold 后先改哪些文件
- 是否能把未冻结 contract 留在 assumptions / blockers，而不是默默补齐
- 是否能正确处理并发 task、模式矩阵、warning/fault 分流和计数器
- 如果用户要走 scenario 工具链，是否会主动检查并暴露复合 wait guard 的兼容性风险
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 结果

真实运行表明，把复杂 PLC 直接接到当前 scenario 工具链上时，`plc-gen` 过去缺少一个关键判断：

- 当前 DSL 形态是否适合 `scenario-init`
- 当前 DSL 形态是否适合 `scenario-validate`
- 当前 DSL 形态是否适合 `scenario-doctor`

以 `docs/已实现/wafer_loader.plc` 为例，这三个命令都会在 runtime bridge 阶段报：

`unsupported guard expression in supervisor.wait_start: mode_auto == true AND start_button == true`

这说明 `plc-gen` 不能再默认把 scenario 命令链当成无条件推荐项。

## 假设观察

本轮观察支持“`plc-gen` 需要显式处理 scenario 工具链兼容性和 scenario-friendly lowering”这一假设。

## 痛点

1. 步骤：
   观察到的阻塞：真实 scenario 命令链在复合 wait guard 上失败。
   缺少的工件或说明：`scenario-toolchain-limitations.md`
   影响：陌生用户会把工具链限制误解成 PLC 整体错误，或者被反复引导去重跑同一批失败命令。

2. 步骤：
   观察到的阻塞：skill 没有明确告诉执行者“如果必须走当前 scenario 工具链，应优先做 scenario-friendly lowering”。
   缺少的工件或说明：显式的复合 guard 兼容性规则。
   影响：生成结果可能在 DSL 层可行，但在用户最常用的验证路径上立刻撞墙。
