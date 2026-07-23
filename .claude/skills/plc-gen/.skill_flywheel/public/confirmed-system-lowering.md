# Confirmed System To PLC Lowering

当用户已经提供确认版 `.system.md` 时，`plc-gen` 不应再从零问需求，而应先把 system contract 压缩成一个 project-lowering 摘要。

对 complex delivery，先把 lowering 映射到 5 类写入物：

1. delivery asset `docs/*.system.md`
2. delivery asset `docs/*.architecture.md` / `docs/*.verification.md`
3. delivery asset `plc/main.bundle.toml` 与 fragments
4. delivery asset `scenarios/nominal/normal.yaml`
5. `*.intent_alignment.contract.json` 或显式 blocker

如果输入本身是多工位整线描述，先冻结 delivery layer，再做 lowering；不要一边按 `station` 写，一边在 system 里保留整线语义。

默认按这 6 个桶来收敛：

1. `task partition`
   - `.system.md` 里明确列出的并发 task，优先原样保留为 `[tasks]` 名称。
   - 不要把 `feed_prep`、`orient_stage`、`transfer_to_measure` 这类并发工位压平成单循环大 task。

2. `blocking / timeout`
   - 所有显式等待都落到 `wait` / `delay` / `axis.move_*`。
   - manual wait 才用 `allow_indefinite_wait: true`。
   - 非 manual wait 默认要有 `timeout -> goto real_task.step`。
   - 如果交付要求当前 scenario 工具链可直接使用，优先避免把关键等待压成单条复合 `wait` guard。

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

## project-level authoring 顺序

对 scaffolded station / module / line，优先：

1. 把确认版 system fact 落到 delivery asset `docs/*.system.md`
2. 让 root `plc/main.system.md` 只承担项目级 bridge / 索引角色，不要让它替代 delivery asset docs
3. 明确 delivery asset `main.bundle.toml` 是本轮 authoritative source entry
4. 清掉 delivery asset docs 与 sidecar 中的 scaffold placeholder
5. 再把上述 6 个桶拆到 fragments
6. 修 scenario
7. 修 intent sidecar 或显式报 blocker

不要只改 root `plc/main.system.md`，却把 delivery asset docs 保持 scaffold 默认文案。
如果 delivery asset docs 仍出现 `Default Starter Flow` 或 `starter intent contract`，说明 lowering 还没真正落盘。

回答时建议先给 5 到 10 行 lowering 摘要，再进入 bundle / fragments 或 scaffold 交付。
