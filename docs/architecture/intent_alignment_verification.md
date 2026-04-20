# Intent Alignment Verification

## 1. 文档角色

本文档定义 RustPLC 中“工艺意图链”和“程序行为链”的一致性验证。

它解决的核心问题只有一个：

工艺意图要求 `A -> B -> C`，程序实际行为却是 `A -> C`，即使程序合法、可运行、形式验证通过，也必须判定为错误。

这类错误不是 parser、semantic、runtime、fault routing 或四类 formal verification 的主问题。  
它们解决的是“程序是否成立”，本文档解决的是“程序是否做了对的事”。

## 2. 问题定义

意图对齐问题的最小形式是：

- 意图链：业务上必须发生的步骤与顺序
- 行为链：程序在仿真或运行中实际发生的步骤与顺序

当两者不一致时，就发生了 intent mismatch。

最典型的 mismatch 不是 fault 分类错误，而是：

- 必经步骤缺失
- 必经顺序被改写
- 看起来完成了，但业务后置条件不成立
- 当前周期看起来正确，下一周期开始漂移

## 3. 两个序列

### 3.1 Intent Sequence

`intent sequence` 是工艺意图的最小业务合同。

它不是 DSL，不是 task.step 源码，也不是 trace。

它只表达 4 类东西：

1. 必经步骤
2. 必经顺序
3. 周期结束时必须成立的业务后置条件
4. 下一周期开始前必须恢复的前提

示意：

```text
start_cycle
-> A_completed
-> B_completed
-> C_completed
-> cycle_restartable
```

这里的 `A_completed / B_completed / C_completed` 是业务里真正要发生的里程碑，不要求等于源码里的 step 名字。

### 3.2 Behavior Sequence

`behavior sequence` 是程序在给定场景下实际产生的业务里程碑序列。

它来自仿真和运行证据，不来自人工脑补。

证据来源可以包括：

- runtime trace
- task context
- 设备动作记录
- 结束时的设备/资源状态
- 紧接下一周期的启动结果

示意：

```text
start_cycle
-> A_completed
-> C_completed
-> ready
```

这条行为链虽然“跑通了”，但与意图链不一致，因为缺失 `B_completed`。

## 4. 一致性判定规则

一致性判定只看 4 个维度。

### 4.1 Required-Step Coverage

意图链中的每个必经步骤都必须在行为链中出现。

如果意图要求 `A -> B -> C`，行为是 `A -> C`，则判定：

- `missing_required_step(B)`

这是最高优先级错误。

### 4.2 Ordering Conformance

意图链规定的先后关系必须被保持。

如果意图要求：

- `A` 必须先于 `B`
- `B` 必须先于 `C`

行为出现：

- `A -> C -> B`
- `B -> A -> C`

则判定：

- `wrong_order`

这里不要求行为链和意图链逐字相同，但要求所有必经步骤之间的业务顺序保持一致。

### 4.3 Postcondition Conformance

当前周期结束时，必须满足意图规定的业务后置条件。

这一步验证的不是“任务回到哪个 step”，而是“业务是否真的完成”。

典型后置条件包括：

- 机构释放
- 工件进入下一工位可接受状态
- 所有参与动作的气缸回到规定姿态
- 系统确实处于业务上可接受的结束状态

如果行为链完成了所有路径步骤，但后置条件不成立，则判定：

- `postcondition_not_met`

### 4.4 Next-Cycle Conformance

意图对齐不是单周期问题，还必须验证下一周期是否从正确起点重新开始。

如果本周期结束后：

- 下一周期跳过了某个必经步骤
- 从中间状态继续跑
- 重复执行上一周期尾部动作

则判定：

- `cross_cycle_drift`

这一步专门用于抓“本轮看起来正常，第二轮开始错”的问题。

## 5. Mismatch 类型

本文档固定使用以下 mismatch 类型：

1. `missing_required_step`
2. `wrong_order`
3. `duplicated_required_step`
4. `premature_readiness`
5. `postcondition_not_met`
6. `cross_cycle_drift`

解释：

- `missing_required_step`：必经步骤漏掉了
- `wrong_order`：步骤存在，但顺序错了
- `duplicated_required_step`：某个必经步骤被重复执行
- `premature_readiness`：程序进入了 `ready` 或等价等待点，但业务上还不能重新开始
- `postcondition_not_met`：当前周期结束时业务后置条件不成立
- `cross_cycle_drift`：下一周期出现跳步、漏步、重复动作或错误起点

其中最根本的是前两项：

- 缺步骤
- 错顺序

## 6. 验证流程

每条意图对齐验证都按固定顺序执行：

