请基于多个 blind-runner 实例的输出做跨实例聚合。

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

实例索引：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\run-index.json`
聚合输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\synthesis.md`
聚合输出 JSON：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\synthesis.json`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\context\task.md`


要求：

1. 先区分多数实例重复出现的共性问题，与只出现在个别实例中的偶发问题。
2. 明确判断多实例对当前研究假设给出的总体信号：支持、削弱或证据不足。
3. 如果实例间结论冲突，写清冲突来自任务分叉、工件缺口、skill 缺口还是纯噪声。
4. 不要直接修改 root-cause 或 decision；你的职责是先给 analyst 提供跨实例证据。

把聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\synthesis.md`。
把机器可读聚合结果写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-072310\logs\synthesis.json`。
