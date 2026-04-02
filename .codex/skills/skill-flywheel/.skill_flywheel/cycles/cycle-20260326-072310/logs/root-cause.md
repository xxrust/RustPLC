# 根因分析

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


## 假设判断

部分支持。

## 结论

1. 痛点：
   盲测执行者无法只靠当前任务导出的公开工件判断 shell runner 是否真的推进了 5 轮。
   分类：
   `public-surface-gap`
   原因：
   当前只导出了 `autonomous-self-improve-command.txt`，没有与该任务配套的检查清单；同时 `.skill_flywheel/profile.md` 仍强调“最小 cycle”，和今晚的 5 轮外层迭代目标冲突。
   最小修复：
   不在本轮同时改文档和代码。先把外层轮次在磁盘状态上变成可判读，再于下一轮补 task-specific checklist 与 profile 对齐。

2. 痛点：
   `progress.txt` 无法反映 fresh-process 外层迭代次数。
   分类：
   `code-gap`
   原因：
   `flywheel.ps1` 每次只调用一次 `flywheel_runner.py --max-iterations 1`，而 runner 之前把 `iteration_count` 直接覆写成本次局部循环编号，导致多次 fresh-process 调用都会把状态、prompt 副本和 progress 记录成 `Iteration 1`。
   最小修复：
   让 `flywheel_runner.py` 从已有 `runner_state.json` 读取 session 级 `iteration_count`，跨 fresh-process 累积编号，并把该编号用于 prompt/log/progress；新增 dry-run 回归测试，验证连续两次调用会生成 `iter_001_prompt.md` 和 `iter_002_prompt.md`。