1. 先写 intent sequence。
2. 把每个意图节点映射成可观察里程碑。
3. 通过 scenario 驱动程序运行，取得 behavior sequence。
4. 先比较 required steps 是否全覆盖。
5. 再比较 ordering 是否保持。
6. 再检查本周期 postcondition。
7. 最后检查下一周期是否从正确起点重新开始。

顺序不能反过来。

如果先看 `ready`、先看 terminal、先看 fault route，而不先看 required steps 和 ordering，就会把 `A -> C` 误判成“基本正确”。

## 7. Cylinder 中应验证的不是 fault route，而是意图链

在 cylinder 场景里，编译链和 runtime 已经能处理很多基础问题，例如：

- timeout 路由
- motion fault 路由
- safety fault 路由

本文档不重复验证这些。

本文档只验证它们之上那一层：

- 这个 cylinder 在工艺里是不是必须先做 A 再做 B
- recovery 之后是不是已经恢复到允许下一步的业务状态
- `ready` 是否真的等于“可重新启动”
- 第二周期是否还保持同一意图链

## 8. Cylinder 例子

### 8.1 双气缸顺序意图：A -> B

工艺意图链：

```text
start_cycle
-> cyl_A_extend_completed
-> cyl_A_retract_completed
-> cyl_B_extend_completed
-> cyl_B_retract_completed
-> cycle_restartable
```

下面这些行为都必须判错：

```text
start_cycle -> cyl_A_extend_completed -> cyl_B_extend_completed -> cycle_restartable
```

错误：

- `missing_required_step(cyl_A_retract_completed)`
- `postcondition_not_met`

```text
start_cycle -> cyl_B_extend_completed -> cyl_A_extend_completed -> ...
```

错误：

- `wrong_order`

```text
start_cycle -> cyl_A_extend_completed -> cyl_A_retract_completed -> cycle_restartable
```

错误：

- `missing_required_step(cyl_B_extend_completed)`
- `missing_required_step(cyl_B_retract_completed)`

### 8.2 单气缸 recovery 意图

工艺意图链：

```text
fault_detected
-> safe_home_restored
-> cycle_restartable
```

如果行为是：

```text
fault_detected -> ready
```

则即使程序回到了等待状态，也必须判错：

- `premature_readiness`
- `postcondition_not_met`

因为这里缺的不是 runtime fault route，而是业务要求的 `safe_home_restored`。

### 8.3 多气缸工位 recovery 意图

工艺意图链：

```text
fault_detected
-> all_required_cylinders_retracted
-> all_required_drives_stopped
-> cycle_restartable
```

如果行为是：

```text
fault_detected
-> some_cylinders_retracted
-> ready
```

则必须判错：

- `premature_readiness`
- `postcondition_not_met`

如果第一轮恢复看起来成功，但第二轮启动时直接跳过前置复位步骤，则必须判错：

- `cross_cycle_drift`

## 9. 文档落地方式

意图对齐文档应该先写意图链，再写观测规则。

推荐固定结构：

1. 场景名
2. intent sequence
3. 可观察里程碑定义
4. 必须保持的顺序关系
5. 周期结束后必须满足的 postconditions
6. 下一周期必须满足的起点条件
7. mismatch 规则

不要先写 fault route，不要先写 DSL step，不要先写 CLI。

## 10. 结论规则

只有同时满足以下 4 条，才能判定“程序与真实工艺意图对齐”：

1. 行为链覆盖了所有 required steps。
2. 行为链保持了所有 required ordering。
3. 当前周期结束时满足业务 postconditions。
4. 下一周期从正确起点开始，没有 drift。

缺任意一条，都不能判定为意图对齐。

## 11. Phase-2 固定关闭集

phase-2 的关闭集固定为以下 3 类资产，达到后即可收口，不再继续把新 case 无限塞回本阶段：

1. Canonical fixtures
   - `tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json`
   - `tests/fixtures/intent_alignment/contracts/recovery_single_contract.json`
   - `tests/fixtures/intent_alignment/contracts/recovery_multi_contract.json`
   - `tests/fixtures/intent_alignment/evidence/*.jsonl`
2. Canonical + mutation regressions
   - `tests/intent_alignment_pipeline.rs` 必须覆盖顺序动作、单执行器 recovery、多执行器 recovery，以及每个已冻结 mismatch 的最小 mutation 反例。
3. Real golden path
   - `examples/dual_axis_platform.plc`
   - `tests/fixtures/intent_alignment/traces/two_cycle_aligned.jsonl`
   - golden path 必须经过 `contract -> extractor -> comparator -> report` 主链，并断言显式语义，而不是只断言流程能跑通。

超出以上关闭集的新 mismatch、新 evidence source 或新的 CLI 入口，统一后置到下一阶段，不在 phase-2 内继续扩张。
