# 并行实验

## 目的

多 agent / 多实例并行的价值，不是“更热闹”，而是更快区分：

- 稳定缺口
- 随机噪声
- 任务路径分叉
- 单次上下文污染

如果一个结论只在单个 blind-runner 身上出现，而在其余实例上无法复现，它更可能是噪声，而不是应当立刻写进 skill 的事实。

但要注意：多个 `weak-blind` 实例并不自动等于 `clean-room`。它们仍可能共享更大的对话环境，只是彼此写集分离。

## 最小并行结构

一轮并行实验至少可以包含：

- 1 个 `experiment-designer`
- N 个 `blind-runner`
- 1 个 `root-cause-analyst`
- 可选 1 个 `synthesizer`

其中：

- `experiment-designer` 负责编写或收窄 `program.md`
- 每个 `blind-runner` 独立执行同一研究程序
- `root-cause-analyst` 负责分析单轮或单实例证据
- `synthesizer` 负责跨实例聚合共性、分歧和停止建议

## 目录建议

如果一轮中存在多个 blind-runner，建议把实例级产物拆开保存，例如：

```text
cycle-20260325-xxxxxx/
├── context/
├── logs/
│   ├── runs/
│   │   ├── run-01.md
│   │   ├── run-02.md
│   │   └── run-03.md
│   ├── run-index.json
│   ├── synthesis.md
│   ├── synthesis.json
│   ├── root-cause.md
│   └── decision.md
```

如果当前脚本还没自动生成这些文件，至少要手工遵守“每个实例独立记录，再聚合”的原则。

当前推荐的脚手架是：

- `prompts/runs/run-xx-agent2.md`
- `logs/runs/run-xx.md`
- `logs/runs/run-xx.json`
- `logs/run-index.json`

## 运行规则

1. 所有 blind-runner 必须共享同一份 `program.md`。
2. 所有 blind-runner 必须使用同一真实任务，除非研究程序明确允许任务扰动。
3. 每个 blind-runner 只写自己的观察，不提前读取其他实例的结论。
4. 聚合发生在盲测结束之后，而不是盲测过程中。
5. 推荐让脚本先生成 run 级 prompt 与日志骨架，再分发给各实例。

## 聚合时看什么

聚合时至少要回答：

- 哪些痛点在多数实例中重复出现
- 哪些痛点只出现于单个实例
- 多实例是否对当前假设给出一致信号
- 如果不一致，冲突出在任务、工件、skill 还是纯噪声

## 什么时候值得并行

优先并行的场景：

- 任务路径很多
- 代理行为随机性高
- 你怀疑单轮观察不稳定
- 这轮的结论会决定是否修改核心 skill

不必并行的场景：

- 只是检查脚本能否初始化
- 当前研究问题非常窄且几乎没有分叉
- 你还没有定义好 baseline 和停止条件

## 证据强度

并行与 clean-room 是两条独立维度：

- `weak-blind + 单实例`
- `weak-blind + 多实例`
- `clean-room + 单实例`
- `clean-room + 多实例`

其中证据强度最高的是 `clean-room + 多实例`。
如果只是 `weak-blind + 多实例`，它更适合发现模式和收窄假设，而不适合直接宣布“skill 已通过盲测”。
