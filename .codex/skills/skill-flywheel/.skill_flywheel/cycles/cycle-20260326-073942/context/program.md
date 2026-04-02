# 本轮研究程序

## 研究问题

`skill-flywheel` 是否已经具备类似 Ralph 的“外壳驱动 + 磁盘状态 + fresh-process”闭环，能够在无人值守时稳定推进自己的下一轮最小改动，而不是停留在单次会话里的人工编排。

## 当前假设

如果把 flywheel 的持久状态、迭代日志、停止条件和后台启动入口放到 shell + runner 层，而不是继续堆在聊天 prompt 里，那么 `skill-flywheel` 就能像 Ralph 一样稳定推进多轮自迭代。

## 对照基线

- baseline skill / baseline 工件：只有单轮 `init_public_surface.py` 和会话内编排，没有真正的外层 runner
- 本轮期待看到的差异：存在一个外壳层循环，能够基于磁盘状态持续拉起新的 Codex 进程，并在达到停止条件后自行收敛

## 固定边界

- 每一轮真正的研究判断仍然要落回 cycle 工件，而不是只写 runner 日志
- shell 只负责 fresh-process、超时、日志和 stop condition，不负责替代 flywheel 的研究判断
- 本轮允许 `weak-blind`，但不允许把它误写成 `clean-room`

## 并行设置

- 并行实例数：1
- 每轮共享的固定输入：目标 skill、自身 `.skill_flywheel/` 配置、runner state、progress log
- 允许变化的因素：每轮收敛出的下一个最小问题
- 禁止共享的内容：仅存在于会话上下文、未落盘的临时推理

## 随机性控制

- 本轮接受哪些随机性来源：Codex 单轮输出波动
- 是否允许不同模型 / 提示顺序 / 上下文长度：否
- 本轮是否要求多实例结论基本一致：不适用

## 任务选择

- 本轮使用的真实任务：让 `skill-flywheel` 学会像 Ralph 一样由外壳驱动自己迭代，并连续跑满 5 轮外层迭代，除非出现硬阻塞
- 为什么这个任务能验证当前假设：它直接测试 runner、state、progress、后台启动和真实 stop condition 是否闭环

## 成功信号

- 存在可直接运行的 shell 启动脚本，而不是只剩 Python 内循环
- 每轮都能在磁盘上看到新的状态推进、进度日志或 cycle 结论
- 外层 shell 可以在 5 轮内持续拉起 fresh Codex 进程，不依赖当前会话挂着

## 失败信号

- shell 仍然只是薄包装，真正循环仍然依赖人工会话
- 状态推进只体现在聊天输出，没有可靠磁盘状态
- 5 轮外层迭代无法启动，或启动后只空转模板

## 决策规则

- 如果属于 `skill-gap`：补 `SKILL.md` / 参考文档中的 runner 协议
- 如果属于 `public-surface-gap`：补可直接运行的公开命令与后台启动说明
- 如果属于 `code-gap`：补 runner、shell 外壳、状态文件或 stop condition
- 如果属于 `task-ambiguity`：收窄“今晚自迭代”任务，不把多目标混在一轮

## 冲突证据处理

- 如果 runner 日志与 cycle 结论冲突：以 cycle `decision` 为语义真相，以 runner log 为调度真相，分别修
- 如果 5 轮内没有新证据：停止并把空转原因记录到 `runner_state.json` 与 `progress.txt`

## 停止条件

- 跑满 5 轮外层迭代，或出现明确硬阻塞
- 得到明确结论：当前 runner 足够稳定，或下一层最小缺口已经定位

## 预算

- 最大轮数：5
- 最大并行实例数：1
- 连续多少轮没有新证据就停止：2
