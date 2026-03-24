# 语义资源互锁规范（Semantic Resource Interlock, SRI）

本文档冻结“动作语义选择性互锁”的一等语义模型，作为 parser / semantic / IR / verification / runtime bridge / runtime-core 的统一合同。

本文档解决的问题不是“再给 `safety` 多加几种写法”，而是把工业控制里稳定存在的“空间/机构/风险资源占用”显式建模为可验证、可执行、可诊断的系统语义。

## 1. 背景与问题

当前 RustPLC 的 `safety` 约束左右两侧只支持：

- 状态引用：`device.state` 或 `device.port.state`
- 模拟量阈值：`AI0 > 10`

当前 safety 引擎对 `axis.move_relative` / `axis.move_absolute` 的安全建模，会把动作统一收敛为轴 `pulse` 端口进入 `active` 状态，而不会区分：

- 该动作是去哪个目标位
- 该动作对应哪个工艺语义
- 该动作占用哪个机械空间或风险窗口

因此，下面两类语义目前无法被原生、精确地表达：

1. 仅对某个特定 motion 语义互锁，而不是对整根轴的所有运动互锁
2. 某个动作与某个机械空间占用互锁，而不是与整个动作窗口粗粒度互锁

`docs/wafer_loader.plc` 中的以下写法正是这个问题的典型表现：

```plc
safety: manual_feed_forward.on conflicts_with axis_arm.pulse.active
safety: manual_swing_down.on conflicts_with axis_arm.pulse.active
safety: step_transfer_cycle.on conflicts_with axis_arm.pulse.active
```

这些规则表达的是“输入请求/步骤请求”与“整根轴的运动窗口”互锁，而不是“出料气缸前进”与“旋臂转向滑轨动作”互锁。

## 2. 目标

本规范的目标固定为：

1. 支持对“特定动作语义”进行精确互锁，而不是被迫退化为整轴 `pulse.active`
2. 让该语义进入 IR，成为 runtime 与 verification 的共享对象
3. 保持 `safety` 的主语义边界稳定，不让 `safety` 直接依赖 AST 级动作字面量
4. 允许一个资源同时由：
   - 某个长时动作语义占用
   - 某个设备状态占用
5. 为并发 task 建模提供稳定、可诊断的冲突检测口径

## 3. 非目标

本规范明确不做：

1. 不支持在 `safety` 中直接引用原始动作字面量，例如：

```plc
safety: axis.move_absolute(axis_arm, position: 0) conflicts_with cyl_feed.extended
```

2. 不根据 `position: 0`、`position: 45`、`position: 90` 这类数值自动推断工业语义
3. 不把几何计算、碰撞区求解、姿态映射下沉到 DSL 运行期
4. 不要求编译器自动从 step 名、goto 名、变量名推断资源占用
5. 不改变并发 task / blocking step 的既有调度语义

## 4. 选定抽象

本规范选定的核心抽象是：

- **语义资源（semantic resource）**
- **资源占用声明（resource claim）**
- **动作语义标签（action semantic tag）**

### 4.1 语义资源

语义资源是对某个工业风险边界、机械空间、工装占用或互斥执行域的显式命名。

示例：

- `slide_pick_zone`
- `swing_transfer_zone`
- `clamp_press_clearance`
- `shared_robot_reach`

语义资源不是设备，不等同于某个 I/O 点，也不等同于某个 step 名。

### 4.2 资源占用声明

资源占用声明回答一个固定问题：

> “什么条件成立时，某个语义资源被认为处于占用态？”

占用来源分两类：

1. **状态占用（state claim）**
   - 当某个状态表达式成立时，占用资源
   - 适合表达持久姿态或机构已进入危险空间
   - 例如：`cyl_feed.extended occupies slide_pick_zone`

2. **动作标签占用（action-tag claim）**
   - 当某个带标签的长时动作处于挂起生命周期时，占用资源
   - 适合表达“正在执行某种运动意图”
   - 例如：`action_tag arm_pick_to_slide occupies slide_pick_zone`

### 4.3 动作语义标签

动作语义标签是对“该动作的工业意图”做显式命名，而不是复用数值参数或 step 名。

示例：

- `arm_pick_to_slide`
- `arm_move_to_orient`
- `gantry_enter_press_zone`

动作语义标签必须由编写者显式声明；编译器不得从 `position`、`distance` 或 step 名自动推断。

## 5. DSL 合同（冻结）

本规范冻结如下 DSL 方向。

### 5.1 语义资源声明

在 `[topology]` 中声明资源：

```plc
resource slide_pick_zone: semantic_resource {
    mode: exclusive
    purpose: "滑轨取片空间占用"
}
```

冻结规则：

- `semantic_resource` 是新的一等声明对象
- `mode` 在 SRI-v1 仅允许 `exclusive`
- `purpose` 可选，但推荐填写

### 5.2 状态占用声明

在 `[constraints]` 中声明状态占用：

```plc
claim: cyl_feed.extended occupies slide_pick_zone
claim: cyl_swing.extended occupies swing_transfer_zone
```

