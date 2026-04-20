# RustPLC 语义门禁实现方案（冻结草案 V1）

日期：2026-02-22  
前置输入：`docs/topology_semantic_gap_matrix_2026-02-22.md`  
目标：冻结 `SEM-101~SEM-105` 的执行顺序、错误结构、与形式化验证隔离边界。

## 1. 设计目标（本轮必须达成）

- 在进入 `Safety/Liveness/Timing/Causality` 前，先执行**拓扑语义门禁**。
- 门禁失败统一返回：`semantic_topology_invalid`。
- 错误项使用稳定码：`SEM-101..SEM-105`。
- 门禁检查以**端口**为主，不再以设备类型矩阵兜底放行。

## 2. 执行边界（硬隔离）

### 2.1 CLI/编译主流程

在 `compile_pipeline` 中改为：

1) parse + preprocess；  
2) `topology_semantic_gate(program.topology)`；  
3) 若失败：立即返回 `semantic_topology_invalid`（附 issues）；  
4) 若通过：再执行 `build_topology_graph / build_state_machine / build_constraint_set / build_timing_model / verify_all`。

**关键点**：不再像当前那样并行收集所有语义阶段错误后统一返回；先过拓扑门禁。

### 2.2 Web API

- `POST /api/topology/parse-plc`：可返回拓扑预览，但必须附加 `semantic_gate` 结果；
- `POST /api/run/no-board-gate` 与 CLI 校验链：门禁失败直接阻断执行。

## 3. 数据结构冻结

```rust
pub struct TopologySemanticIssue {
    pub code: TopologySemanticCode, // SEM-101..105
    pub line: usize,
    pub relation: Option<String>,   // driven_by/reports_to/detects
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_port: Option<String>,
    pub to_port: Option<String>,
    pub message: String,
    pub suggestion: String,
}

pub enum TopologySemanticCode {
    Sem101PortNotFound,
    Sem102DirectionInvalid,
    Sem103TypeIncompatible,
    Sem104SemanticRoleIncompatible,
    Sem105DanglingPort,
}

pub struct TopologySemanticGateError {
    pub code: &'static str, // "semantic_topology_invalid"
    pub issues: Vec<TopologySemanticIssue>,
}
```

返回契约：

- 顶层错误码固定 `semantic_topology_invalid`；
- 明细问题码固定 `SEM-101..SEM-105`；
- 错误消息可本地化，但 `code` 必须稳定。

## 4. 门禁算法冻结（顺序不可变）

1. `SEM-101` 端口存在性：关系涉及的端口必须能在端口目录中解析。
2. `SEM-102` 方向：`from.role in {producer,bidirectional}` 且 `to.role in {consumer,bidirectional}`。
3. `SEM-103` 类型兼容：`from.type` 与 `to.type` 按兼容矩阵匹配。
4. `SEM-104` 语义角色兼容（可选字段，存在则强校验）。
5. `SEM-105` 悬空端口：声明端口未参与任何关系。

说明：
- 任一步失败都继续收集错误，最终一次性返回 issues（提升可修复性）。
- 但无论收集多少，顶层都统一为 `semantic_topology_invalid`。

## 5. 端口解析策略冻结（过渡期）

由于现有 DSL 仍以 `driven_by/reports_to/detects` + 可选 `ports` 为主，采用“双通道”策略：

- **显式端口优先**：若设备声明 `ports`，仅使用显式端口。
- **过渡映射次之**：若未声明 `ports`，使用后端固定的 `DeviceType -> 默认端口契约` 生成临时端口（在语义层，不是 UI 层）。

过渡映射用于兼容历史示例，但必须可追踪（issue/日志标记 `implicit_port_contract=true`）。

## 6. 关系与类型兼容矩阵冻结（V1）

关系层：

- `driven_by`: output -> input，且 type 同类（digital/analog/pneumatic）。
- `reports_to`: output -> input，且采集链路同类（digital/analog）。
- `detects`: output(state) -> input(detector)；至少要求方向正确，若 `semantic_role` 存在则强校验。

类型层：

- 允许：同类 type；`generic` 仅在显式声明为过渡时允许。
- 禁止：跨类硬连（digital->analog、pneumatic->digital 等）。

## 7. UI 协议冻结

- 输入端口只渲染左侧；输出端口只渲染右侧；某侧无端口则该侧不显示。
- 禁止“inferred handle 自动补全”创建连线。
- 禁止“fallback 合法化连线”掩盖语义错误。
- `parse-plc` 返回若含 `semantic_gate.valid=false`，UI 必须显式展示错误列表。

## 8. 与现有系统衔接

- 现有 `PlcError` 继续用于 parse/通用语义错误；
- 新增拓扑门禁错误结构，不强行塞入 `PlcError` 枚举分支（降低改动面）；
- CLI 最终输出保持人类可读，同时在 `--output json` 模式透出结构化 `semantic_gate`。

## 9. 分批落地（实现顺序）

A. 新增门禁模块与数据结构（不改现有验证器）；  
B. 在 `compile_pipeline` 接线硬隔离；  
C. 收紧旧设备类型矩阵（去掉错误放行）；  
D. 修复非法布线回归夹具；  
E. UI 去降级化 + 错误面板联动。

## 10. 验收闸门

- 非法布线回归夹具必须在门禁阶段失败，且至少包含：
  - `SEM-102`（方向错误，或通过角色判定得出）
  - `SEM-103`（类型不兼容，若命中）
- 门禁失败时，验证摘要中不得出现 `Safety/Liveness/Timing/Causality 通过`。
- UI 在该文件下不得提示“降级可连线成功”。
