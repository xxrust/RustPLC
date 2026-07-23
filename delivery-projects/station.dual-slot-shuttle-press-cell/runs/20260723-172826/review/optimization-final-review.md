# Complex Project Optimization Final Review

## 最终修正后限定复核

本节以 `result.json` 指向的权威运行 `harness-runs/20260723-172826` 为准。上一轮 findings 全部保留在后文，状态更新如下。

- Harness 执行：`pass`
- Acceptance：`blocked`
- AC 分布：12 pass / 5 blocked / 0 fail
- 严格 AC 完成率：`70.6%`
- 修正后实现完成度：`84%`
- 修正后 Harness 证据完整度：`86%`

### Finding 状态

| 原 finding | 状态 | 修正后证据 |
| --- | --- | --- |
| High-1 `manual_assist` 未真正释放 startup | **Resolved** | startup 新增 `classify_residual_state -> wait_manual_ack -> route_after_manual_ack -> recheck_residual_clear`。residual trace 为 detected tick 0、cleared tick 30、manual ack tick 40、startup consume ack tick 41、startup ready tick 51，因果前置已成立。 |
| High-2 one-shot 被 intent `aligned` 掩盖 | **Resolved as explicit blocker** | runner 新增 `external_reinit_intent_schema_support=known_gap`，明确 `aligned` 只证明七个 nominal milestone，不证明 external replenishment 与 runtime recreation。schema 能力本身仍 blocked。 |
| Medium-1 AC-09 持续性 oracle 不完整 | **Resolved** | before-ready 断言 1 次 recycle 后接受后续 edge；running 断言 running reject、batch-complete reject 和 2 次 recycle；faulted 断言 2 次 reject 和 2 次 recycle，且无 accept/acquire。 |
| Medium-2 HMI binding 未闭合 | **Open** | PLC 内部 code producer 保持通过；目标 HMI register/transport/consumer binding 仍是 integration blocker。 |
| Medium-3 同 run specimen 未冻结 | **Resolved by digest manifest** | `input-manifest.json` 包含 49 个唯一输入，其中 implementation 41 个；`missing_required` 为空，manifest SHA-256 写入 `result.json`。完整 specimen 已具备同 run 文件级审计依据。 |
| Low-1 trace oracle 使用数字索引 | **Open, low severity** | 新 oracle 覆盖更完整，但 task/step numeric id 仍可能随合法重排而漂移。 |

### 四项限定确认

1. **Manual ack 已成为 startup 因果前置。** `defaults.plc` 的 residual=true 分支必须等待 `manual_acknowledged == true`，随后执行物理 residual 清零复查。runner 同时检查 manual ack 发布与 startup ack 消费的先后顺序。
2. **AC-09 recycle 与重复 edge 已闭合。** 当前四条 trace 和 runner 计数共同证明 before-ready、running、faulted 三类拒绝均返回持续监听入口；running 还覆盖批次完成后的第三个 start，faulted 覆盖第二个 start。
3. **External reinit 已明确 schema-blocked。** intent 的 `aligned` 不再被解释为 restart 通过证据。剩余工作是产品 schema 支持 `external_reinitialization`，不是 specimen 内继续伪造第二周期。
4. **Input manifest 覆盖完整 specimen。** 同 run 记录 `file_count=49`、`implementation_file_count=41`、`missing_required=`，49 条路径唯一，并在 `result.json` 中记录 manifest digest。

### 修正后剩余 Findings

- **High blockers:** `GAP-SIM-FAULT-INJECTION` 与 `GAP-PROCESS-TRANSFORM-CARRIER` 继续阻塞 AC-06、AC-07、AC-11、AC-12、AC-14。
- **Schema blocker:** `GAP-EXTERNAL-REINIT-INTENT-SCHEMA` 已被诚实分类，能力仍未实现。
- **Integration blocker:** `GAP-HMI-TRANSPORT-BINDING` 仍阻止“真实 HMI 可见”声明。
- **Runtime blocker:** `GAP-RESIDUAL-TOKEN-INJECTION` 仍无法表示启动前精确残件 token 位置。
- **Deployment evidence:** 目标硬件 timing 证据仍缺失；no-board 数据不能替代目标硬件测量。
- **Low harness risk:** trace transition 仍使用 numeric task/step id，建议后续从同 run IR 按名称解析。

修正后的 `84%` 表示 specimen 内部的两项错误因果归因已经关闭，front-door 与同 run 输入证据已显著增强。交付状态仍必须保持 `blocked`，因为 5 个 hard AC 和多个产品层能力缺口尚未解除。

## 历史首轮复核（已由上方状态取代）

本次独立复核以 `result.json` 指向的权威运行 `harness-runs/20260723-165501` 为准。

- Harness 执行：`pass`
- Acceptance：`blocked`
- AC 分布：12 pass / 5 blocked / 0 fail
- 严格 AC 完成率：`12 / 17 = 70.6%`
- 独立评估的实现完成度：`76%`
- Harness 证据完整度：`72%`

