请基于多个 blind-runner 实例的输出做跨实例聚合。

研究程序：`<PROGRAM_PATH>`
任务：
<TASK>
实例索引：`<RUN_INDEX_PATH>`
聚合输出 Markdown：`<SYNTHESIS_PATH>`
聚合输出 JSON：`<SYNTHESIS_JSON_PATH>`
<PROFILE_CONTEXT_BLOCK><TASK_TEMPLATE_BLOCK>

要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`<SYNTHESIS_PATH>`。
把机器可读聚合结果写入：`<SYNTHESIS_JSON_PATH>`。
