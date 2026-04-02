请改进项目 `<PROJECT_NAME>` 的目标 skill：`<TARGET_SKILL_PATH>`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `<REPO_ROOT>` 和目标 skill。

研究程序：`<PROGRAM_PATH>`
真实任务：
<TASK>

禁止读源码执行者使用的显式辅助工件包：`<PUBLIC_DIR>`
痛点记录路径：`<PAIN_POINTS_PATH>`
根因分析路径：`<ROOT_CAUSE_PATH>`
<PROFILE_CONTEXT_BLOCK><TASK_TEMPLATE_BLOCK>

要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`<AGENT1_FEEDBACK_PATH>`。