当前项目已经具备可编译、可验证、可运行的有限两件 nominal 主链。AC-09 的三类 front-door 行为在当前 trace 中真实发生，PLC 内部状态码和故障码进入 IR，物理残件输入也能触发 `manual_assist`。项目仍不满足完整验收，原因包括 5 个已记录产品 blocker，以及两个会造成错误因果归因的高严重度问题。

## Findings

### High-1: `manual_assist` 可达，但没有真正释放 startup

`startup_self_check` 的执行许可只依赖两个外部事实：

1. `residual_present == false`
2. `rising_edge(operator_empty_confirm)`

`manual_assist` 同时消费相同的两个事实，并在之后写入 `manual_acknowledged = true`。startup 从未读取 `manual_acknowledged`。权威 trace 明确展示：

- tick 30：startup 的残件等待完成，同时 manual task 的残件清除等待完成。
- tick 40：startup 消费空机确认并继续自检；manual task 也消费同一上升沿并发布确认。
- tick 50：startup 发布 ready。

这条链路证明 manual task 是并发观察者，不是 startup 的授权者。`run_selftest.ps1` 将“manualConfirmed 早于 startupReady”解释为 `residual_manual_assist_blocks_and_releases_startup`，属于顺序相关性替代因果依赖。

影响：

- `plc/main.system.md` 中“manual_assist ... before startup may continue”的语义没有进入执行模型。
- AC-16 当前 pass 不能证明 manual-assist admission 闭环。
- 删除 `manual_acknowledged` 写入后，现有 startup 仍会通过，说明该信号当前没有控制作用。

建议修正：startup 先分类 residual 状态。无残件路径直接等待空机确认；有残件路径等待 `manual_acknowledged == true`，随后重新证明 `residual_present == false` 再进入安全自检。对应 harness 应验证 startup transition 对 manual ack 的真实依赖，而不只比较四个 trace 时间戳。

### High-2: one-shot 文档诚实，intent 结果仍存在 restart 假阳性

最新结果已经诚实记录：

- `GAP-ONE-SHOT-RESTART-CONTRACT`
- `GAP-EXTERNAL-REINIT-INTENT-SCHEMA`
- nominal 只证明单批次，不证明 in-process second cycle。

同时，`project-check` 的 intent step 仍返回 `aligned`，runner 又把 `project_check_failure_isolated` 标为 pass，并把 AC-12 的阻塞原因仅归结为 process-model `OP-003`。这会让下游读取者误以为 restart semantics 已对齐。

影响：intent comparator 的 `aligned` 只能证明当前七个 milestone 的单周期顺序，不能证明 external replenishment、runtime recreation 或下一周期启动能力。

建议修正：在 schema 支持 `restart_policy.kind = external_reinitialization` 前，harness 应把 restart 维度单独标为 `blocked/schema_gap`。AC-12 的证据应同时列出 `OP-003` 与 external-reinit schema gap，不应使用“overall only blocked by OP-003”的表述。

### Medium-1: AC-09 当前实现通过，但 runner 的持续性 oracle 不完整

当前源码与 trace 支持 AC-09：

- before-ready：先拒绝，之后接受新 edge。
- running：第二次 start 被拒绝。
- faulted：fault 锁存后 start 被拒绝。
- running/faulted trace 都出现 `task=1;from=18;to=0;reason=action`，说明 rejection 后实际返回等待入口。

runner 的 AC-09 判定没有断言 `18 -> 0`。running 只检查 reject、首次 accept、首次 acquire 各一次；faulted 只检查 reject 一次且无 accept/acquire。若未来 `recycle_after_reject` 不再返回等待入口，这两组 oracle 仍可能通过。

建议修正：

- 三类拒绝都断言 rejection 后存在 `recycle_after_reject -> wait_start_cycle`。
- running 场景在批次完成后再发第三个 edge，验证 front-door 仍服务且进入 batch-complete rejection。
- faulted 场景发两个独立 edge，验证 faulted rejection 可重复消费。
- 从 IR 的 task/step 名称解析 transition id，减少硬编码 index 漂移。

### Medium-2: PLC 内部 status/fault code 已进入 IR，HMI 可见性仍未闭合

权威 IR 与 runner 的 13 项 mapping oracle 证明以下能力已经存在：

- startup、running、batch-complete 状态码 producer。
- axis timeout/reject/motion/safety、cylinder timeout/retract-failed、residual fault code producer。
- `accept_cycle` 清除旧 `hmi_fault_code`。

当前变量类型是 `float`，语义上作为离散枚举码使用。项目没有目标寄存器、传输协议或 HMI consumer binding。最新 result 已将其记录为 `GAP-HMI-TRANSPORT-BINDING`，这个边界是诚实的。

残余问题：系统文档中的“visible HMI status/fault code”仍是未履行的交付义务。对外报告应使用“PLC-internal numeric code producer”，直到部署绑定被同 run 证据验证。若目标 HMI 要求整数寄存器，还需冻结 float 到寄存器整数的转换规则，并禁止小数写入。

### Medium-3: AC-15 只验证证据路径，未冻结本次 specimen

