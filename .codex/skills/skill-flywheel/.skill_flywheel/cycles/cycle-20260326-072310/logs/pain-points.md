# 痛点记录

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

## 结果

按 `weak-blind` 边界仅读取目标 skill、`context/` 和导出的 `public/` 后，可以直接找到前台/后台启动 5 轮外层迭代的命令，但无法仅凭当前公开工件判断“外层 runner 是否真的推进了 5 轮”、“应如何观察 `runner_state.json` / `progress.txt`”以及“什么算允许的提前停止”。

## 假设观察

部分支持。shell 启动入口已经存在，但对当前任务而言，公开工件还不足以支撑一次自包含的外层迭代观察闭环，因此还不能只靠导出工件验证假设。

## 痛点

1. 步骤：
   读取 `public/autonomous-self-improve-command.txt`。
   观察到的阻塞：
   只能看到启动命令，看不到与当前任务配套的检查顺序、状态文件观察点和 stop condition 解释。
   缺少的工件或说明：
   `autonomous-self-improve` 专用的公开 checklist，至少应覆盖 `runner_state.json`、`progress.txt`、`runner_logs/` 和“第 5 轮前不要提前收敛”的判定口径。
   影响：
   盲测执行者无法在不越界读源码的前提下，判断外层 shell loop 是否真的形成 fresh-process 闭环。

2. 步骤：
   对照 `context/profile.md` 与 `context/task.md`。
   观察到的阻塞：
   profile 仍写着“优先验证能否启动并检查一个最小 cycle，不是追求多轮深度迭代”，和当前任务的 5 轮外层迭代目标冲突。
   缺少的工件或说明：
   与当前任务一致的局部 profile，明确本轮优先验证 shell runner / background runner / 磁盘状态推进，而不是继续停留在最小单轮闭环。
   影响：
   即使启动命令存在，盲测执行者也会拿到互相冲突的任务优先级，容易过早收敛成“只验证一轮是否能跑”。
