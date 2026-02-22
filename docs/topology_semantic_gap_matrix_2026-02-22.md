# RustPLC 拓扑语义门禁差距矩阵（基线盘点）

日期：2026-02-22  
范围：`parser` / `semantic` / `verification` / `web-server` / `web-ui`  
依据规范：`docs/RustPLC 拓扑语义与关系验证规范.md`

## 1) 基线结论（先给结论）

- 现状**不满足**新规范的语义门禁要求，核心差距在于：
  - 当前拓扑验证仍以“设备类型矩阵”为主，不是“端口级规则”。
  - `driven_by` 仍允许明显错误语义（如 `digital_input -> digital_input`、`digital_input -> sensor`）。
  - 缺少 `SEM-101~SEM-105` 结构化错误码与统一门禁返回。
  - UI 仍存在“端口降级绑定”路径，会掩盖语义问题。
- 实证：`examples/two_cylinder.plc` 目前可直接整体验证通过（包含你指出的错误语义）。

## 2) 实证证据

### 2.1 错误语义仍可通过完整验证

执行：

```bash
cargo run --quiet -- examples/two_cylinder.plc --no-print-ir
```

结果：Safety/Liveness/Timing/Causality 全部通过（exit code 0）。

### 2.2 当前实现明确放行了禁止关系

- `src/semantic/mod.rs:2737` 允许 `digital_input -> digital_input`（`driven_by`）。
- `src/semantic/mod.rs:2735` 允许 `digital_input -> sensor`（`driven_by`）。

### 2.3 UI 降级模式仍是可达路径

- 节点端口缺省时使用 fallback contract：`web-ui/src/utils/portContract.ts:74`。
- 连线允许 inferred handle + degraded binding：`web-ui/src/components/canvas/TopologyCanvas.tsx:374`。
- 前端直接提示“降级模式”而非硬阻断：`web-ui/src/components/canvas/TopologyCanvas.tsx:574`。

## 3) 规范条款对照差距矩阵

| 规范条款 | 目标 | 当前实现 | 差距判定 | 关键位置 |
|---|---|---|---|---|
| 关系白名单（4.1） | 仅允许定义好的 `relation × port属性` 组合 | 通过 `DeviceKind × DeviceKind` 决定连通性 | **不符合**（粒度错误且放行过宽） | `src/semantic/mod.rs:2719` |
| `SEM-101` 端口存在性 | `from_port`/`to_port` 必须存在 | `TopologyConnection` 端口可为空；`detects` 还会兜底填 state 名称 | **不符合** | `src/parser/mod.rs:130`、`src/semantic/mod.rs:454` |
| `SEM-102` 方向性 | output -> input | 仅按设备类型推断方向，不按端口角色检查 | **部分符合**（非端口级） | `src/semantic/mod.rs:423` |
| `SEM-103` 类型兼容 | 端口 type 必须兼容 | 仅映射到 `ConnectionType`，未比较端口声明类型 | **不符合** | `src/semantic/mod.rs:423` |
| `SEM-104` 语义角色兼容 | 按 `semantic_role` 矩阵校验 | AST/Parser/UI 类型中无 `semantic_role` 字段 | **不符合** | `src/ast/mod.rs:49`、`src/parser/plc.pest:32`、`web-ui/src/types/index.ts:93` |
| `SEM-105` 悬空端口 | 已声明端口必须参与关系 | 当前无悬空端口遍历校验 | **不符合** | `src/semantic/mod.rs:369` |
| 语义先于验证（1.2/5） | 未过语义门禁不得进入 formal | `compile_pipeline` 确实会在 semantic error 时中止 verify | **基本符合**（但缺专门“拓扑门禁”错误域） | `src/main.rs:9515` |
| 统一错误码 | 返回 `SEM-101~105` | 当前错误体系为 `parse/semantic/type_mismatch/...` | **不符合** | `src/error/mod.rs:24` |
| 显式端口/关系 DSL（第3章示例） | 支持 `ports {}` 与 `relation { from: A.p, ... }` | 语法仅支持 `device ... { driven_by/reports_to/detects }` | **不符合** | `src/parser/plc.pest:91` |
| UI 不得掩盖语义错误 | 不允许降级连线替代语义错误 | 目前保留降级路径 + 虚线警告 | **不符合** | `web-ui/src/components/canvas/TopologyCanvas.tsx:374` |

## 4) two_cylinder 直接问题映射

- 文件：`examples/two_cylinder.plc:11` 与 `examples/two_cylinder.plc:53`
- 问题：
  - `start_button: digital_input { driven_by: X4 }` -> `digital_input -> digital_input`
  - `sensor_B_ret: sensor { driven_by: X3, detects: cyl_B.retracted }` -> `digital_input -> sensor` 且语义混叠
- 之所以通过：当前矩阵明确放行上述关系，且没有 `sensor` 语义冲突规则。

## 5) 迁移影响面（粗量化）

基于仓库文本扫描（示例/测试/源码夹具）：

- `sensor { driven_by + detects }` 命中：
  - examples: 15 文件
  - tests: 6 文件
  - src 内嵌样例: 4 文件
- `digital_input { driven_by: ... }` 命中：
  - examples: 17 文件
  - tests: 4 文件
  - src 内嵌样例: 3 文件

结论：语义收紧后会触发**较大规模样例与测试改造**，需要分批迁移而不是一次性硬切。

## 6) 对后续实施的约束（用于下一步设计冻结）

- 先冻结“门禁执行顺序 + 错误码 + 返回结构”，再改代码。
- 第一批先实现 `SEM-101~103`（硬门禁），`SEM-104` 采用能力探测（字段缺失可先跳过但保留框架）。
- `SEM-105` 先做“warning 可升级为 error”的开关，以便平滑迁移现有示例。
- UI 侧在门禁前不再新增任何“自动推断连线”能力，避免再次掩盖语义缺陷。

