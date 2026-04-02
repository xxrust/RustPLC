# 本轮研究程序

## 研究问题

`skill-flywheel` 是否已经具备一轮最小自测所需的局部配置和输出结构，能让盲测执行者在不读源码的前提下初始化并检查一个研究回合。

## 当前假设

如果为 `skill-flywheel` 本身补齐局部 `program.md`、任务模板和一份显式导出的运行命令工件，那么一名只读目标 skill 与 `public/` 的执行者就能完成最小 smoke test。

## 对照基线

- baseline skill / baseline 工件：没有 `.skill_flywheel/` 自测配置的 `skill-flywheel`
- 本轮期待看到的差异：无需读取仓库普通文件，也能启动并检查一轮最小 cycle

## 成功信号

- 盲测执行者可以仅依赖目标 skill、任务说明和导出工件运行 `init_public_surface.py`
- 生成出的 cycle 目录包含 `context/program.md`、`logs/decision.md` 和三份 agent prompt
- 盲测执行者能指出是否还缺关键输入，而不是被流程本身卡住

## 失败信号

- 盲测执行者仍然需要读取仓库普通文件才能知道怎么启动这轮测试
- 生成出的 cycle 缺少研究协议要求的核心工件
- 任务模板或辅助工件仍然不足以支撑一次最小自测

## 并行设置

- 并行实例数：1
- 每个实例共享的固定输入：目标 skill、`self-smoke.md`、导出的 smoke 工件
- 每个实例允许变化的因素：无
- 实例之间禁止共享的内容：不适用

## 随机性控制

- 本轮接受哪些随机性来源：无
- 是否允许不同模型 / 提示顺序 / 上下文长度：否
- 本轮是否要求多实例结论基本一致：是

## 决策规则

- 如果属于 `skill-gap`：修改 `skill-flywheel` 文案或模板
- 如果属于 `public-surface-gap`：补充 `.skill_flywheel/public/` 下的辅助工件
- 如果属于 `code-gap`：修改初始化脚本或其输出
- 如果属于 `task-ambiguity`：收窄自测任务模板

## 停止条件

- 完成一轮最小自测并留下 cycle 产物
- 得到明确结论：继续下一轮，或当前已足够作为基础 smoke test

## 预算

- 最大轮数：2
- 最大并行实例数：1
- 连续多少轮没有新证据就停止：1
