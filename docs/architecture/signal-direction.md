# 并发 Task 与阻塞 Step 语义契约（Gate-A）

## 1. 文档角色

本文件是并发执行模型的术语冻结文档，是 US-001（Gate-A）的唯一架构语义来源。

后续所有实现故事（IR、runtime、verification、bridge、docs）都必须复用本文件术语，不允许在局部模块发明冲突定义。

## 2. 冻结术语

1. **active task**
   进入调度集合、在当前运行周期中可被遍历推进的 task。
2. **task context**
   某个 active task 的独立执行上下文，至少包含：当前 step、step_entered_at、等待状态、超时状态、挂起动作状态。
3. **blocking step**
   当前 tick 内不能完成离开、必须等待后续 tick 条件满足的 step。
4. **non-blocking step**
   当前 tick 内可完成并离开的 step（例如仅含即时动作且无等待条件）。
5. **pending action**
   已发起但尚未完成的长时动作实例；生命周期至少覆盖 `Pending -> Done/Fault/Timeout`。
6. **completion condition**
   step 离开判定条件。仅当 completion condition 满足时，task 才能离开当前 step。

## 3. 调度与推进规则

1. 每个 runtime tick 统一调度 active task 集合，并按 task 声明顺序（`task[0] -> task[1] -> ...`，即索引升序）遍历。
2. 单个 task 在同一 tick 内允许串联推进多个 non-blocking step。
3. 单个 task 一旦遇到 blocking step，必须在该 tick 停止推进。
4. 某个 task 被 blocking step 阻塞，不得阻塞其他 task 在同一 tick 或后续 tick 的推进。

## 4. 并发语义边界

task 并发的定义固定为：

- 多个 task 同时持有独立 task context，并被统一调度器按固定顺序遍历。
- 并发不是“单执行点在 task.step 之间来回跳转”的等价表述。

实现层（IR/runtime/verification）必须显式建模 task context，禁止回退到全局单 Location 的隐式并发。

## 5. 首版自动阻塞范围

首版默认自动阻塞（blocking step）范围固定为：

1. `axis.move_relative` 与 `axis.move_absolute`
2. `delay`
3. `wait`
4. `timeout` 驱动的等待阶段
5. 依赖外部反馈完成的动作（例如需要设备反馈/外部函数回执才能判定完成）

不在本名单内的动作默认按 non-blocking 处理，除非后续故事显式扩展并同步文档。

## 6. Runtime Budget 与调度公平性

1. runtime transition budget 的口径固定为 **per-task-per-tick**：
   - 单个 task 在同一 tick 内的 transition 链式推进上限是 `MAX_TRANSITIONS_PER_TASK_PER_TICK`。
   - 该上限用于防止同 tick 自循环或零时延链路导致的无限推进。
2. 全局每 tick 的最大 transition 上界按活跃 task 数量线性放大：
   - `max_transitions_all_tasks_per_tick_upper_bound = active_task_count * max_transitions_per_tick_cap`。
   - 该值用于 report/预算估算，不改变单个 task 的硬上限。
3. 调度公平性与 budget 的关系：
   - 调度器按固定顺序遍历所有 active task；
   - 每个 task 在自己的预算耗尽、命中 blocking step 或完成推进后让出执行；
   - 单个 task 的预算耗尽不会剥夺其他 task 在同 tick 的推进机会。
4. 运行时错误与告警必须带上下文（task 索引、尝试次数、per-task cap、active task 数），避免并发场景下的“超预算”不可解释。

## 7. 约束与变更规则

1. 若实现需要新增/收紧术语，必须先更新本文件，再更新代码与测试。
2. verification 的 safety/liveness/timing/causality 规则必须使用本文件同名术语解释。
3. runtime bridge 与 codegen 不得自行补语义，必须消费已经在本文件与 IR 中闭合的定义。

## 8. 最小验收映射（US-001）

- 本文件提供 `active task/task context/blocking step/non-blocking step/pending action/completion condition` 的明确定义。
- 本文件明确“同 tick 串联 non-blocking，遇 blocking 立即停”的推进规则。
- 本文件明确 task 并发是“多独立 context 被统一调度”，不是单执行点跳转。
- 本文件明确首版自动阻塞范围：`axis.move_*`、`delay`、`wait`、`timeout` 等等待、外部反馈动作。