冻结规则：

- 左侧在 SRI-v1 仅允许 `state_reference`
- 不允许阈值表达式直接作为 `claim` 左侧
- 若需要模拟量窗口，应先离散化为稳定信号，再用状态占用声明

### 5.3 动作语义标签

在长时动作上声明 `semantic_tag`：

```plc
step manual_arm_pick_move:
    action: axis.move_absolute(axis_arm, position: 0, params: stepper_default_fast, speed: 5)
        semantic_tag: arm_pick_to_slide
        timeout: 1500ms -> orient_axis_fault.move_timeout
        on_reject -> orient_axis_fault.move_reject
        on_motion_fault -> orient_axis_fault.move_motion_fault
        on_safety_fault -> orient_axis_fault.move_safety_fault
```

冻结规则：

- `semantic_tag` 是动作元数据，不参与动作参数白名单
- SRI-v1 仅要求 `axis.move_relative` 与 `axis.move_absolute` 支持 `semantic_tag`
- 其他动作是否支持 `semantic_tag`，不在 SRI-v1 范围内

### 5.4 动作标签占用声明

在 `[constraints]` 中声明动作标签占用：

```plc
claim: action_tag arm_pick_to_slide occupies slide_pick_zone
```

冻结规则：

- `action_tag <name>` 引用的是动作元数据标签，不是 step 名
- 引用的标签必须至少被一个动作声明
- 同一标签可被多个语义等价动作复用，但必须具有同一工业语义

## 6. 占用生命周期（冻结）

### 6.1 状态占用

`claim: <state_expr> occupies <resource>` 的生命周期为：

- 当 `<state_expr>` 成立时，资源被占用
- 当 `<state_expr>` 不成立时，资源释放

示例：

- `cyl_feed.extended occupies slide_pick_zone`
  - 只要 `cyl_feed.extended` 为真，`slide_pick_zone` 就处于占用态

### 6.2 动作标签占用

`claim: action_tag <tag> occupies <resource>` 的生命周期在 SRI-v1 固定为：

- 从动作被 runtime 接受并进入挂起生命周期开始
- 持续到该动作进入 `Done` / `Fault` / `Timeout` / `Reject` 的终态为止

固定解释：

- 对 `axis.move_*`，占用从命令被接受那一刻开始
- 即使当前 tick 只是刚发起动作，占用也必须在该 tick 生效
- 后续 tick 的轮询阶段继续保持占用，直到动作终态

这意味着：

- `arm_pick_to_slide` 只在“去滑轨的 motion 正在执行”期间占用资源
- `arm_move_to_middle`、`arm_move_to_orient` 不会自动占用该资源，除非它们显式使用同一 `semantic_tag`

## 7. 资源冲突语义（冻结）

SRI-v1 中，`semantic_resource { mode: exclusive }` 的规则固定为：

- 任一可达全局状态中，同一资源最多只能被一个活跃 claim 占用
- 若两个或更多 claim 同时成立，则构成 safety violation

冲突来源可以是：

1. 状态占用 vs 状态占用
2. 状态占用 vs 动作标签占用
3. 动作标签占用 vs 动作标签占用

## 8. 与 safety 的关系（冻结）

SRI 不是对 `safety` 的语法糖扩写，而是新的 first-class 语义层。

固定原则：

1. `safety` 继续用于：
   - 状态互斥
   - requires 前置条件
   - 模拟量阈值互锁
2. SRI 用于：
   - 语义资源占用
   - 特定动作语义与特定姿态/空间占用之间的互斥
3. verification 层必须把 SRI 视为 safety 主路径的一部分，而不是后置插件

换言之：

- `safety` 不直接理解“某个动作去滑轨”
- SRI 先把“去滑轨动作”提升为可占用资源的稳定语义对象
- safety 检查器在全局状态空间中检查资源独占性

## 9. IR 合同（冻结）

SRI-v1 在 IR 中新增三类核心对象。

### 9.1 语义资源

```text
SemanticResource {
    name: String,
    mode: Exclusive,
    purpose: Option<String>,
}
```

### 9.2 资源占用规则

```text
ResourceClaimRule {
    source: ClaimSource,
    resource: String,
    reason: Option<String>,
    source_location: Option<...>,
}

ClaimSource =
    State(StateExpr)
  | ActionTag { tag: String }
```

### 9.3 动作语义标签

`TransitionAction::AxisMoveRelative` 与 `TransitionAction::AxisMoveAbsolute` 在 IR 中新增：

```text
semantic_tag: Option<String>
```

`PendingActionContext` 在 IR / runtime bridge / runtime-core 中同步新增：

```text
semantic_tag: Option<String>
```

固定要求：

- 动作标签必须随 pending action 一起进入 runtime 与 verification
- verification 不得回退到仅凭 `pulse.active` 猜测动作语义

## 10. verification 口径（冻结）

### 10.1 safety

Safety verifier 必须把资源占用纳入全局状态检查。

对每个可达全局状态：

