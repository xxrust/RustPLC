---
name: skill-flywheel
description: 用于把 skill 改进组织成可重复的研究回合：设定实验程序、在真实任务中盲测、归因问题，并据此持续迭代 skill。
---

# skill-flywheel

把 `skill-flywheel` 当作研究型编排 skill，而不是项目私有知识库或普通任务执行器。

它的职责不是完成某个固定 feature，而是围绕目标 skill 持续回答四个问题：

- 这个 skill 当前最值得验证的假设是什么
- 在真实任务里它具体卡在哪里
- 这些问题属于 skill、辅助工件、代码还是任务本身
- 下一轮最小改动和下一轮实验分别是什么

目标 skill 的局部研究配置放在它自己的 `.skill_flywheel/` 中，不要写进本 skill。

按需加载这些参考文档：

- `references/program.md`
  如何把一轮 flywheel 写成可执行的研究程序。
- `references/parallel.md`
  如何在同一轮里并行运行多个盲测实例，并汇总结果。
- `references/workflow.md`
  单轮实验协议、执行顺序、继续条件与停止条件。
- `references/boundary.md`
  说明本 skill 中“禁止读源码”的具体边界，以及允许和禁止读取的范围。
- `references/classification.md`
  如何把痛点归类为 skill、辅助工件、代码或任务本身的问题。
- `references/local-target-config.md`
  目标 skill 中的 `.skill_flywheel/` 应该放什么。

按需加载这些 agent 角色文件：

- `agents/skill-editor.md`
  可读源码的 skill 改进者。
- `agents/blind-runner.md`
  禁止读源码的盲测执行者。
- `agents/root-cause-analyst.md`
  可读源码的根因分析者。
- `agents/synthesizer.md`
  多实例聚合者。

## 核心规则

1. 每一轮都先写清研究问题、当前假设、成功信号和停止条件，再开始执行。
2. 不要把回合固定死在“三个 agent”上。真正固定的是最小角色集合：
   - 至少 1 个盲测执行者
   - 至少 1 个可读源码的根因分析者
   - 只有在结论明确属于 `skill-gap` 时，才需要 skill 改进者
3. 当任务存在随机性、路径分叉或上下文噪声时，优先并行运行多个盲测实例，而不是只看单次观察。
4. 并行实例应共享同一份 `program.md` 和同一任务，但彼此独立记录观察，不要先互相对齐结论。
5. 盲测模式必须显式区分：
   - `clean-room`：由外部编排器启动全新顶层对话，只注入本轮允许的工件。这是默认推荐模式。
   - `weak-blind`：在当前会话内启动子 agent 或等价实例，不继承完整上下文，但仍可能受到系统指令、父 prompt 和共享工作区影响。这只能用于低成本探索，不能当作高可信盲测。
6. 如果一轮结论将决定是否修改核心 skill，默认应使用 `clean-room`，不要把 `weak-blind` 结果直接当成最终证据。
7. 如果当前环境无法或不应启动子 agent，不要卡住；改用单代理 fallback，但必须把本轮明确标记为 `weak-blind`。
   - 先按 blind-runner 边界，只读取目标 skill、`context/` 和显式导出的 `public/`
   - 先写完 `logs/pain-points.*`，再解除边界做 `logs/root-cause.*` 与 `logs/decision.*`
   - 不要把单代理 fallback 的结果写成 `clean-room`
   - 如果本轮原计划并行，但实际只跑了单代理 fallback，要把“未执行并行实例”记入决策，而不是假装已有并行证据
8. 默认公开物只有真实目标 skill；只有显式导出的辅助工件，才额外放进 `public/`。
9. Agent 2 只依赖目标 skill 和显式导出的 `public/` 辅助工件；如果必须读取仓库其他文件才能完成任务，应记为发现，不能越界读取。
10. 优先补稳定、显式导出的辅助工件，例如命令帮助、manifest、诊断、报告，而不是把私有推理写进 skill。
11. 目标 skill 保持精简。可从仓库稳定导出的事实，不继续堆进 skill。
12. 所有判断尽量落盘到 cycle 工件，不依赖会话记忆。

## 默认流程

1. 先读 `references/program.md`、`references/parallel.md` 和 `references/workflow.md`。
2. 检查目标 skill 旁边是否存在 `.skill_flywheel/`。
3. 如果存在，读取其中的 `program.md`、`public_surface.json`、`profile.md`、可选的 `.skill_flywheel/public/`，以及需要的任务模板。
4. 用局部 `program.md` 明确本轮的：
   - 研究问题
   - 当前假设
   - 成功信号
   - 停止条件
5. 运行 `scripts/init_public_surface.py` 创建 cycle 目录。未显式指定时，默认写入目标 skill 的 `.skill_flywheel/cycles/`。其中至少包含：
   - `public/`
   - `logs/`
   - `prompts/`
   - `context/`
6. 如果当前环境允许并且本轮证据强度要求足够高，读取 `agents/` 下的角色文件，结合 `prompts/` 和 `context/` 启动本轮研究闭环。
7. 如果当前环境不允许或不适合启动子 agent，改走单代理 fallback：
   - 先用 blind-runner 边界完成观察并写 `logs/pain-points.*`
   - 再切回可读源码视角完成 `logs/root-cause.*` 与 `logs/decision.*`
   - 明确把本轮标注为 `weak-blind`
8. 如果需要并行盲测，就复制 `agent2` 角色到多个独立实例，并让它们分别写各自的观察记录。
9. 汇总盲测观察，再阅读 `logs/pain-points.md`、`logs/root-cause.md` 和 `logs/decision.md`。
10. 把最小修复落在正确层级：
   - 目标 skill
   - 显式导出的辅助工件
   - 代码库对外契约 / 工具 / 诊断
11. 只有当 `logs/decision.md` 明确写出“进入下一轮”时，才继续下一轮；否则停止并保留本轮结论。

## 最低完成标准

不要停留在空结论。至少要产出：

- 一份本轮研究程序副本：`context/program.md`
- 一个辅助工件包，或一个更好的辅助工件导出方案
- 至少一份盲测观察；如果本轮启用了并行实例，则要有可比较的多份观察
- Agent 3 给出的根因判断
- 一份写明“假设是否成立、下一轮是否继续”的决策记录
- 如果问题属于 `skill-gap`，则给 Agent 1 一份最小 skill 改动清单
