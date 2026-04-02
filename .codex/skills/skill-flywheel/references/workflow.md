# 工作流

## 目标

把目标 skill 的改进过程组织成可重复的研究回合，而不是一次性讨论。

每一轮都应回答：

- 这轮验证的假设是什么
- 盲测观察到了什么
- 根因属于哪一层
- 下一步最小动作是什么
- 是否值得进入下一轮

## 单轮协议

1. 选定目标 skill 和一个真实任务。
2. 写或更新目标 skill 旁边的 `.skill_flywheel/program.md`，明确本轮研究问题、当前假设、baseline、成功信号、随机性控制、预算、停止条件，以及本轮盲测模式是 `clean-room` 还是 `weak-blind`。
3. 查找目标 skill 旁边的 `.skill_flywheel/`。
4. 用 `scripts/init_public_surface.py` 根据局部配置创建 cycle 目录；未显式指定时，默认写入目标 skill 的 `.skill_flywheel/cycles/`。
5. 让 Agent 2 默认只依赖真实目标 skill；只有在明确导出辅助工件时，才额外读取 `public/`。
6. 把所有阻塞记录到 `logs/pain-points.md`。
7. 让 Agent 3 检查仓库并对每个阻塞做分类。
8. 如果阻塞属于 `skill-gap`，由 Agent 1 修改目标 skill。
9. 如果阻塞属于 `public-surface-gap`，优先补更好的显式辅助工件，不要让 Agent 2 直接改读仓库普通文件，也不要把 skill 越改越胖。
10. 如果阻塞属于 `code-gap`，就增强真正的对外契约，例如：
   - `--help`
   - manifest
   - 报告
   - 稳定示例
   - 机器可读诊断
11. 把本轮结论写入 `logs/decision.md`：假设是否被支持、证据是否足够、是否值得进入下一轮。
12. 只有在决策记录明确要求时，才使用同一任务或相近任务复跑，验证修复是否真的生效。

## 盲测模式

默认分两档：

1. `clean-room`
   使用外部编排器开启全新顶层对话，只注入本轮允许的工件。这是默认推荐模式。
2. `weak-blind`
   使用当前会话内的子 agent 或等价方式，虽然可能不继承完整历史，但仍共享更大的宿主环境和父级提示。

如果你打算依据这轮结果修改核心 skill，请优先使用 `clean-room`。

## 单代理 Fallback

如果当前环境无法或不应启动子 agent，不要让回合停在空模板。改用单代理 fallback，但必须显式承认这只是 `weak-blind` 证据：

1. 先把自己约束在 blind-runner 边界内，只读取目标 skill、`context/` 与显式导出的 `public/`。
2. 先完成 `logs/pain-points.md` 与必要的结构化观察记录，不要提前读取仓库普通文件补流程。
3. 再切回可读源码视角，完成 `logs/root-cause.md` 与 `logs/decision.md`。
4. 在 `decision` 中写清本轮为何采用单代理 fallback，以及这会怎样降低证据强度。
5. 不要把这种结果写成 `clean-room`，也不要把缺失的并行实例当作已经存在的证据。

## 并行回合

如果本轮要并行跑多个 blind-runner，协议应改成：

1. 先在 `program.md` 里写清并行实例数、实例差异来源和共享输入。
2. 给每个 blind-runner 一份独立实例输出，不要共写同一份观察日志。
3. 先做实例级观察，再做跨实例聚合。
4. 只有聚合完成后，才允许 analyst 和 skill-editor 下结论。

多实例并行的关键不是“多开几个 agent”，而是让每个实例保持独立证据链。

如果并行实例本身也只是 `weak-blind`，那么它们只能用于发现模式，不能替代真正的 clean-room 复验。

## 默认顺序

默认按这个顺序执行：

1. Agent 1 简短审阅当前目标 skill 和 `program.md`，只做必要准备。
2. Agent 2 在尽量新的上下文中执行盲测任务。
3. Agent 3 做根因分析，并给出本轮决策建议。
4. 只有当 Agent 3 明确判定为 `skill-gap` 时，Agent 1 才做最小 skill 改动。

不要让 Agent 3 重做整份 skill 设计。它的职责是分类和给出建议。

## 工件约定

每一轮 cycle 至少应留下：

- `manifest.json`
- `context/program.md`
- `logs/pain-points.md`
- `logs/root-cause.md`
- `logs/decision.md`
- `context/task.md`
- 可选的 `logs/agent1-feedback.md`
- 可选的 `context/profile.md`
- 如果启用并行实例，建议额外留下：
  - `logs/run-index.json`
  - `logs/synthesis.md`
  - `logs/synthesis.json`
  - `logs/runs/run-*.md`

记录要尽量具体：

- 这轮原本在验证什么
- 卡在哪一步
- 观察到的失败现象
- 缺了什么工件或能力
- 建议把修复落在哪一层
- 下一轮是否值得继续

## 聚合与裁决

如果存在多个 blind-runner，不要直接拿任一实例的观察当全局结论。至少要区分：

1. 多数实例重复出现的共性问题
2. 只出现在个别实例里的噪声或偶然路径
3. 多实例互相冲突、需要回到 `program.md` 重新收窄的问题

只有当问题在聚合后仍然稳定，才值得进入 `skill-gap` 或 `public-surface-gap` 修复。

## 继续条件

只有满足下面任一条件时，才建议继续下一轮：

1. 本轮已经得到新的、可操作的证据。
2. 下一轮的研究问题比本轮更窄、更清晰。
3. 你已经把应落在本轮的最小修复落到正确层级。

如果只是重复旧观察、没有新假设、或仍然依赖聊天记忆推进，就不要继续下一轮。

## 预算与停止

研究回合除了内容停止条件，还应有硬预算：

1. 最大轮数
2. 最大并行实例数
3. 连续多少轮没有新证据就停止
4. 连续多少轮只发现同类 `public-surface-gap` 就停止

如果 `program.md` 没写这些预算，默认不要无限继续。

## 防臃肿规则

在改目标 skill 之前，先问三个问题：

1. 这个缺失信息是不是跨很多任务都稳定存在？
2. 它能不能从仓库、CLI 或生成工件里直接导出？
3. 如果写进 skill，会不会和应当由导出工件承载的信息重复？

如果第 2 个问题的答案是“能”，优先补导出工件。