1. 评估所有状态占用 claim 是否成立
2. 评估所有动作标签占用 claim 是否成立
   - 若某个 active task 的 pending action 携带该标签，则该 claim 成立
3. 汇总每个资源的活跃 holder 集合
4. 若独占资源的 holder 数量大于 1，则报错

### 10.2 liveness

SRI 不改变 liveness 的基本语义，但增加一条约束：

- 若 runtime 会因资源冲突拒绝或安全故障分流，则 liveness 夹具必须显式覆盖这些分支

### 10.3 timing

SRI 不直接引入新的时间上界，但资源冲突导致的安全分流路径必须计入 timing 分析可达路径。

### 10.4 causality

SRI 不要求 causality 理解几何学，但必须把 `semantic_tag` 视为动作语义元数据的一部分，避免“同名标签未绑定任何动作”的静默放行。

## 11. runtime / bridge 口径（冻结）

### 11.1 runtime bridge

runtime bridge 必须把以下信息显式降级到 runtime 可执行结构：

- 资源定义
- claim 规则
- pending action 上的 `semantic_tag`

### 11.2 runtime-core

runtime-core 必须在动作发起与 pending 轮询期间维护资源占用。

固定要求：

1. 当带 `semantic_tag` 的长时动作被接受时，立即开始占用对应资源
2. 当动作进入终态时，释放对应资源
3. 状态占用型 claim 根据当前设备状态实时评估
4. 若某次动作发起会导致独占资源冲突，runtime 必须按安全故障路径处理，而不是静默覆盖

SRI-v1 对 runtime 结果分类的固定解释：

- 资源独占冲突属于 `safety_fault`
- 不归类为 `reject`

原因：

- 这是安全互锁违反，不是参数格式错误，也不是调度队列繁忙

## 12. codegen 口径（冻结）

codegen 不得丢失 SRI 语义。

SRI-v1 固定规则：

1. 若目标后端支持等价的资源互锁执行模型，则必须显式降级
2. 若目标后端不支持，则必须在 codegen 阶段明确拒绝，不允许静默忽略

推荐诊断：

- `[SRI-020] target backend does not support semantic resource interlock`

## 13. 诊断规则（冻结）

建议固定以下诊断族：

### SRI-001

- 触发条件：重复资源名
- 模板：`[SRI-001] semantic resource '<name>' is declared more than once.`

### SRI-002

- 触发条件：claim 引用不存在的资源
- 模板：`[SRI-002] claim references unknown semantic resource '<name>'.`

### SRI-003

- 触发条件：`claim: action_tag <tag> ...` 引用未声明于任何动作的标签
- 模板：`[SRI-003] action_tag '<tag>' is not used by any supported action.`

### SRI-004

- 触发条件：`semantic_tag` 用在 SRI-v1 不支持的动作类型上
- 模板：`[SRI-004] semantic_tag is not supported on action '<action_kind>' in SRI-v1.`

### SRI-005

- 触发条件：试图把模拟量阈值直接作为 claim 左侧
- 模板：`[SRI-005] claim source must be a state reference or action_tag in SRI-v1.`

### SRI-006

- 触发条件：试图依赖 `position` / `distance` 自动推断标签
- 模板：`[SRI-006] action semantic must be declared explicitly; positional auto-inference is not supported.`

## 14. wafer_loader 迁移示例（规范性示例）

目标语义：

- 出料气缸前进占用“滑轨取片空间”
- 旋臂执行“去滑轨取片”动作时也占用该空间
- 二者不能同时成立

规范性示例：

```plc
[topology]

resource slide_pick_zone: semantic_resource {
    mode: exclusive
    purpose: "滑轨取片空间占用"
}

[constraints]

claim: cyl_feed.extended occupies slide_pick_zone
claim: action_tag arm_pick_to_slide occupies slide_pick_zone

[tasks]

task mode_service:
    step manual_arm_pick_move:
        action: axis.move_absolute(axis_arm, position: 0, params: stepper_default_fast, speed: 5)
            semantic_tag: arm_pick_to_slide
            timeout: 1500ms -> orient_axis_fault.move_timeout
            on_reject -> orient_axis_fault.move_reject
            on_motion_fault -> orient_axis_fault.move_motion_fault
            on_safety_fault -> orient_axis_fault.move_safety_fault
```

这一定义表达的是：

- “去滑轨取片”这一动作语义与“出料气缸已前进”互斥
- 不会误伤 `position: 45` 或 `position: 90` 的其他旋臂动作
- 不依赖 `axis_arm.pulse.active` 这一整轴粗粒度窗口

## 15. 设计裁决

当以下三种方案发生分歧时，本规范的裁决固定如下：

1. **直接在 `safety` 中引用动作字面量**
   - 拒绝
2. **继续用 `pulse.active` / 输入按钮 `.on` 近似表达特定动作语义**
   - 仅允许作为迁移期临时方案
3. **把资源占用与动作标签提升为一等语义**
   - 采用

理由：

- 它满足“IR 是唯一语义汇合点”
- 它能进入 verification 与 runtime
- 它避免把上层语义问题下沉为局部补丁

