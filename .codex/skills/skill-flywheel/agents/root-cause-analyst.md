请结合任务、盲测执行者的输出，以及必要的仓库源码进行分析。

研究程序：`<PROGRAM_PATH>`
任务：
<TASK>
痛点日志：`<PAIN_POINTS_PATH>`
目标 skill：`<TARGET_SKILL_PATH>`
仓库根目录：`<REPO_ROOT>`
<PROFILE_CONTEXT_BLOCK><TASK_TEMPLATE_BLOCK>

把每个痛点分类为以下之一：

- `skill-gap`
- `public-surface-gap`
- `code-gap`
- `task-ambiguity`

要求：

1. 优先建议稳定、显式导出的辅助工件，而不是让盲测执行者直接读取仓库普通文件，或继续往 skill 里塞大量源码知识。
2. 每个痛点都要给出分类、原因和最小修复。
3. 明确判断本轮研究假设是被支持、被削弱，还是证据不足。
4. 如果属于 `skill-gap`，明确写出 Agent 1 需要补上的最小改动。
5. 如果这轮存在多个 blind-runner，先区分“多数实例的共性问题”和“单个实例的偶发问题”，不要把单实例噪声直接升级成全局结论。

把结论写入：`<ROOT_CAUSE_PATH>`。
把本轮决策写入：`<DECISION_PATH>`。
