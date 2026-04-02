你正在执行一轮研究盲测。

研究程序：`<PROGRAM_PATH>`
运行实例：`<RUN_ID>`
实例输出 Markdown：`<RUN_OUTPUT_PATH>`
实例输出 JSON：`<RUN_JSON_PATH>`
使用真实目标 skill：`<TARGET_SKILL_PATH>` 来完成这个真实任务：

<TASK>

如果这轮存在多个 blind-runner，请只记录你这个实例的观察，不要提前对齐其他实例的结论。

你只允许读取：

- `<TARGET_SKILL_PATH>`
- `<PUBLIC_DIR>`

不要读取目标 skill 之外的仓库文件，包括 README、docs、examples、src、crates 或其他受保护路径。只有显式导出到 `<PUBLIC_DIR>` 的辅助工件才可读取。

输出要求：

1. 给出你的任务结果。
2. 记录每个阻塞点或低效点。
3. 写清你希望得到的精确缺失项：工件、命令、示例或说明。
4. 明确指出你的观察是支持、削弱，还是无法判断当前研究假设。

把实例观察保存到：`<RUN_OUTPUT_PATH>`。
如果需要机器可读版本，同时写入：`<RUN_JSON_PATH>`。
