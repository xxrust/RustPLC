# Benchmark 治理

## 目标

当 `skill-flywheel` 不再只验证“流程是否闭环”，而要验证“目标 skill 是否真的变强”时，必须把 benchmark 治理与当前优化回合拆开。

这份文档只定义长期稳定的 benchmark 角色边界、目录契约与冻结规则，不绑定任何具体项目或具体 case。

## 角色分离

至少区分这四个角色：

1. `benchmark-proposer`
   负责从真实任务、仓库样本或历史失败中整理候选 case。
2. `benchmark-curator`
   负责审核、冻结、分层与退役 case。
3. `runner-judge`
   负责执行被测 skill、读取 hidden rubric / oracle，并写入评测结果。
4. `flywheel-optimizer`
   负责根据评测输出收窄假设、补 skill / public surface / code。

长期规则：

- `benchmark-proposer` 可以是 `skill-flywheel` 的子 agent。
- `benchmark-curator` 不应只是当前优化回合里的同一执行者；至少要与本轮 active optimizer 解耦。
- `runner-judge` 可以自动化，但它读取 hidden rubric / oracle 的路径必须与 flywheel 的可见输入隔离。
- `flywheel-optimizer` 不得在同一轮里一边改 skill，一边改 frozen benchmark case。

## 为什么不能让 flywheel 兼任 curator

如果当前优化回合既发现失败，又能直接冻结/改写 benchmark，就会出现：

- 为了适配当前 skill 表现而修改题目
- 把失败样本“清洗”成更容易通过的样本
- 在没有外部约束的情况下，把 dev case 当作 holdout 结果宣传

这会让 benchmark 失去“外部证据”的意义。

## Split 约定

默认只用三层 split：

1. `dev`
   允许日常迭代反复跑；结果可回流给 flywheel。
2. `holdout`
   只在阶段验收时运行；不应在每轮优化后立即泄露完整 hidden 细节。
3. `canary`
   用于后续新增的真实新样本；优先反映新问题，而不是稳定回归。

推荐规则：

- `dev` 用于发现模式、收敛假设、验证最小修复。
- `holdout` 用于阶段门禁，防止 flywheel 只会过自己看过的题。
- `canary` 用于持续吸收新鲜失败样本，但要和历史稳定集区分开。

## Case 状态

每个 case 至少有一个状态：

- `draft`
  候选样本，尚未进入冻结 benchmark。
- `frozen`
  已冻结；当前优化回合不得修改内容。
- `retired`
  因过时、重复或契约漂移而退出主 benchmark，但应保留历史记录。

长期规则：

- flywheel 在读到 `frozen` case 的失败结果后，只能改 skill / public surface / code，不能回头改题。
- 如果确实需要修改 frozen case，应由 curator 明确退役旧 case，再创建新 case，不要静默覆写。

## 可见性边界

对被测 skill 或 blind runner，默认只公开：

- case 的 `public/` 输入
- benchmark root 下允许公开的说明

默认 hidden：

- `rubric`
- `oracle`
- 参考答案式说明
- 仅供 judge 使用的执行脚本

对 flywheel，默认只公开：

- case id
- split
- 执行结果摘要
- 失败分类
- 聚合统计

不要把完整 hidden oracle 直接交给当前优化回合。

## Oracle

`oracle` 指外部判定依据。

常见类型：

1. `executable`
   通过命令、脚本、工具链运行结果直接判定。
2. `structured`
   通过固定 JSON 字段、结构约束或机器可读信号判定。
3. `textual`
   通过 rubric 检查文本行为；仅在缺少更硬判据时使用。

长期偏好：

- 能用 `executable`，就不要只用 `textual`
- 能用 `structured`，就不要只靠模糊主观评价

## Blocker

`blocker` 指当前任务无法继续完成的真实阻塞，而不是普通低效点。

例如：

- 缺少会改变结构的关键信息
- 对外契约、CLI、报告或诊断本身缺失
- 工具链存在已知限制
- 被测产品尚未承载所需语义

当 case 设计包含 blocker 路径时，rubric / oracle 应明确：

- 何时应诚实报 blocker
- 何时不应伪装成已完成
- 何时属于 skill 自身误判

## 最小目录契约

推荐使用：

```text
benchmark-root/
├── manifest.json
├── cases/
│   ├── dev/
│   │   └── case-001/
│   │       ├── case.json
│   │       ├── public/
│   │       ├── hidden/
│   │       └── evaluation/
│   ├── holdout/
│   └── canary/
```

其中：

- `public/`
  给被测 skill 的输入
- `hidden/`
  给 curator / judge 的 rubric、oracle、内部说明
- `evaluation/`
  只写执行结果，不写新的题面

## 单轮协议

当 flywheel 要基于 benchmark 做效果验证时，推荐顺序：

1. proposer 生成或整理候选 case。
2. curator 冻结一批 `dev` case。
3. runner-judge 运行被测 skill，并写 `evaluation/result.json`。
4. flywheel 只读取聚合结果与失败摘要，决定最小修复。
5. 修复后重跑 `dev`。
6. 只有到阶段验收时，才运行 `holdout`。

## 结果落盘

每个评测结果至少记录：

- case id
- split
- 被测 skill 版本或 revision
- run time
- pass / fail / blocked
- blocker 分类
- 关键证据路径
- 结构化 metrics

聚合结果至少记录：

- 总 case 数
- pass 数
- blocked 数
- 最常见失败分类
- 是否存在从 `dev` 反复出现的稳定痛点

## 什么时候值得引入 benchmark

优先在这些场景使用 benchmark：

- 你要证明某个 skill 的真实任务成功率提高了
- 你怀疑 flywheel 只是在优化文案，而没有改善交付
- 你需要把“研究结论”升级为“阶段验收结论”

如果只是脚本初始化、自举目录或验证 JSON/Markdown 是否同步，没必要引入完整 benchmark。
