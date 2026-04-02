请改进项目 `rust_plc` 的目标 skill：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel`。

如果需要借助 `$skill-creator` 来重构或补全文案，可以使用；这不是必须的。

你可以读取仓库源码 `E:\personal_project\rust_plc` 和目标 skill。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\context\program.md`
真实任务：
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


禁止读源码执行者使用的显式辅助工件包：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\public`
痛点记录路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\pain-points.md`
根因分析路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\root-cause.md`

局部配置路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\context\profile.md`

任务模板路径：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\context\task.md`


要求：

1. 保持目标 skill 精简，不要把能稳定导出的事实都塞进 skill。
2. 如果某个阻塞更适合通过显式辅助工件、诊断、命令或代码修改解决，要明确指出。
3. 只在根因明确属于 `skill-gap` 时修改目标 skill，不要把研究程序里应承担的内容误塞进 skill。
4. 如果根因属于 `skill-gap`，给出最小 skill 改动方案。
5. 把需要交回的最小改动写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\agent1-feedback.md`。
