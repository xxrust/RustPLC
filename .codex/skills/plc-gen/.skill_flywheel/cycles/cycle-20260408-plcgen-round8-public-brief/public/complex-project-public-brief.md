# Complex Project Public Brief Contract

当 `plc-gen` 面对复杂项目，而且调用者看不到仓库源码时，主 agent 必须先准备一份 `public brief`，再进入 one-shot 编排。

这不是可选文案，而是 architect / implementer / reviewer 的共同输入。

## brief 最低内容

至少写清：

1. 任务目标
   - 这轮要修什么、生成什么，还是交付什么
2. 当前 source shape
   - 单文件 `.plc`
   - 还是 `.bundle.toml` + fragments
   - 是否已存在 scaffold 项目
3. 已冻结的 system / lowering facts
   - task partition
   - blocking / timeout / wait / delay / axis.move_*
   - mode / supervisor / warning / fault
   - resource / interlock / counter / retry
4. 当前已有文件与期望写入物
   - 当前已有的关键文件
   - 这轮允许 skill 写入哪些文件
5. authored artifact 范围
   - 是否需要 scenario
   - 是否需要可选 `*.intent_alignment.contract.json`
6. 不可改变的边界
   - 不允许破坏的 source boundary
   - 不允许擅自补全的未冻结 contract
7. blocker / assumptions
8. 成功判据

## one-shot 交接规则

### architect 基于 brief 交付

- source shape 决策
- lowering 决策
- write scope 拆分
- proof map

### implementer 基于 brief 交付

- 修改后的文件
- scope closure statement
- residual risks

### reviewer 基于 brief 和实现结果交付

- findings
- verdict
- residual risks

## 不要做的事

- 不要把 brief 省略成“去看源码就知道”
- 不要把命令列表当作 brief 主体
- 不要让 implementer 自己重新猜 contract
- 不要让 reviewer 一边审一边继续发明需求

## brief 不足时怎么办

如果子 agent 发现 brief 不足：

- 记录缺口
- 退回主 agent 补 brief
- 不要越权读源码补洞
