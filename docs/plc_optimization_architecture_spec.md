# PLC Optimization Architecture Spec

## 1. 文档定位

这份文档讨论的不是某一个站的技巧，而是 RustPLC 面向各类 PLC 程序时，应该如何建模“最优分析”。

它回答的核心问题不是：

- AI 能不能写出一段能跑的 PLC

而是：

- 对同一个工业目标，系统如何表达多个合法实现
- 系统如何定义 CT
- 系统如何在功能安全前提下最小化 CT
- 系统如何证明“这个方案更优”不是拍脑袋

这份文档是架构文档，不冻结最终 DSL 语法。

---

## 2. 第一性原理

对 PLC 项目来说，`最优` 不是一句描述，而是一个完整问题：

1. 目标意图是什么
2. 哪些实现与该意图语义等价
3. 哪些实现满足功能安全与资源约束
4. 每个实现的时间语义是什么
5. CT 到底按哪种定义计算
6. 在合法实现空间里，哪个实现使 CT 最小
7. 这个“最小”是全局最优、有界最优，还是启发式改进

如果这 7 件事不明确，那么“最优 PLC 程序”在工程上没有可审查含义。

---

## 3. 顶层抽象

RustPLC 的最优分析应建立在下面 5 个抽象层之上。

### 3.1 Intent

`Intent` 表达系统想完成什么，不表达具体怎么做。

例如：

- 把工件送到观测位
- 完成检测
- 把 NG 工件送到异常出口
- 把多个工件组装成一个新工件

Intent 必须尽量避免提前固化某种具体执行方式，否则会在语义层提前消灭优化空间。

### 3.2 Feasible Realization Set

`Feasible Realization Set` 表达：

- 针对同一个 Intent，哪些实现方式在语义上等价
- 其中哪些实现同时满足功能安全、资源、时序和设备能力约束

例如同一个“到达目标位”意图，可能存在：

- 串行实现
- 并行实现
- 带同步点的实现
- 带预定位的实现

最优分析不是直接改 PLC 文本，而是在这个合法实现集合里做选择。

### 3.3 Time Semantics

`Time Semantics` 表达：

- 一个实现的完成时间怎样计算
- 串行、并行、同步、资源冲突、等待、循环、批处理、pipeline 的时间如何合成

没有 Time Semantics，就没有可审查的 CT。

### 3.4 CT Functional

`CT Functional` 表达：

- 本次优化到底最小化哪一种 CT

CT 不是唯一的，至少要区分：

- 单件完成时间
- 工位循环时间
- 稳态节拍
- 最坏情况 CT
- 正常路径 CT
- 含异常恢复的 CT

### 3.5 Optimality Evidence

`Optimality Evidence` 表达：

- 候选方案相对什么基线更优
- 更优多少
- 满足哪些约束
- 最优性属于哪一类

没有 Evidence，就只能叫“候选方案”，不能叫“最优方案”。

---

## 4. 三类能力必须分层

RustPLC 中必须严格区分下面三类能力。

### 4.1 控制语义层

它回答：

- 设备能做什么
- 工件会发生什么
- 资源约束是什么
- safety / liveness / timing / causality 边界是什么

这是 DSL / IR / verification 的主路径。

### 4.2 最优分析层

它回答：

- 在所有合法 realization 中，哪个 realization 更优
- 更优是按照哪个 CT functional 定义

它不应伪装成普通 task/step 语义。

### 4.3 执行层

它回答：

- 当前 PLC 周期该执行哪个已选 realization 的下一步
- 偏差发生后是继续执行、切换计划、重规划还是降级运行

执行层不负责临场发明目标函数或重新定义最优性。

---

## 5. 为什么不能直接优化 PLC 文本

如果把“最优”理解成“直接改写 PLC 代码文本”，通常会出现下面 5 个问题：

1. Intent 和 realization 混在一起
2. verification 无法区分语义错误和优化选择
3. runtime 被迫承担规划器职责
4. 时间代价只能靠直觉比较
5. AI 容易输出看似合理、但无证据支持的“优化版本”

更合理的方向是：

- DSL / IR 先定义 Intent 与合法动作空间
- Optimization Layer 在合法 realization 集合中选方案
- Execution Layer 执行被选中的显式方案

---

## 6. 语义等价与实现等价

优化分析中最重要的一条原则是：

- 可以替换 realization
- 不可以改变 intent

这意味着优化层只能在“语义等价”的实现之间做选择。

一个 realization 若要被视为同一 intent 的合法替代，至少必须满足：

