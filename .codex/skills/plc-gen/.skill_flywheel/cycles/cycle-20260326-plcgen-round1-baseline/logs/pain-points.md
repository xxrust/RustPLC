# 痛点记录

任务：
确认 `plc-gen` 在没有本地 flywheel 公开工件面的情况下，是否足以支撑 `wafer_loader.system.md` 级别的 Day-1 项目生成。

## 结果

基线观察表明，执行者能看出这是“项目级请求，优先 scaffold”这类大方向，但无法只靠 `plc-gen` 本体稳定回答：

- 到底该用 `rust_plc` 还是 `cargo run --release --bin rust_plc --`
- source workspace 模式下为什么不能 `cd` 进 scaffold 目录
- 给定确认版 `.system.md` 时，哪些内容应直接消费，哪些内容应留在 assumptions
- 最小验证链的固定顺序是什么

## 假设观察

本轮观察支持“`plc-gen` 主要缺的是 Day-1 公开工件面，同时存在一个较小的 `.system.md` ingest skill-gap”这一假设。

## 痛点

1. 步骤：判断 launcher 与命令链
   观察到的阻塞：盲测执行者知道要读 `references/commands.md`，但在禁止越界读取时拿不到 launcher 矩阵，也无法直接解释 source workspace 的工作目录规则。
   缺少的工件或说明：Day-1 launcher 工件、validation order 工件。
   影响：对陌生用户给出的命令不稳定，容易误写成 `cargo run --release -- ...` 或误导用户进入 scaffold 目录后继续跑 Cargo。

2. 步骤：把确认版 `.system.md` 收口到 Day-1 回复
   观察到的阻塞：skill 只说“意图不稳定时先依赖 system contract”，但没有反向说清“意图已经稳定时就直接消费 `.system.md`，不要再发散提问”。
   缺少的工件或说明：system contract gate，以及对 assumptions / blockers 的更明确规则。
   影响：面对 `wafer_loader.system.md` 这类输入时，执行者仍可能重开问卷，导致 Day-1 路径失稳。
