# 根因分析

任务：
为 `skill-flywheel` 自己初始化一轮新的研究回合，并在不启动子 agent 的前提下完成一次最小 `weak-blind` 闭环。

要求：

1. 使用提供的命令初始化一个新的 cycle。
2. 盲测观察阶段只允许读取：
   - 目标 skill 的 `SKILL.md`
   - 新 cycle 下的 `context/`
   - 新 cycle 下的 `public/`
3. 基于这些输入，手工完成：
   - `logs/pain-points.md`
   - `logs/pain-points.json`
   - `logs/root-cause.md`
   - `logs/root-cause.json`
   - `logs/decision.md`
   - `logs/decision.json`
4. 如果当前流程缺少单代理 fallback、证据强度标注或闭环步骤说明，把它记录成痛点。
5. 不要把本轮结果写成 `clean-room` 通过；如果只是当前会话内完成的观察，必须明确标记为 `weak-blind`。


## 假设判断

削弱。

## 结论

1. 痛点：公开初始化命令仍然指向旧的 `self-smoke.md`。
   分类：`public-surface-gap`
   原因：`.skill_flywheel/public/` 中导出的命令工件仍然是上一轮 smoke 自测用的静态文件，当前 cycle 会原样复制这些文件，但没有针对当前任务切换为新的命令说明。
   最小修复：新增面向 `single-agent-closeout.md` 的公开命令与清单工件，并把它们纳入 `.skill_flywheel/public_surface.json`；旧 smoke 工件保留为显式命名的历史任务工件，不再假装是当前轮的通用入口。

2. 痛点：单代理 / 无子 agent 时的 fallback 没有写成稳定协议。
   分类：`skill-gap`
   原因：`SKILL.md`、`references/workflow.md` 与 `references/boundary.md` 都强调了 blind-runner、analyst 和 `weak-blind` / `clean-room` 的边界，但没有明确规定“如果当前回合不能或不应启动子 agent，该怎样按顺序手工完成 blind-runner 观察、root-cause 和 decision，并如何标记证据强度”。
   最小修复：在 `SKILL.md` 与相关参考文档里补一套最小单代理 fallback 协议，明确：
   - 先按 blind-runner 边界只读目标 skill、`context/`、`public/`
   - 先写 `logs/pain-points.*`
   - 再解除边界做 root-cause / decision
   - 本轮必须显式标记为 `weak-blind`
   - 不能把这类结果写成 `clean-room`
