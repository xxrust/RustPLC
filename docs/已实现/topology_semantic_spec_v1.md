# RustPLC 拓扑语义与关系验证规范 V1（合并版）

> 本文合并以下三份材料：
> 1) 《RustPLC 设备抽象层次设计分析报告》
> 2) `docs/topology_relation_strict_whitelist.md`
> 3) `docs/composite_device_port_semantics.md`
>
> 目标：恢复并强化 RustPLC 的核心原则——**形式化验证首先验证关系语义正确性**。

---

## 1. 背景与问题

RustPLC 当前在设备建模上以原子设备为主，但工业现场大量设备属于复合形态（例如内置传感器电缸）。

当前痛点：

1. 设备关系语义边界不够严格，导致不合理链路可能被放行。
2. 示例与测试中存在会“固化错误语义”的写法。
3. 形式化验证与拓扑语义门禁未完全解耦，可能出现“性质通过但语义不对”。

**核心要求**：
- 拓扑语义错误必须在语义阶段硬失败；
- 未通过语义门禁，不得进入 Safety/Liveness/Timing/Causality 验证。

---

## 2. 架构原则（总纲）

### 2.1 建模原则

1. **物理一体，语义分解**：
   - 物理上一个设备可对应一个节点；
   - 语义上按端口能力（role/type/semantic_role）做关系判定。

2. **关系有方向且必须可解释**：
   - 左侧端子 = 输入（consumer）
   - 右侧端子 = 输出（producer）
   - 无输入端不得显示左端子；无输出端不得显示右端子。

3. **端口优先于设备类型**：
   - 设备类型用于粗分类；
   - 最终合法性在端口级裁定。

### 2.2 工业对标原则（吸收结论）

- 借鉴 OPC UA 的分层/组合思想（严谨语义模型）。
- 保持 DSL 的可读与轻量，不做过度复杂化。
- 保留工程落地路径：先修语义白名单，再推进复合设备。

---

## 3. 严格关系白名单（V1 强约束）

## 3.1 `driven_by`
语义：执行/控制链路中的“被驱动”。

仅允许：
- `digital_output -> solenoid_valve`
- `digital_output -> motor`
- `solenoid_valve -> cylinder`
- `analog_output -> motor`（启用模拟控制场景）

明确禁止：
- `digital_input -> digital_input`
- `digital_input -> sensor`
- `digital_output -> digital_output`
- `sensor -> *`（通过 driven_by）
- `* -> sensor`（通过 driven_by）

## 3.2 `reports_to`
语义：观测结果上报到采集点（IO 映射）。

仅允许：
- `sensor -> digital_input`
- `sensor -> analog_input`

禁止：
- 非 `sensor` 作为 `reports_to` 源
- `reports_to` 目标不是输入采集点

## 3.3 `detects`
语义：被观测对象状态到观测器的检测关系。

仅允许：
- `cylinder -> sensor`（`extended/retracted` 等状态）
- `motor -> sensor`（项目定义的可检测状态）

禁止：
- `sensor` 同时声明 `driven_by` 与 `detects`（语义混叠）
- `detects` 目标不是 `sensor`
- `detects` 源不具备可观测状态语义

---

## 4. 复合设备语义规范（V1）

## 4.1 复合设备最小模型

复合设备可声明多个端口，端口字段：

- `id`
- `type`: `digital | analog | pneumatic | logical | generic`
- `role`: `producer | consumer | bidirectional`
- `semantic_role`（建议）:
  - `actuator_cmd`
  - `status_feedback`
  - `fault_feedback`
  - `telemetry`

## 4.2 电缸示例（单节点多能力）

同一节点可包含：

- `cmd_extend`：digital, consumer, actuator_cmd
- `cmd_retract`：digital, consumer, actuator_cmd
- `inpos_ext`：digital, producer, status_feedback
- `inpos_ret`：digital, producer, status_feedback
- `fault`：digital, producer, fault_feedback
- `position`：analog, producer, telemetry

约束解释：
- 控制链路必须进入 `actuator_cmd` 端口；
- 状态链路必须由反馈端口输出；
- 不能把反馈端口当控制输入。

---

## 5. 语义门禁（Topology Semantic Gate）

在进入形式化验证前，必须依次通过：

1. 关系类型白名单校验（按第 3 节）。
2. 端口存在性校验（关系引用端口必须存在）。
3. 方向校验（producer -> consumer）。
4. 类型兼容校验（digital/analog/...）。
5. 语义角色校验（例如 actuator_cmd 不可接 telemetry）。
6. 复合设备一致性校验（同设备多端口合法但不得自相矛盾）。

任一失败：
- 统一返回 `semantic_topology_invalid`
- **禁止进入** Safety/Liveness/Timing/Causality

---

## 6. 非法输入布线问题归类（规范判定）

以下写法在本规范下必须报错：

- `device start_button: digital_input { driven_by: X4 }`
- `device sensor_B_ret: sensor { driven_by: X3, detects: cyl_B.retracted }`

判定原因：

1. `digital_input -> digital_input`（被禁止）。
2. `digital_input -> sensor`（被禁止）。
3. `sensor` 语义混叠（同时 `driven_by` + `detects`）。

---

## 7. 回归测试与验收标准

最小回归集合：

1. 正例：
   - `Y -> valve -> cylinder`
   - `cylinder detects -> sensor`
   - `sensor reports_to -> X`

2. 反例：
   - `digital_input -> digital_input` driven_by 必须失败。
   - `digital_input -> sensor` driven_by 必须失败。
   - `sensor` 同时 `driven_by + detects` 必须失败。
   - 关系非法时不得出现“形式化通过”结论。

3. 报告口径：
   - `semantic_topology_invalid`
   - `formal_verification_failed`

---

## 8. 分阶段落地路线

### 阶段 A（立即）
- 落实严格关系白名单。
- 增加语义硬失败门禁。
- 修复对应的非法布线回归夹具。

### 阶段 B（短中期）
- 引入复合设备端口语义（不必一次到位引入复杂继承）。
- 将关系校验从设备类型矩阵迁移到端口约束矩阵。

### 阶段 C（中长期）
- 评估模板化/继承（如 `composite_device`、`template`）。
- 在不损失可验证性的前提下提升表达力。

---

## 9. 执行纪律

1. 关系语义是形式化验证入口门禁，不得后移。  
2. 示例、测试、文档必须与白名单同步更新。  
3. 任何放宽规则必须附带风险说明与反例测试。  
4. UI 展示不得掩盖语义错误（必须可见且可诊断）。

---

状态：V1 Draft（可作为实现基线）
