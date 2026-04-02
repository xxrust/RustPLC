# 语义资源互锁开发指南（Semantic Resource Interlock Development Guide）

本文档说明如何把 SRI 规范落到 RustPLC 现有分层中。

本文档不是冻结合同；它服务于实现顺序、风险控制、迁移方式和测试门禁。

## 1. 为什么选 SRI，而不是继续扩 `safety`

### 1.1 不选“动作字面量直接进 safety”

不采用：

```plc
safety: axis.move_absolute(axis_arm, position: 0) conflicts_with cyl_feed.extended
```

原因：

1. `safety` 将直接依赖 AST 级动作结构
2. safety 规则会绑死数值字面量与语句写法
3. runtime / verification 将被迫重复理解动作 payload
4. 会破坏“IR 是唯一语义汇合点”

### 1.2 不选“继续用 pulse.active 近似”

不采用：

```plc
safety: manual_feed_forward.on conflicts_with axis_arm.pulse.active
```

原因：

1. 粒度过粗，会误伤无关 motion
2. 左侧常常是输入请求，不是实际危险姿态
3. 无法稳定表达“某个动作语义”而不是“整根轴在动”

### 1.3 选“资源占用 + 动作标签”

采用：

- 资源是稳定工业语义
- 标签是动作语义入口
- claim 是资源占用绑定

这样做的优点是：

1. `safety` 仍然保持“检查状态/占用是否同时成立”的风格
2. 特定 motion 语义能进入 IR 与 pending action 生命周期
3. 同一模型可以同时覆盖：
   - motion vs posture
   - posture vs posture
   - motion vs motion

## 2. 分层改造路径

## 2.1 Parser

建议联动文件：

- `src/parser/plc.pest`
- `src/parser/mod.rs`
- `src/ast/mod.rs`

新增解析对象：

1. `resource` 声明
2. `claim` 约束
3. `semantic_tag:` 动作元数据

建议原则：

1. `semantic_tag` 不进入 `axis.move_*` 参数白名单；应作为动作附属元数据解析
2. `claim` 在语法层只负责结构，不裁决标签是否被使用
3. 不在 parser 阶段做 `position -> semantic_tag` 自动推断

## 2.2 AST

建议新增：

- `SemanticResourceDeclaration`
- `ResourceClaimConstraint`
- `ClaimSource`
- `semantic_tag: Option<String>` on supported action statement

AST 只保留源码结构，不做：

- 标签等价归并
- 资源占用生命周期判定
- runtime 可执行化

## 2.3 Semantic

建议联动文件：

- `src/semantic/mod.rs`
- `src/diagnostics.rs`
- `src/ir/mod.rs`

语义阶段必须完成：

1. 资源名唯一性校验
2. claim 引用资源存在性校验
3. `action_tag` claim 至少绑定到一个受支持动作
4. `semantic_tag` 仅出现在 SRI-v1 支持的动作上
5. 明确拒绝位置数值自动推断

语义阶段不应做：

1. 根据 `position: 0` 自动补标签
2. 根据 step 名自动补标签
3. 把资源冲突偷偷重写回 `safety conflicts_with`

### 2.3.1 推荐 canonicalization

推荐把资源 claim 在 semantic 阶段统一降级到 IR：

- `State(StateExpr)`
- `ActionTag(tag)`

不要在 runtime 或 verification 中重新解释 AST。

## 2.4 IR

建议联动文件：

- `src/ir/mod.rs`

推荐新增：

1. `SemanticResource`
2. `ResourceClaimRule`
3. `ClaimSource`
4. `semantic_tag` on `TransitionAction::AxisMoveRelative`
5. `semantic_tag` on `TransitionAction::AxisMoveAbsolute`
6. `semantic_tag` on `PendingActionContext`

为什么 `PendingActionContext` 也要带标签：

- 资源占用是在 pending 生命周期中持续存在的
- verification 与 runtime 都不能只看 transition 发起瞬间

## 2.5 Verification

建议联动文件：

- `src/verification/safety.rs`
- `src/verification/liveness.rs`
- `src/verification/timing.rs`
- `src/verification/causality.rs`

### 2.5.1 safety

safety 是 SRI 的主验证入口。

实现口径建议：

1. 在全局状态中保留既有 `task_pending`
2. 再引入“当前活跃 pending action 的 semantic_tag 集合”视图
3. 每个状态节点计算：
   - 哪些 `state claim` 成立
   - 哪些 `action_tag claim` 成立
4. 对每个独占资源，若 holder 数量 > 1，则报错

错误报告建议至少包含：

- 资源名
- 活跃 holder 列表
- 路径
- 触发来源是 state claim 还是 action_tag claim

### 2.5.2 liveness

liveness 不需要为 SRI 增加新的证明算法，但要覆盖：

1. 因资源冲突进入 `on_safety_fault`
2. 因无冲突而动作成功完成

### 2.5.3 timing

timing 应保留两条口径：

1. 正常无冲突路径
2. 资源冲突后分流路径

避免 timing 只统计成功路径而忽略安全分流。

### 2.5.4 causality

causality 不需要理解几何空间，但建议加两项门禁：

1. `action_tag` claim 的标签必须能追溯到具体动作
2. 未绑定动作的标签必须显式报错，不得静默忽略

## 2.6 Runtime Bridge

建议联动文件：

- `src/runtime_bridge.rs`

bridge 负责把 SRI 语义降成 runtime 可执行数据，而不是现场临时判断。

建议职责：

