# 本轮研究程序

## 研究问题

`plc-gen` 是否已经具备最小但稳定的 Day-1 公开工件面，让一个不接触 RustPLC 源码的执行者只拿到：

- 真实 `plc-gen` skill
- 一份已确认的 `plc/main.system.md`
- 显式导出的 Day-1 辅助工件

就能回答“如何 scaffold 项目、生成或修复 `plc/main.plc`，并给出真实验证链”。

## 当前假设

`plc-gen` 当前真正缺的不是更多 reference，而是一个围绕“已确认 `.system.md` -> scaffold -> `plc/main.plc` -> validation”的 task-specific 公开面；如果把 launcher 选择、system contract gate、文件顺序和验证顺序显式导出，并在 skill 本体里补齐“已给 `.system.md` 时不要重新发散需求”的规则，盲测执行者就更容易稳定交付 Day-1 答案。

## 对照基线

- baseline skill / baseline 工件：
  只有 `plc-gen` 本体，没有本地 `.skill_flywheel/` 配置，也没有 Day-1 公开工件。
- 本轮期待看到的差异：
  盲测执行者不再需要自己翻 `references/` 才知道 launcher、文件顺序、验证顺序和 system contract gate。

## 固定边界

- 盲测模式固定为 `weak-blind`
- 盲测执行者默认只能读取目标 skill、`context/` 与显式导出的 `public/`
- 不允许把仓库普通 `docs/` 或 `references/` 直接当作盲测输入

## 并行设置

- 并行实例数：1
- 每个实例共享的固定输入：同一份 `plc-gen`、同一份 Day-1 任务、同一组 `public/`
- 每个实例允许变化的因素：无
- 实例之间禁止共享的内容：不适用，本轮不并行

## 随机性控制

- 本轮接受的随机性来源：单代理自检带来的宿主上下文污染
- 不允许不同任务模板混跑
- 本轮不把 `weak-blind` 结果当作高可信 clean-room 证据

## 任务选择

- 本轮使用的真实任务：
  给一个像 `wafer_loader.system.md` 这样的确认版 system contract，判断陌生用户 Day-1 应如何 scaffold、改哪些文件、哪些项记为 assumptions，以及按什么顺序验证。
- 为什么这个任务能验证当前假设：
  这是 `plc-gen` 面向陌生用户最常见、也最容易因公开面缺失而失稳的路径。

## 成功信号

- `init_public_surface.py` 能在 `plc-gen` 上正常初始化 cycle
- 导出的 `public/` 只包含少量 Day-1 辅助工件，而不是原始 reference 文档
- 盲测执行者仅靠这些工件就能知道：
  - 先判断 launcher
  - 先看 `plc/main.system.md` 再写 `plc/main.plc`
  - 已确认 `.system.md` 不应重新发散成问卷
  - 最小验证链至少是 `scenario-validate` + `scenario-doctor`

## 失败信号

- `public_surface.json` 与 `init_public_surface.py` 不兼容
- Day-1 任务仍需要越界读取 `references/` 或仓库普通文档
- skill 仍无法稳定约束“已确认 `.system.md` 时少追问、先交付”

## 决策规则

- 如果属于 `skill-gap`：补 `plc-gen` 本体中的回答顺序、`.system.md` 优先级和 assumptions 规则
- 如果属于 `public-surface-gap`：补 `.skill_flywheel/public/` 工件，不再依赖盲测执行者自己翻 reference
- 如果属于 `code-gap`：补 `skill-flywheel` 导出脚本或 `plc-gen` 的稳定对外契约
- 如果属于 `task-ambiguity`：收窄 Day-1 任务，只保留“确认版 `.system.md` -> scaffold / validate”主路径

## 冲突证据处理

- 如果多实例结论冲突：先视为证据不足，本轮不升格为 skill 结论
- 如果证据不足：只允许补最小公开工件或更窄任务，不继续泛化

## 停止条件

- `plc-gen` 的本地 flywheel 配置升级到当前协议
- 至少完成两轮弱盲研究记录
- `init_public_surface.py` 与 `test_plc_gen_target_config.py` 均通过

## 预算

- 最大轮数：3
- 最大并行实例数：1
- 连续 1 轮没有新证据就停止
