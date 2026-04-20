# Semantic Twin Geometry View

## 1. Purpose

RustPLC 现在已经有三类稳定事实源：

- authored intent：`*.system.md`、`*.architecture.md`、`*.intent_alignment.contract.json`
- compiled semantics：`TopologyGraph`、`StateMachine`、`ConstraintSet`
- executable evidence：`sim-plc` / `no-board-gate` / board trace / intent-alignment report

缺的不是更大的数字孪生，而是一个更小、更可靠的表达层：

- 不依赖真实设备，也不要求用户读代码
- 能快速回答“程序会不会按逻辑推进”
- 能明确区分 authored / verified / observed / blocked
- 能被静态图、动态图、Web UI、SVG、诊断页共同消费

本设计把这个表达层定义为 `Semantic Twin Geometry View`。

它不是物理仿真，也不是 3D 建模。
它是对 RustPLC 语义与证据的几何投影。

## 2. Problem

当前仓库里已经能分别看到：

- 编译后的 IR
- 四类 verification 结果
- trace / diff / timing / diagnosis
- intent alignment verdict

但这些工件分散，阅读成本高，且没有统一几何语言去表达：

- 主流程是什么
- 哪些 step 会阻塞
- 哪些资源会冲突
- 哪些 fault / recovery 路径真实存在
- 哪些结论来自文档，哪些来自形式验证，哪些来自真实 trace

这会导致用户在“不看代码”和“没有真实设备”的情况下，很难建立足够强的逻辑信心。

## 3. Non-Goals

本能力明确不做：

- 高保真物理数字孪生
- CAD 级机构几何建模
- 电气柜、线缆、流体、受力、碰撞的物理求解
- 直接取代 HMI
- 在 MVP 阶段承诺完整 Web 动画编辑器

## 4. Core Idea

Geometry View 不是另一份业务模型。
它只能由现有语义和证据自动导出。

唯一允许的主链：

```text
system / contract / scenario
          |
          v
+---------------------------+
| semantic + IR            |
| topology/state_machine   |
| constraints              |
+---------------------------+
          |
          +--------------------+
          |                    |
          v                    v
 verification            trace / intent report
          |                    |
          +----------+---------+
                     |
                     v
        +-----------------------------+
        | geometry artifact compiler   |
        +-----------------------------+
                     |
                     v
        JSON artifact for SVG / Web / replay
```

如果图不是从这条链自动生成，它就不是可信工件。

## 5. Three Views

Geometry View 固定拆成三个视图。

### 5.1 Constellation View

用于 30 秒理解系统结构。

它回答：

- 系统有哪些任务、设备、资源、工件位置
- 哪些节点属于同一个 task / topology / evidence 面
- 哪些约束对象是系统级核心实体

建议视觉语法：

- task：轨道环
- step：恒星
- device：星团节点
- resource：引力中心
- workpiece site：停泊点

### 5.2 Orbit View

用于理解程序如何推进。

它回答：

- state machine 中有哪些 transition
- 哪些 guard 是 `condition` / `delay` / `timeout`
- 哪些动作是 blocking / pending / fault-routed
- task 之间如何并发

建议视觉语法：

- transition：轨道弧线
- blocking step：带引力井的节点
- timeout / fault route：逃逸轨迹
- current active task：高亮轨道

### 5.3 Evidence View

用于回答“你为什么相信它”。

它回答：

- 哪些语义来自 authored contract
- 哪些约束被 verification 消费
- 哪些转移被真实 trace 观测到
- intent-alignment 当前是 `aligned` / `mismatch` / `blocked`

建议视觉语法：

- authored：冷色
- verified：稳定光晕
- observed：运动尾迹
- warning：橙色扰动
- blocked：红色断裂

## 6. Artifact Model

MVP 的 geometry artifact 采用稳定 JSON，而不是直接耦合某个前端实现。

工件包含：

- `lanes`
  - `topology`
  - `evidence`
  - 每个 `task:<name>`
- `nodes`
  - `device`
  - `task`
  - `step`
  - `semantic_resource`
  - `timing_rule`
  - `causality_chain`
  - `workpiece_site / holder / carrier`
  - `claim_source / external_reference`
- `edges`
  - `contains`
  - `topology_link`
  - `transition`
  - `resource_claim`
  - `timing_scope`
  - `causality`
- `overlays.trace`
  - observed transitions
- `overlays.intent`
  - verdict / mismatch / blocker / warnings

这意味着后续任何渲染器都不需要重新理解 DSL。
它只消费 geometry artifact。

## 7. Evidence Semantics

Geometry View 必须显式带证据状态，而不是只画结构。

MVP 统一使用：

- `authored`
- `derived`
- `verified`
- `observed`
- `warning`
- `blocked`

关键原则：

- `TopologyGraph` / `ConstraintSet` 上来的结构默认是 `authored` 或 `verified`
- `StateMachine` 导出的 task/step/transition 是 `derived`
- trace overlay 是 `observed`
- intent warnings 是 `warning`
- intent blocker 是 `blocked`

## 8. First Delivery Shape

第一阶段不做复杂 UI，先交付 CLI artifact：

```text
rust_plc geometry-export <source.plc|source.bundle.toml> \
  --out out/geometry/<name>.geometry.json \
  [--trace <trace.jsonl>] \
  [--intent-report <report.json>]
```

原因：

- 最小侵入
- 可直接进 CI / project-check / web-server
- 易于做快照测试
- 不把视觉实现绑定到当前 web stack

## 9. Recommended Follow-On Path

### Phase 1

- 交付 geometry JSON schema
- 从 compile semantics 自动导出静态几何
- 支持 trace / intent overlay

### Phase 2

- Web UI 读取 geometry artifact
- 提供 `Constellation / Orbit / Evidence` 三种过滤视图
- 支持 trace scrubber 与关键转移跳转

### Phase 3

- 引入动画尾迹
- 引入 `trace-diff` 与 board trace 叠加
- 引入 diagnostics deep-link

### Phase 4

- 讨论更强的几何语法，例如 orbit packing、resource gravity、fault halo
- 仍然不进入物理孪生

## 10. Repository Hooks

Geometry View 在仓内应绑定这些入口：

- compile seam
  - `src/cli_support/plc_pipeline.rs`
- CLI seam
  - `src/cli/utilities.rs`
- semantic model
  - `src/ir/mod.rs`
- evidence model
  - `src/trace_diff.rs`
  - `src/intent_alignment/report.rs`
- future web seam
  - `crates/web-server/src/main.rs`

这保证几何工件始终站在 IR 与 evidence 之上，而不是降级成前端手工模型。

## 11. Acceptance Bar

只有同时满足下面四点，Geometry View 才算有价值：

1. 用户可以不读 DSL 源码，理解主流程与阻塞点。
2. 用户可以分辨 authored / verified / observed / blocked。
3. 同一个 artifact 可被 CLI、Web、诊断和文档复用。
4. 图和 trace/report 同步更新，不依赖人工重画。

## 12. Takeaway

RustPLC 需要的不是“大而全数字孪生”。

它需要一个更小、更可信、更优雅的中间层：

- 以 IR 为中心
- 以 verification 和 trace 为证据
- 以 geometry artifact 为统一输出

这样系统既保持抽象，又能快速让人建立逻辑信心。