1. 生成资源表
2. 生成 claim 规则表
3. 把 `semantic_tag` 附到 runtime pending action 元数据
4. 构建动作发起时的资源冲突检查元数据

bridge 不应做：

1. 基于数值位置猜语义标签
2. 基于 step 名临时拼标签

## 2.7 runtime-core

建议联动文件：

- `crates/runtime-core/src/lib.rs`

runtime-core 的关键职责是把资源占用做成真实执行约束。

建议执行规则：

1. 动作发起阶段：
   - 若动作带 `semantic_tag`
   - 先检查其对应资源是否已被其他 claim 占用
   - 若会冲突，则走 `safety_fault`
2. 动作 pending 阶段：
   - claim 继续保持
3. 动作终态阶段：
   - 释放 claim
4. 状态占用：
   - 根据当前设备状态实时评估，不依赖动作历史

### 2.7.1 为什么资源冲突走 `safety_fault`

推荐固定为 `safety_fault`，不走 `reject`：

- `reject` 更适合参数非法、前置条件缺失、调度不接受
- 资源互锁冲突本质是安全域禁止
- 与现有 `axis.move_*` 的 `on_safety_fault` 桶最一致

## 2.8 Codegen

建议联动文件：

- `src/codegen/st.rs`

SRI 上线初期建议采取保守策略：

1. 若 ST 目标尚无等价实现，直接拒绝 codegen
2. 不允许把带 SRI 的程序静默降成“无互锁” ST

这比生成错误语义的代码安全得多。

## 3. 推荐实施顺序

建议按以下顺序推进：

1. 冻结文档与诊断编号
2. parser / AST / semantic / IR 打通
3. safety verifier 打通
4. runtime bridge / runtime-core 打通
5. codegen 再决定支持还是显式拒绝
6. 最后迁移示例与 skills

原因：

- 没有 verifier 与 runtime 共享的 IR，占用语义会再次下沉
- 先打通 codegen 没有意义，因为 codegen 不能反向发明语义

## 4. wafer_loader 的推荐迁移方式

不推荐继续保留：

```plc
safety: manual_feed_forward.on conflicts_with axis_arm.pulse.active
```

推荐拆成“真实危险姿态 + 特定动作语义”：

```plc
[topology]
resource slide_pick_zone: semantic_resource {
    mode: exclusive
    purpose: "滑轨取片空间占用"
}

[constraints]
claim: cyl_feed.extended occupies slide_pick_zone
claim: action_tag arm_pick_to_slide occupies slide_pick_zone
```

并在“去滑轨取片”的几个 motion 上显式打标签：

```plc
action: axis.move_absolute(axis_arm, position: 0, params: stepper_default_fast, speed: 5)
    semantic_tag: arm_pick_to_slide
```

这样可以得到三个收益：

1. 只拦“去滑轨”的动作，不拦 `45` / `90`
2. 左侧改成真实姿态 `cyl_feed.extended`，不再依赖输入按钮 `.on`
3. runtime 与 verification 的语义对象一致

## 5. 测试门禁建议

SRI 上线时，至少补齐以下测试层：

### 5.1 parser / semantic

1. 资源声明成功
2. 未知资源报错
3. 未绑定 `action_tag` 报错
4. `semantic_tag` 用于不支持动作时报错
5. 试图自动推断标签时报错

### 5.2 safety verification

1. `state claim` vs `action_tag claim` 冲突反例
2. `state claim` vs `action_tag claim` 正例
3. 两个不同 `action_tag` 同资源冲突
4. 多 task 并发下资源冲突检出

### 5.3 runtime

1. 发起带标签动作时资源已被占用，应走 `on_safety_fault`
2. 动作 pending 期间 claim 保持
3. 动作 Done / Fault / Timeout 后 claim 释放
4. 不带标签的其他 motion 不应错误占用资源

### 5.4 examples

建议新增：

- 一个最小 `resource interlock` fixture
- 一个 `wafer_loader` 精简版 fixture

## 6. 迁移策略

建议分三阶段：

### 阶段 A：引入语义，不迁移业务

- 加入 SRI 语法、IR、verification、runtime
- 现有示例暂不替换

### 阶段 B：示例并行表达

- 保留旧的 `pulse.active` 近似规则
- 同时引入 SRI 表达
- 用测试对比两者差异，确认 SRI 更精确

### 阶段 C：移除粗粒度近似

- 删除旧的 `pulse.active` 临时互锁
- 统一改成资源占用模型

注意：

- 阶段 B 不能长期保留，否则会出现双重约束、重复误报

## 7. 对 skills 与文档的同步要求

SRI 一旦落地，以下内容必须同步：

1. `AGENTS.md`
2. `docs/architecture/signal-direction.md` 中对 pending action 的相关表述
3. `.codex/skills/plc-system`
4. `.codex/skills/plc-gen`
5. `docs/wafer_loader.system.md`
6. 相关 examples 与 tests

尤其要避免继续在 skill 模板里把“特定 motion 语义”退化为 `pulse.active`。

## 8. 最终裁决

如果未来再出现下面这类需求：

- “某个动作去某个姿态时，与某机构占位互斥”
- “某个 motion 语义与另一机构当前危险姿态互斥”
- “并非整轴运动都危险，只有特定 motion 危险”

优先进入 SRI，而不是再补：

- `*.pulse.active`
- 输入按钮 `.on`
- step 名字上的隐式约定

这条裁决的目标只有一个：

把“动作语义选择性互锁”从示例层补丁，上提为 RustPLC 的稳定一等语义。

