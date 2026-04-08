# 本轮研究程序

## 研究问题

`plc-gen` 现在虽然已经引入了 `public brief`、one-shot 和多 agent 编排，但这些新规则是否已经暴露成盲测执行者可直接消费的公开工件，而不是仍然主要埋在 skill / references 里。

## 当前假设

当前最主要的缺口不是继续扩写 `plc-gen` 本体，而是缺少一份显式导出的 “complex-project public brief contract”。

如果把下面这件事公开导出：

- 复杂项目里主 agent 必须先准备什么 `public brief`
- brief 至少包含哪些字段
- architect / implementer / reviewer 各自基于 brief 做什么

那么一个看不到源码的执行者就不需要翻 `references/` 才能理解这套 one-shot 编排。

## 对照基线

- baseline skill / baseline 工件：
  `plc-gen` 已写入 one-shot / public brief 规则，但 `.skill_flywheel/public/` 还没有专门的 public brief 工件。
- 本轮期待看到的差异：
  弱盲执行者只靠真实 `plc-gen` + 导出的 `public/`，就能回答复杂项目下如何准备 public brief、如何拆分角色、哪些内容属于 authored artifacts。

## 固定边界

- 本轮盲测模式固定为 `weak-blind`
- 盲测执行者默认只能读取目标 skill、`context/` 与显式导出的 `public/`
- 不允许把仓库普通 `docs/`、`src/` 或其他实现文件当作盲测输入

## 并行设置

- 并行实例数：1
- 每个实例共享的固定输入：同一份 `plc-gen`、同一份 complex-project 任务、同一组 `public/`
- 每个实例允许变化的因素：无
- 实例之间禁止共享的内容：不适用，本轮不并行

## 随机性控制

- 本轮接受的随机性来源：单代理 fallback 的宿主上下文污染
- 不允许更换任务模板
- 不把本轮结果写成 `clean-room`

## 任务选择

- 本轮使用的真实任务：
  给一个复杂项目请求，包含确认版 `.system.md`、多文件 bundle 倾向、scenario/gate 约束，以及可选 intent-alignment；要求回答“主 agent 必须先准备什么 public brief，再如何 one-shot 拆给 architect / implementer / reviewer”。
- 为什么这个任务能验证当前假设：
  因为它正好验证 `plc-gen` 最新引入、也最容易重新藏回 skill 内部的那部分能力。

## 成功信号

- 导出的 `public/` 中存在专门的 public brief 工件，而不是只靠 skill / references 内文
- 弱盲执行者能回答：
  - public brief 至少包含什么
  - architect / implementer / reviewer 如何基于 brief 交接
  - 哪些是 skill 写入物，哪些是工具链产物
- `test_plc_gen_target_config.py` 通过

## 失败信号

- 仍然必须翻 `references/public-brief-template.md` 才能答题
- 仍然把 public brief 结构藏在主 skill 规则里，没有显式 public artifact
- target config 测试未覆盖新工件

## 决策规则

- 如果属于 `skill-gap`：只补 `plc-gen` 本体里缺失的选择规则
- 如果属于 `public-surface-gap`：补 `.skill_flywheel/public/` 与 `public_surface.json`
- 如果属于 `code-gap`：补 `skill-flywheel` 导出脚本或目标测试
- 如果属于 `task-ambiguity`：收窄任务，只保留 complex-project public brief 主路径

## 冲突证据处理

- 如果多实例结论冲突：视为证据不足，本轮不升格成 skill 结论
- 如果证据不足：只允许补更窄 public artifact，不继续泛化

## 停止条件

- public brief contract 已进入 `.skill_flywheel/public/`
- target config 测试覆盖新工件
- 完成一轮弱盲 cycle 并写出 decision

## 预算

- 最大轮数：1
- 最大并行实例数：1
- 连续 1 轮没有新证据就停止
