# PRD：模拟量验证一致性与外部输入标记

## 1. 引言 / 概述

当前验证引擎对模拟量阈值安全规则存在静默跳过，并将模拟输入在因果推断中一视同仁，导致“完备证明”误导、模拟量约束未覆盖、以及外部输入（如操作员设定值/ADC 通道）产生因果链误报。本功能引入模拟量阈值一致性验证、外部输入标记与更严格的阈值语义校验，使安全/等待条件的处理一致且可解释。

## 2. 目标

- 消除模拟量阈值规则被静默跳过的问题，避免误导性的“完备证明”。  
- 支持在 Safety 约束与 `wait` 条件中使用模拟量阈值比较，并基于离散抽象参与验证。  
- 通过 `external` 标记减少外部输入的因果误报。  
- 增加测试与文档，固定行为并明确限制。  

## 3. 用户故事

### US-001：Safety 对跳过的模拟量阈值规则输出告警并降级证明等级
**描述**：作为用户，我希望 Safety 在跳过模拟量阈值规则时输出明确告警，并将证明等级从“完备证明”降级，避免误导。  

**验收标准：**
- [ ] SafetyReport / SafetySummary 新增 warnings 字段（Vec<String>），用于收集告警信息。  
- [ ] 当阈值规则无法建模时，向 warnings 追加告警，包含被跳过的规则文本。  
- [ ] 存在被跳过阈值规则时，SafetySummary.level 改为“有界验证（模拟量规则未覆盖）”。  
- [ ] 无阈值规则时仍输出“完备证明”。  
- [ ] 新增单元测试覆盖告警与等级降级。  
- [ ] `cargo test` 通过。  

### US-002：阈值比较的语义校验（安全 + wait）
**描述**：作为开发者，我希望对 safety 与 wait 中的阈值比较进行设备类型与 range 校验，拒绝错误类型或超范围阈值。  

**验收标准：**
- [ ] `safety` 阈值比较仅允许 `analog_input` 设备，否则报 type_mismatch。  
- [ ] `wait` 阈值比较仅允许 `analog_input` 设备，否则报 type_mismatch。  
- [ ] `analog_input` 未声明 `range` 时，阈值比较报 semantic 错误。  
- [ ] 阈值超出 `range` 时，报 semantic 错误。  
- [ ] 新增单元测试覆盖 safety 与 wait 两类场景。  
- [ ] `cargo test` 通过。  

### US-003：PEG 语法与 AST 支持 external 属性
**描述**：作为控制工程师，我希望在 `digital_input` 和 `analog_input` 上声明 `external: true`，标记为外部输入。  

**验收标准：**
- [ ] `plc.pest` 的 attribute_name 新增 `external`。  
- [ ] `DeviceAttributes` 新增 `external: Option<bool>` 字段。  
- [ ] `parser/mod.rs` 解析 `external` 属性并写入 AST。  
- [ ] 新增解析器单元测试：`device X0: digital_input { external: true }` 可解析。  
- [ ] 不含该属性时 `external == None`，行为不变。  
- [ ] `cargo test` 通过。  

### US-004：因果推断对 external 输入跳过动作→传感器推断
**描述**：作为控制工程师，我希望标记为 `external: true` 的输入设备不参与因果推断的传感器集合，减少误报。  

**验收标准：**
- [ ] `collect_sensor_names` 读取 `external` 属性。  
- [ ] `analog_input` 默认参与传感器集合；若 `external: true` 则排除。  
- [ ] `sensor` 设备继续参与因果推断（不受 external 影响）。  
- [ ] 新增测试：`analog_input { external: true }` 的 wait 不触发因果链误报。  
- [ ] 新增测试：未标 external 的 `analog_input` 仍触发推断。  
- [ ] `cargo test` 通过。  

### US-005：Safety BMC 阈值离散抽象
**描述**：作为用户，我希望 Safety 引擎能通过阈值离散抽象实际验证模拟量安全规则，而非跳过。  

**验收标准：**
- [ ] 从 `safety` 与 `wait` 中的阈值条件收集阈值，结合 `range` 切分区间。  
- [ ] `analog_input` 以区间状态进入 Safety BMC。  
- [ ] `set_analog` 映射到对应区间状态。  
- [ ] 阈值规则绑定到满足条件的区间集合。  
- [ ] 新增测试覆盖区间映射与阈值冲突场景。  
- [ ] `cargo test` 通过。  

### US-006：wait 中模拟量阈值条件映射为离散谓词
**描述**：作为用户，我希望 wait 中的模拟量比较在状态机中可表示为离散谓词，使活性/时序检查一致。  

**验收标准：**
- [ ] `wait` 模拟量比较生成基于区间的离散谓词字符串。  
- [ ] Liveness 将模拟量 wait 视为普通条件边。  
- [ ] 含模拟量 wait 且无 timeout 的步骤仍触发 liveness 告警。  
- [ ] 新增测试覆盖模拟量 wait 的 liveness 行为。  
- [ ] `cargo test` 通过。  

### US-007：文档与示例
**描述**：作为用户，我希望有文档与示例说明 external 标记与模拟量验证限制。  

**验收标准：**
- [ ] `analog_pressure_demo.plc` 的 AI0 添加 `external: true`。  
- [ ] 示例展示安全阈值被实际验证的场景。  
- [ ] 文档说明：外部输入与反馈传感器的区别、如何使用 `analog_input` 阈值比较与限制。  
- [ ] 集成测试覆盖更新后的示例。  
- [ ] `cargo test` 通过。  

## 4. 功能需求（Functional Requirements）

- FR-1：`digital_input` / `analog_input` 支持 `external: true`（默认 `false`）。  
- FR-2：因果推断遇到 external 输入时跳过动作→传感器推断。  
- FR-3：`safety` / `wait` 的阈值比较需校验设备类型与 `range`。  
- FR-4：Safety 通过阈值离散分区纳入模拟量规则。  
- FR-5：Safety summary 对未完全建模的规则降级证明等级并输出告警。  
- FR-6：`wait` 中模拟量比较映射为离散谓词，确保活性/时序一致。  
- FR-7：更新文档与示例覆盖 external 与模拟量验证限制。  

## 5. 非目标（Out of Scope）

- 连续时间/实数精确验证。  
- PID 控制建模与验证。  
- 硬件抽象层（EtherCAT/Modbus/GPIO）。  
- SMT 实数算术推理（除非后续明确要求）。  

## 6. 设计考虑

- 数值比较仅发生在 `analog_input` 上，传感器（sensor）用于离散反馈信号。  
- 避免 `==`，用区间或容差带表达阈值。  
- 外部设定值/上位机输入标记 `external: true`，不要求动作链。  

## 7. 技术考虑

- 阈值分区应同时覆盖 `safety` 与 `wait` 中出现的阈值。  
- `==` 比较的离散语义需统一（落入阈值桶或限制/警告）。  
- 证明等级语义：仅当所有规则可建模时才允许“完备证明”。  
- 向后兼容：旧程序默认行为不变，新增校验与告警可解释。  

## 8. 成功指标

- 模拟量阈值规则被跳过时，验证结果不再出现“完备证明”。  
- 至少覆盖：external 解析、因果跳过、阈值校验、区间抽象、模拟量 wait 的 liveness 行为。  
- 示例程序中外部输入引起的因果误报显著减少。  

## 9. 未决问题

- 模拟量 `==` 的离散语义如何定义？  
- 区间边界与舍入规则如何统一？  
- 外部输入是否允许被显式 `causality` 规则引用并参与推断？  
