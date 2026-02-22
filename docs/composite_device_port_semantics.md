# 复合设备端口语义规范（草案）

> 目标：在“关系语义必须严格正确”的前提下，支持一个物理设备同时包含多个执行功能与多个感知功能（如内置传感器电缸）。

## 1. 问题定义

在工业现场，一个物理节点可能同时具备：

- 执行能力（actuation）
- 状态感知能力（sensing）

例如电缸：
- 需要接收控制命令（伸出/回缩）
- 同时输出多个状态（到位、原点、故障、位置区间）

若仍用“设备单一角色”建模，会出现：
1. 语义误判：合法链路被判错。
2. 规则放宽：错误链路被放行。
3. 形式化验证失真：验证通过并不代表关系正确。

## 2. 核心建模原则

1. **物理一体，语义分解**：
   - UI/资产层仍可视为一个设备节点。
   - 语义/验证层按端口能力判定关系合法性。

2. **关系必须绑定端口（推荐强约束）**：
   - 关系合法性在端口级检查，而非仅设备类型级检查。

3. **端口角色固定**：
   - `consumer`：接收输入（左侧端子）
   - `producer`：输出信号（右侧端子）
   - `bidirectional`：双向（仅明确场景允许）

4. **UI 显示约束**：
   - 无 `consumer` 端口 -> 不显示左侧端子
   - 无 `producer` 端口 -> 不显示右侧端子

## 3. 复合设备最小语义模型

设备可声明多个端口，每个端口至少包含：

- `id`：端口标识
- `type`：`digital | analog | pneumatic | logical | generic`
- `role`：`producer | consumer | bidirectional`
- （建议）`semantic_role`：`actuator_cmd | status_feedback | fault_feedback | telemetry`

说明：
- `type` 解决信号兼容性（数字量/模拟量等）
- `role` 解决方向性
- `semantic_role` 解决“同是 producer，但含义不同”问题

## 4. 电缸示例（内置传感器）

示例端口（一个节点内）：

- `cmd_extend`：digital, consumer, actuator_cmd
- `cmd_retract`：digital, consumer, actuator_cmd
- `inpos_ext`：digital, producer, status_feedback
- `inpos_ret`：digital, producer, status_feedback
- `fault`：digital, producer, fault_feedback
- `position`：analog, producer, telemetry

语义效果：
- 控制链通过 `cmd_*` 端口进入
- 状态链通过 `inpos_* / fault / position` 端口输出
- 同一设备内“执行 + 感知”并存，不再被错误视为语义冲突

## 5. 三类关系在复合设备下的约束

## 5.1 `driven_by`
- 必须落到目标设备的 `consumer` 端口。
- 源端口必须是 `producer`。
- 不允许把 `status_feedback` 端口当作控制输入目标。

## 5.2 `reports_to`
- 只用于“观测结果上报到采集点”。
- 源端口建议限定为 `status_feedback | fault_feedback | telemetry`。
- 目标端口必须是采集型 `consumer`（X/AI 等）。

## 5.3 `detects`
- 表示“被观测对象状态 -> 观测设备”的检测关系。
- 源必须是可观测状态端口（通常 producer）。
- 目标必须是观测器输入端口（consumer），且语义角色匹配。

## 6. 形式化验证门禁（必须）

在进入 Safety/Liveness/Timing/Causality 之前，先执行 `Topology Semantic Gate`：

1. 端口存在性校验（关系引用端口必须存在）。
2. 方向校验（producer -> consumer）。
3. 类型兼容校验（digital/analog/...）。
4. 语义角色校验（actuator_cmd 不可接 telemetry 输入等）。
5. 复合设备特例校验（同设备多端口关系合法，但禁止自相矛盾绑定）。

若任一失败：
- 返回 `semantic_topology_invalid`
- **禁止**进入后续形式化验证

## 7. 对现有问题的直接收益

针对 `two_cylinder` 一类问题：
- 可明确禁止 `digital_input -> digital_input` 的错误驱动。
- 也可支持“一个设备既有控制端又有反馈端”的合法复杂场景。
- 避免“要么放得太松，要么卡得太死”的两难。

## 8. 落地建议

1. 先落文档白名单（关系 + 端口 + 语义角色）。
2. 再改语义引擎：从“设备类型矩阵”升级到“端口约束矩阵”。
3. 最后更新示例与测试：
   - 正例：复合设备合法链路
   - 反例：方向错、类型错、角色错

---

状态：Draft v1（待评审）
