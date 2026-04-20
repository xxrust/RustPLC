# 本轮研究程序

## 研究问题

给定确认版 `examples/three_station_assembly.system.md`，`plc-gen` 当前的 skill 与公开工件，是否已经足够让弱盲执行者：

- scaffold 一个真实的 station 级 structured-fragments 项目
- 把 confirmed system 真正下沉到 delivery asset docs / bundle / fragments
- 运行真实 `project-check`
- 并在 intent sidecar 仍是占位状态时正确报告 blocker，而不是把 scaffold 误报成“已生成项目”

## 当前假设

当前主缺口更像 `public-surface-gap`，不是 `plc-gen` 完全不会做项目生成。

更具体地说，公开面很可能缺 3 件事：

- source shape 该如何从 root scaffold 切到 delivery asset `main.bundle.toml`
- 确认版 `.system.md` 进入 scaffold 后，哪些占位 docs/sidecar 必须立刻替换
- 复杂项目下 intent sidecar 什么时候是默认要求，而不是“有空再说”

## 对照基线

- baseline skill / baseline 工件：
  `plc-gen` 主 skill 已要求 structured-fragments、delivery asset、`project-check` 与复杂项目默认 intent alignment。
- 本轮期待看到的差异：
  盲测执行者只靠真实 `plc-gen` + 导出的 `public/`，就能从 confirmed `.system.md` 走到真实项目交付链，而不是停在 scaffold 默认壳子。

## 固定边界

- 本轮盲测模式固定为 `weak-blind`
- 盲测执行者默认只读取目标 skill、`context/` 与显式导出的 `public/`
- 不允许把仓库普通 `docs/`、`src/` 或其他实现文件当作盲测输入
- 研究对象固定为 `examples/three_station_assembly.system.md`

## 并行设置

- 并行实例数：2
- 每个实例共享的固定输入：同一份 `plc-gen`、同一份 confirmed `.system.md`、同一组 `public/`
- 每个实例允许变化的因素：实现者自己的作者化路径与命令顺序
- 实例之间禁止共享的内容：彼此的输出目录与中途结论

## 随机性控制

- 本轮接受的随机性来源：执行者自己的 authoring 选择与工具链失败路径
- 不允许更换任务模板
- 不把本轮结果写成 `clean-room`

## 任务选择

- 本轮使用的真实任务：
  从 `examples/three_station_assembly.system.md` 生成一个 station 级 structured-fragments RustPLC 项目，并尽量跑通 `project-check`。
- 为什么这个任务能验证当前假设：
  因为它同时覆盖了 source-shape 选择、confirmed-system 下沉、delivery docs/intent sidecar authoring、project-check 入口，以及“不能把 scaffold 占位物当交付”的整条链路。

## 成功信号

- blind runner 产出真实的 delivery asset docs，而不是停留在 scaffold 默认文案
- delivery asset `main.bundle.toml` 与 scenario 真正成为执行入口
- intent sidecar 要么被真实 authoring，要么被显式报告为 blocker
- `test_plc_gen_target_config.py` 通过

## 失败信号

- 执行者只成功 scaffold，但没有替换 delivery asset 占位 docs
- 执行者仍把 root `plc/main.system.md` 当成复杂项目唯一需要修改的文件
- 执行者把 scaffold 占位 intent contract 当成可验证 sidecar
- target config 仍声明导出不存在的 public artifact

## 决策规则

- 如果属于 `skill-gap`：补 `plc-gen` 本体中的硬规则或 workflow
- 如果属于 `public-surface-gap`：优先补 `.skill_flywheel/public/`、`profile.md` 与 target config 测试
- 如果属于 `code-gap`：补 `skill-flywheel` 导出脚本或目标测试
- 如果属于 `task-ambiguity`：收窄任务，只保留“confirmed system -> delivery asset project-check”主路径

## 冲突证据处理

- 如果多实例结论冲突：视为证据不足，本轮不升格成 skill 结论
- 如果证据不足：只允许补更窄、更直接的 public artifact，不继续泛化

## 停止条件

- 公开工件已能覆盖这轮真实任务的主路径
- target config 测试通过
- 至少完成一轮修复后复跑，并拿到“不是只 scaffold 成功”的证据

## 预算

- 最大轮数：1
- 最大并行实例数：2
- 连续 1 轮没有新证据就停止
