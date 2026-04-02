你正在执行一轮研究盲测。

研究程序：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\context\program.md`
运行实例：`run-01`
实例输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\runs\run-01.md`
实例输出 JSON：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\runs\run-01.json`
使用真实目标 skill：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel` 来完成这个真实任务：

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


如果这轮存在多个 blind-runner，请只记录你这个实例的观察，不要提前对齐其他实例的结论。

你只允许读取：

- `E:\personal_project\rust_plc\.codex\skills\skill-flywheel`
- `E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\public`

不要读取目标 skill 之外的仓库文件，包括 README、docs、examples、src、crates 或其他受保护路径。只有显式导出到 `E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\public` 的辅助工件才可读取。

输出要求：

1. 给出你的任务结果。
2. 记录每个阻塞点或低效点。
3. 写清你希望得到的精确缺失项：工件、命令、示例或说明。
4. 明确指出你的观察是支持、削弱，还是无法判断当前研究假设。

把实例观察保存到：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\runs\run-01.md`。
如果需要机器可读版本，同时写入：`E:\personal_project\rust_plc\.codex\skills\skill-flywheel\.skill_flywheel\cycles\cycle-20260326-075638\logs\runs\run-01.json`。