1. 工件状态转移等价
2. 入口与出口契约等价
3. 终态分类等价
4. 资源占用语义可接受
5. 故障边界不被悄悄改变
6. safety / liveness / timing 基本边界不被破坏

换句话说：

- 优化层可以改变“怎么实现”
- 不能改变“完成后系统意味着什么”

---

## 7. 合法 realization 的判定

RustPLC 不应把 realization 是否可选交给经验判断，而应显式检查下面几类约束。

### 7.1 Functional Safety

必须满足：

- 安全互锁不被破坏
- 危险区不被非法进入
- 联合动作不引入新碰撞路径
- 故障路由仍然闭合

功能安全是最优分析的前提，不是目标函数中的一个“软惩罚项”。

### 7.2 Resource Legality

必须满足：

- 独占资源不被重复占用
- 共享资源的竞争关系明确
- 设备能力边界未超出

### 7.3 Motion Coordination Legality

若 realization 涉及并行或联合运动，必须满足：

- 运动维度可独立控制
- 联合运动在该设备/该工艺下是允许的
- 完成条件可明确定义
- 任一子动作失败时的 fault 行为可闭合

### 7.4 Semantic Equivalence

必须证明：

- 完成后的工件位置、属性、终态、出口语义与原 intent 一致

---

## 8. Time Semantics 必须成为一等公民

RustPLC 若要认真讨论 CT 最小化，必须显式建模时间语义。

### 8.1 基本对象

时间分析至少需要下面这些对象：

- action duration
- wait duration
- timeout upper bound
- synchronization barrier
- resource occupancy window
- pending action lifecycle
- loop / batch / pipeline structure

### 8.2 基本时间合成规则

首版至少要支持下面几类时间组合。

串行：

- `T(serial(a, b)) = T(a) + T(b)`

并行：

- `T(parallel(a, b)) = max(T(a), T(b))`

同步后继续：

- `T(join(a, b, c)) = max(T(a), T(b)) + T(c)`

资源冲突串行化：

- 若 `a` 和 `b` 竞争同一独占资源，则不得按并行合成，必须由调度规则串行化

等待与超时：

- 正常路径时间与最坏情况时间必须分开计算

### 8.3 循环与稳态

循环系统不能只看单次路径长度，还要区分：

- 单次迭代时间
- 首件完成时间
- 稳态吞吐节拍
- 清空系统时间

否则“CT 最小化”会把非稳态与稳态概念混为一谈。

---

## 9. CT Functional 的统一框架

RustPLC 不应把 `CT` 当成一个含糊词，而应要求每次优化问题显式选定 CT functional。

### 9.1 Completion Time

定义：

- 某个工件从进入流程到完成离开的总时间

适用：

- 单件搬运
- 单件加工
- 单件异常恢复

### 9.2 Cycle Time

定义：

- 相邻两次产品启动或完成之间的周期时间

适用：

- 节拍式单站
- 重复循环工位

### 9.3 Steady-State CT

定义：

- 系统进入稳态后，单位产出的平均间隔

适用：

- pipeline
- 多缓冲区串联系统
- 多 task 并发执行系统

### 9.4 Worst-Case CT

定义：

- 在保守 timing bound 下的最大完成时间

适用：

- 高可靠场景
- 需要保证上界的系统

### 9.5 Fault-Inclusive CT

定义：

- 把异常检测、恢复、回收、重试纳入后的 CT

适用：

- 不能只按 happy path 评价的工艺

每次优化问题必须明确说明：

1. 优化的是哪一种 CT
2. 该 CT 针对哪类对象
3. 使用 nominal 还是 worst-case 时间口径
4. 是否包含异常恢复路径

---

## 10. CT 计算与 CT 最小化必须分开

这是架构上的硬分层。

### 10.1 CT Evaluation

`CT Evaluation` 是分析问题。

输入：

- 一个固定 realization
- 时间语义模型

输出：

- 该 realization 的 CT 值
- 关键路径
- 资源瓶颈
- 同步瓶颈

### 10.2 CT Minimization

`CT Minimization` 是优化问题。

输入：

- 一个 Intent
- 一个 Feasible Realization Set
- 一个 CT functional

输出：

- 使 CT 最小的 realization
- 或当前已知更优 realization

如果不先把 CT Evaluation 和 CT Minimization 分开，系统很容易把“我觉得更快”误当成“已经被分析为更优”。

---

## 11. Optimization Problem 的标准结构

RustPLC 应要求每个优化问题都显式给出下面这些字段。

### 11.1 Objective

例如：

- minimize completion_time
- minimize cycle_time
- minimize steady_state_ct
- minimize energy_subject_to_ct

### 11.2 Constraints

例如：