`same_run_evidence_paths` 只检查 step log 和 artifacts 是否位于当前 run 目录。`result.json` 记录 runner、acceptance、intent source 和 manifest digest，但没有记录 bundle、所有 `.plc` fragment、scenario、process model 与 intent contract 的完整 digest，也没有把输入 specimen 快照复制到 run 目录。

影响：文件在运行后被修改时，`result.json` 仍可能继续指向同一路径，机械验证无法证明当前文件就是本次执行输入。当前 run 的时间关系一致，但 harness contract 仍缺少可重复审计能力。

建议修正：运行前复制完整 specimen 到 `harness-runs/<id>/inputs/`，所有命令只消费该快照；或者生成包含每个输入文件相对路径和 SHA-256 的 `input-manifest.json`，并把其 digest 写入 `result.json`。

### Low-1: 多个 trace oracle 依赖 task/step 数字索引

AC-04、AC-09、residual/manual 与 concurrency oracle 使用固定 `task=N;from=N;to=N`。这能锁定当前编译布局，但会把正常的 step 插入、task 重排和语义变化混在一起。建议由同 run IR 建立 `task_name.step_name -> numeric id` 映射，再执行 trace 断言。

## Blockers

| Blocker | 当前证据 | 验收影响 |
| --- | --- | --- |
| `GAP-SIM-FAULT-INJECTION` | 三个 fault scenario trace 与 nominal 字节一致 | AC-06、AC-07、AC-14 blocked |
| `GAP-PROCESS-TRANSFORM-CARRIER` | expected/actual operation 都为 10，仅剩 OP-003 | AC-11、AC-12 blocked |
| `GAP-EXTERNAL-REINIT-INTENT-SCHEMA` | phase-2.v1 无法表达 external runtime recreation | restart intent 不能判定 aligned |
| `GAP-RESIDUAL-TOKEN-INJECTION` | 只能证明物理残件汇总信号，不能注入并保持精确既有 token 位置 | 精确 residual recovery blocked |
| `GAP-HMI-TRANSPORT-BINDING` | 内部 code 在 IR，目标 HMI binding 缺失 | HMI 可见性 blocked |
| `GAP-HARDWARE-TIMING-EVIDENCE` | no-board p99 通过，静态预算仍告警且无目标硬件数据 | 部署时序 blocked |
| Manual-assist causal gap | startup 不消费 `manual_acknowledged` | 当前 claim 需要降级或修正 |

## Subagent 完成度

### Front-door optimization subagent

- 建议完整度：`92%`
- 主实现采纳度：`88%`

有效贡献：准确识别一次性 front-door 的根因；提出持续 edge consumer、同 task 分类、running/faulted 专用拒绝、拒绝后回收以及真实 scenario/trace oracle。当前实现基本遵循该结构，且权威 trace 证明三类行为发生。

偏差与遗漏：harness 没有落实其“拒绝后返回 wait”的硬断言；running/faulted 场景没有用后续第三/第二 edge 证明持续服务。实现选择在 running/faulted rejection 中保留原 machine/fault code，只写独立 reject reason，这一偏差合理，避免覆盖真实运行/故障状态。

未发现该 subagent 花大量时间盲目试错的证据。其建议路径集中，主要问题发生在主实现的 oracle 收口阶段。

### Residual/HMI optimization subagent

- 建议完整度：`94%`
- 主实现采纳度：`70%`

有效贡献：准确识别 residual producer 缺失、manual task 不可达、精确 token injection blocker、HMI binding 边界和 external-reinit schema gap；物理 residual input、operator confirmation、内部状态码/故障码以及诚实 blocker 记录均被采纳。

关键偏差：subagent 要求形成 `operator confirmation -> residual recheck -> startup continuation` 的闭环。主实现采用两个并发 root task 同时消费相同输入，`manual_acknowledged` 没有进入 startup admission。这是当前最主要的实现完整度损失。

未发现该 subagent 存在大量无效试错。分析本身已指出正确边界，执行偏差来自 lowering 选择没有保留建议中的因果链。

## 完成度判定

`76%` 是当前合理的项目完成度：

- nominal compile/runtime/verification 主链完整。
- front-door 当前行为成立，证据 oracle 仍需加固。
- residual 检测与 manual task 可达，manual 对 startup 的授权因果未成立。
- status/fault code 已进入 IR，部署 HMI binding 未完成。
- one-shot 文档已经收敛，intent schema 与 aligned verdict 仍不具备 external-reinit 表达能力。
- 5 个 hard AC 保持 blocked，不能将交付状态提升为 validated 或 validated-with-warnings。

下一轮修正顺序应为：

1. 修复 manual-assist 到 startup 的真实因果依赖并升级 oracle。
2. 将 restart intent 从 `aligned` 主张中拆出，显式标记 schema-blocked。
3. 加固 AC-09 recycle/重复 edge oracle。
4. 冻结同 run 输入快照或完整 digest manifest。
5. 再处理 fault injection、OP-003、HMI binding 与目标硬件时序产品 blocker。
