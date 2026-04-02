# 本轮研究程序

## 研究问题

当当前会话无法或不应启动子 agent 时，`skill-flywheel` 是否仍然提供了足够明确的单代理 fallback，让研究者能在不伪装成 `clean-room` 的前提下完成一轮 `weak-blind` 闭环。

## 当前假设

如果把“单代理 / 手工角色执行”的约束、证据强度和落盘步骤显式写进 `skill-flywheel` 及其导出工件，那么研究者即使不能并行开 agent，也能稳定完成一轮最小 flywheel，而不会停在空模板或误报已完成高可信盲测。

## 对照基线

- baseline skill / baseline 工件：当前文案强调 agent 角色与并行 scaffold，但没有把单代理 fallback 写成稳定协议
- 本轮期待看到的差异：即使不启动子 agent，也能留下完整的 pain-points、root-cause、decision，并明确标记为 `weak-blind`

## 固定边界

- 盲测观察阶段只读取目标 skill、`context/` 与显式导出的 `public/`
- 不把当前会话里的仓库探索结果偷渡进盲测结论
- 本轮不得把 `weak-blind` 结果写成 `clean-room` 通过

## 并行设置

- 并行实例数：1
- 每个实例共享的固定输入：目标 skill、任务说明、导出的辅助工件
- 每个实例允许变化的因素：无
- 实例之间禁止共享的内容：不适用

## 随机性控制

- 本轮接受哪些随机性来源：无
- 是否允许不同模型 / 提示顺序 / 上下文长度：否
- 本轮是否要求多实例结论基本一致：不适用

## 任务选择

- 本轮使用的真实任务：初始化一轮新 cycle，并在不启动子 agent 的前提下手工完成 blind-runner 观察、根因分析和决策记录
- 为什么这个任务能验证当前假设：它直接检验 skill 是否给出了“无法开 agent 时怎么继续”的明确协议，而不是只会生成空骨架

## 成功信号

- 研究者仅凭目标 skill、cycle 上下文和导出工件，就能写出一份合格的 `logs/pain-points.md`
- `logs/root-cause.md` 与 `logs/decision.md` 能明确标记本轮证据属于 `weak-blind`
- 本轮不会因为缺少子 agent 而停在空模板，且不会把证据强度写高

## 失败信号

- 研究者无法从现有 skill / public 工件判断单代理时应如何扮演各角色
- 日志里无法明确表达 `weak-blind` 证据边界，或容易误写成 `clean-room`
- 为了闭环不得不依赖仓库普通文件或会话记忆补流程

## 决策规则

- 如果属于 `skill-gap`：修改 `skill-flywheel`，补清单代理 fallback 与证据标注规则
- 如果属于 `public-surface-gap`：补充 `.skill_flywheel/public/` 中的执行清单或落盘模板
- 如果属于 `code-gap`：修改初始化脚本，让 cycle 默认导出更直接的闭环辅助工件
- 如果属于 `task-ambiguity`：重写本轮任务模板，消除“初始化”和“闭环记录”之间的歧义

## 冲突证据处理

- 如果观察与既有 cycle 结论冲突：以当前实际产物和可执行流程为准，复核旧结论是否只验证了 scaffold 而非闭环
- 如果证据不足：不直接宣称 skill 通过，只记录下一轮需要补的最小工件或协议

## 停止条件

- 完成一轮单代理 `weak-blind` 闭环并留下完整 cycle 产物
- 得到明确结论：当前 skill 足以支持这种 fallback，或已经定位到最小修复

## 预算

- 最大轮数：1
- 最大并行实例数：1
- 连续多少轮没有新证据就停止：1