- safety constraints
- resource constraints
- reachability constraints
- process order constraints
- maximum wait constraints

### 11.3 Decision Variables

例如：

- realization choice
- ordering choice
- batching choice
- allocation choice
- parameter-set choice

### 11.4 Solve Time

例如：

- compile time
- pre-run planning time
- runtime replanning time

### 11.5 Optimality Class

例如：

- global optimal
- bounded optimal
- heuristic improvement
- baseline improvement

---

## 12. 通用优化问题类型

这些类型不是按行业分，而是按抽象结构分。

### 12.1 Realization Selection

在多个语义等价 realization 中选一个更优 realization。

例如：

- 串行 vs 并行
- 单步完成 vs 预定位后完成
- 一次处理 vs 分阶段处理

### 12.2 Ordering Optimization

在多个可行顺序中选择 CT 更小的顺序。

### 12.3 Allocation Optimization

在多个资源分配方案中选择 CT 更小的方案。

### 12.4 Batching Optimization

在多种合批与拆批方式中选择更优节拍方案。

### 12.5 Parameter Optimization

在多个参数集之间选择更优的时间质量折中方案。

### 12.6 Recovery Optimization

在多个异常恢复方案中选择损失更小的方案。

---

## 13. Plan Artifact 应表达什么

优化输出不应只是“一个新程序”，而应是一个可检查的计划对象。

这个计划对象至少应包含：

1. 目标 intent 标识
2. 被选中的 realization 标识
3. 关键决策变量取值
4. 目标函数定义
5. 计算得到的 CT
6. legality check 结果
7. optimality class
8. baseline 对比信息

也就是说：

- Plan Artifact 不仅决定“做什么顺序”
- 还应决定“同一意图用哪种 realization 执行”

---

## 14. 最优性证据

RustPLC 不应接受“更优”这种裸结论，而应要求最小证据集。

最小证据集至少包括：

1. baseline
2. candidate
3. objective definition
4. CT evaluation result
5. legality check result
6. optimality class

若声称是全局最优，还应补：

- solver 结论
- 最优性证明或可接受的证明替代

若只是启发式改进，应明确写：

- 使用了什么启发式
- 相对什么基线有改进
- 哪些场景下不保证最优

---

## 15. 与 verification 的关系

verification 和 optimization 是两条不同主线。

verification 回答：

- 能不能这样做

optimization 回答：

- 在能这样做的前提下，哪个 realization 更好

二者关系固定为：

1. semantic / verification 先定义合法边界
2. optimization 只能在合法边界内选 realization
3. 选出的 plan artifact 仍应再次经过 legality check

optimization 不能替代 verification。

---

## 16. AI 在体系中的职责

AI 的合理职责不是直接声称“这就是最优 PLC 代码”，而是：

1. 帮用户把工业需求翻译成 Intent
2. 帮用户枚举 candidate realizations
3. 帮用户补齐 objective / constraints / decision variables
4. 帮用户解释 CT functional 与证据
5. 帮用户生成 plan artifact 或 optimization problem 规格

AI 在这个体系里更接近：

- 问题建模器
- 候选方案生成器
- 解释器

而不是：

- 无证据的最优性宣称者

---

## 17. 对项目落地的建议

如果 RustPLC 未来要正式支持最优分析，建议按下面顺序推进。

### Phase 1

- 先把工件、资源、位置、任务、约束语义做扎实
- 先确保 safety / liveness / timing / causality 的合法边界稳定

### Phase 2

- 显式引入 Intent 与 Realization 的区分
- 冻结 realization legality 的判定规则

### Phase 3

- 建立基础 Time Semantics
- 明确 serial / parallel / join / resource conflict / loop 的时间合成规则

### Phase 4

- 冻结 CT functional 的分类与口径
- 支持 CT evaluation

### Phase 5

- 定义 Optimization Problem 与 Plan Artifact 的稳定结构

### Phase 6

- 再接入具体求解器
- 再让 AI 帮助用户构造优化问题与解释结果

---

## 18. 结论

RustPLC 面向各类 PLC 程序的最优分析，正确方向不是：

- 让 AI 直接写一段自称最优的 PLC 流程

而是：

- 先用 DSL / IR 定义 Intent 与合法动作空间
- 再枚举或构造语义等价的 Feasible Realization Set
- 再为 realization 建立显式 Time Semantics
- 再选择明确的 CT functional
- 最后在功能安全前提下最小化该 CT functional，并输出带证据的 plan artifact

只有这样，`最优` 才不是一句模糊口号，而是一个：

- 可建模
- 可计算
- 可最小化
- 可验证
- 可解释
- 可落地

的工程能力。
