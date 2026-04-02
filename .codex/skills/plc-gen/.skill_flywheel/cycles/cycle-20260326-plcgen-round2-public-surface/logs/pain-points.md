# 痛点记录

任务：
# PLC Scaffold Day-1

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `plc/main.system.md`，而且这份 contract 的复杂度接近 `wafer_loader.system.md`，应该如何先指导一个不接触 RustPLC 源码的新手完成 Day-1 scaffold、`plc/main.plc` 生成或修复，以及最小验证？

观察点：

- 是否先判断 launcher，而不是直接甩一串 `cargo run` 命令
- 是否把 `.system.md` 当作主输入，而不是重新发散成大问卷
- 是否能说清 scaffold 后先改哪些文件
- 是否能把未冻结 contract 留在 assumptions / blockers，而不是默默补齐
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


## 结果

基于新导出的 `public/` 工件，盲测执行者已经可以不读 `references/` 就回答出：

- launcher 的二分路径
- source workspace 不能进入 scaffold 目录继续跑 Cargo 的原因
- scaffold 后优先关注 `plc/main.system.md`、`plc/main.plc` 与 `scenarios/nominal/normal.yaml`
- 最低验证门槛是 `scenario-validate` + `scenario-doctor`
- 对确认版 `.system.md` 应默认直接消费，只把未冻结 contract 留在 assumptions / blockers

本轮没有再暴露新的 Day-1 公开面阻塞。

## 假设观察

本轮观察支持当前假设：`plc-gen` 的主要阻塞确实是 Day-1 公开工件面，而不是需要继续堆 reference。

## 痛点

1. 步骤：
   观察到的阻塞：没有新的强阻塞，但本轮证据仍是单代理 `weak-blind`，而且观察与改动几乎同轮发生。
   缺少的工件或说明：无新的 Day-1 工件缺口。
   影响：还不能把这轮直接当成高可信“已通过盲测”的最终证据，需要再跑一轮验证性 cycle。

2. 步骤：
   观察到的阻塞：需要确认 `plc-gen` 本体中的新规则是否已经足够稳定地约束回答顺序，而不只是靠公开工件兜底。
   缺少的工件或说明：更明确的最终停止条件与验证结论。
   影响：需要进入下一轮，把问题收窄成“skill + public surface 一起是否稳定闭环”。
