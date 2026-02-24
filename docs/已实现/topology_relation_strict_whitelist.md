# 拓扑关系严格白名单（语义门禁草案）

> 目的：恢复并强化“形式化验证首先验证关系语义正确性”的核心原则。  
> 结论：**关系语义不通过时，必须在语义阶段硬失败，禁止进入 Safety/Liveness/Timing/Causality 验证。**

## 1. 基本原则

1. 关系是有方向的：左侧为源（producer），右侧为目标（consumer）。
2. 端子语义固定：
   - 左侧端子 = 输入（consumer）
   - 右侧端子 = 输出（producer）
   - 无输入端的设备不得显示左侧端子；无输出端的设备不得显示右侧端子。
3. `digital_input`（如 X0/X1/X2）是输入采集点，不允许承担“驱动别人”的职责。
4. `sensor` 是状态/事件观测器，不能再被 `driven_by` 当成执行链被驱动。

## 2. 关系定义与严格白名单

## 2.1 `driven_by`
语义：执行/控制链路中的“被驱动”。

仅允许：
- `digital_output -> solenoid_valve`
- `digital_output -> motor`
- `solenoid_valve -> cylinder`
- `analog_output -> motor`（如项目启用模拟控制）

禁止（明确列出高风险误用）：
- `digital_input -> digital_input`
- `digital_input -> sensor`
- `digital_output -> digital_output`
- `sensor -> *`
- `* -> sensor`（通过 `driven_by`）

## 2.2 `reports_to`
语义：观测结果上报码点（IO映射/采集归属）。

仅允许：
- `sensor -> digital_input`
- `sensor -> analog_input`

禁止：
- 非 `sensor` 作为 `reports_to` 源
- `reports_to` 目标不是输入点（`digital_input`/`analog_input`）

## 2.3 `detects`
语义：观测器对被观测对象状态的检测。

仅允许：
- `cylinder -> sensor`（如 `extended` / `retracted`）
- `motor -> sensor`（如项目定义了可检测状态）

禁止：
- `sensor` 同时声明 `driven_by` 与 `detects`（语义混叠）
- `detects` 目标不是 `sensor`
- `detects` 源不是可观测状态设备

## 3. 对 `two_cylinder` 的直接约束结论

以下写法应判为语义错误：

- `device start_button: digital_input { driven_by: X4 }`
- `device sensor_B_ret: sensor { driven_by: X3, detects: cyl_B.retracted }`

原因：
- 第一条属于 `digital_input -> digital_input`（禁止）。
- 第二条属于 `digital_input -> sensor` + `sensor` 语义混叠（禁止）。

## 4. 验证门禁要求（必须执行）

1. 拓扑关系白名单校验失败 => `parse/semantic` 阶段返回错误，直接中断。
2. 只有白名单通过后，才允许进入形式化验证（Safety/Liveness/Timing/Causality）。
3. 验证报告中需明确区分：
   - `semantic_topology_invalid`（关系不合法）
   - `formal_verification_failed`（关系合法但性质验证失败）

## 5. 回归测试要求

最少应包含：

1. **正例**：典型双气缸链路（Y->valve->cylinder，cylinder detects->sensor，sensor reports_to->X）通过。
2. **反例**：`digital_input -> digital_input`（`driven_by`）必须失败。
3. **反例**：`digital_input -> sensor`（`driven_by`）必须失败。
4. **反例**：`sensor` 同时 `driven_by` + `detects` 必须失败。
5. **反例**：关系非法时不得产出“形式化通过”结论。

## 6. 落地顺序建议

1. 先在语义层收紧关系矩阵（白名单）并补反例测试。  
2. 再修复 `examples/two_cylinder.plc` 与相关测试夹具。  
3. 最后校正前端端口显示与连线绑定规则，确保 UI 不再掩盖语义错误。

---

状态：Draft v1（待确认后转为项目强约束）
