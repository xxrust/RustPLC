请结合任务、盲测执行者的输出，以及必要的仓库源码进行分析。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\context\program.md`
任务：
让 `skill-flywheel` 像 Ralph 一样，通过外壳层驱动的 fresh-process 循环持续迭代自己。

今晚的目标只有两件事：

1. 学会像 Ralph 一样用外壳开启迭代，而不是只在单次会话里人工编排。
2. 让外层 runner 至少连续推进 5 轮外层迭代；除非出现硬阻塞，否则不要在第 5 轮之前提前收敛。

执行要求：

1. 优先修 shell runner、后台启动脚本、磁盘状态、进度日志和 stop condition。
2. 每轮都要把真正的研究判断落到 cycle 工件里，不要只写 runner 日志。
3. 每轮只做一个最小 next action，不要把整套系统重写成大工程。
4. 如果本轮只是 `weak-blind`，必须明确标记，不能伪装成 `clean-room`。
5. 如果连续两轮没有新证据，允许提前停止，但必须把原因写进 `runner_state.json`、`progress.txt` 和本轮 cycle 的 `decision`。

痛点日志：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\pain-points.md`
目标 skill：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel`
仓库根目录：`E:\personal_project\rust_plc`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\context\task.md`


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

把结论写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\root-cause.md`。
把本轮决策写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\decision.md`。
