# 模拟 I/O 形式化验证问题与处理建议

## 发现的问题（漏洞/缺口）

1. **Safety 对模拟量阈值约束静默跳过**
   - `SafetyExpr::Threshold` 在绑定时直接返回 `None`，导致包含阈值的安全规则不会进入 BMC 搜索，验证可能错误显示“通过”。
   - 位置：`src/verification/safety.rs:635` `src/verification/safety.rs:651`

2. **模拟量在安全模型中无数值语义**
   - `analog_input/analog_output` 只有单一状态 `analog_active`，`set_analog` 仅写入该常量状态，无法表达数值变化。
   - 位置：`src/verification/safety.rs:371` `src/verification/safety.rs:475`

3. **模拟输入被当作传感器参与因果推断**
   - `analog_input` 被加入 `sensor_names`，导致 `wait` 中出现模拟输入会触发动作→传感器链路推断；对外部/环境输入可能产生误报或逼迫编造因果链。
   - 位置：`src/verification/causality.rs:289`

4. **阈值比较缺少类型/范围校验**
   - 语义阶段对 `wait` / `safety` 的阈值比较仅校验设备是否存在，不校验设备类型或阈值是否超出 `range`。
   - 位置：`src/semantic/mod.rs:1038` `src/semantic/mod.rs:1136`

## 处理建议（Sensor/模拟量/ADC）

### 方案 A：阈值派生布尔（最小改动、落地快）
- 将模拟量传感器建模为 `analog_input`，再由阈值生成布尔 `sensor`。
- 建议扩展 DSL：
  - `sensor pressure_hi: sensor { source: AI0, threshold: 6.0, hysteresis: 0.2, debounce: 50ms }`
- `wait` 仍使用布尔语义：`wait: pressure_hi == true`。

### 方案 B：模拟量数值比较 + 离散化
- 允许 `wait` 直接对 `analog_input` 比较数值。
- 在验证时收集所有阈值并离散化为有限区间，将“区间编号”作为状态进入 BMC。

### 方案 C：SMT 实数语义（成本高、语义强）
- Safety 引入 Z3 实数/整数变量，`set_analog`、阈值约束、`range` 作为 SMT 约束。
- 优点语义准确；缺点是验证成本与工程复杂度显著上升。

## 最小安全修补建议

1. **显式告警**：Safety 输出 `warnings` 提示“阈值约束未验证”，避免误导。
   - 位置：`src/verification/safety.rs:651`

2. **外部输入标注**：为 `analog_input` / `digital_input` 增加 `external: true` 或 `role: external`，因果推断遇到外部输入时跳过推断以减少误报。
   - 位置：`src/verification/causality.rs:289`
