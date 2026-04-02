# Confirmed System To PLC Lowering

当用户已经提供确认版 `.system.md` 时，`plc-gen` 不应再从零问需求，而应先把 system contract 压缩成一个 `.plc` lowering 摘要。

默认按这 6 个桶来收敛：

1. `task partition`
   - `.system.md` 里明确列出的并发 task，优先原样保留为 `[tasks]` 名称。
   - 不要把 `feed_prep`、`orient_stage`、`transfer_to_measure` 这类并发工位压平成单循环大 task。

2. `blocking / timeout`
   - 所有显式等待都落到 `wait` / `delay` / `axis.move_*`。
   - manual wait 才用 `allow_indefinite_wait: true`。
   - 非 manual wait 默认要有 `timeout -> goto real_task.step`。

3. `mode matrix`
   - 自动主流程和手动 / 单步 / 维护模式，优先分离成独立 task 或 supervisor + service 结构。
   - 不要把所有模式分支塞进同一个自动 task 的若干 `if`。

4. `warning vs fault`
   - “修复后操作员刷新并自动续跑”更像 warning task。
   - “达到阈值后停机告警”更像 fault task。
   - 二者不要混成同一个 `fault_handler`，除非 system contract 本身没有区分恢复语义。

5. `counters / retry / streak / rate`
   - 这类状态优先提升到 `[topology] variable`。
   - 在 step 里用 `compute` 更新，而不是依赖隐式副作用。

6. `resource / interlock`
   - 共享占用区优先用 `semantic_resource` + `claim`。
   - 真实状态依赖用 `requires`。
   - 真实互斥冲突用 `conflicts_with`。
   - 不要用 `conflicts_with` 表达纯顺序关系。

回答时建议先给 5 到 10 行 lowering 摘要，再进入 `.plc` 或 scaffold 交付。
