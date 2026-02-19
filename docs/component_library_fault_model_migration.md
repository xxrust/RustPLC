# 元件库 + 元件异常模型（新格式说明与迁移指南）

本文档说明以下内容：

- 新格式是什么（拓扑 + 场景）
- 旧格式与新格式的字段级差异
- 迁移步骤
- 常见报错码与处理方式
- 可直接运行的完整示例（正常场景 + 异常场景）

---

## 1. 新格式总览

### 1.1 拓扑文件（`component-topology`）

核心字段：

- `schema_version`
- `component_library`（元件类型定义）
- `components`（元件实例）
- `connections`（实例之间连线）

可用命令：

```bash
cargo run -- component-topology-validate examples/component_model/topology.json --output json
```

### 1.2 场景文件（`component-scenario`）

核心字段：

- `schema_version`
- `tick_ms`
- `duration_ms`
- `switch_events`（开关事件）
- `sensor_events`（传感器事件）
- `component_faults`（元件异常注入）

可用命令：

```bash
cargo run -- component-scenario-validate examples/component_model/scenario_normal.json --output json
```

---

## 2. 旧格式 vs 新格式（字段级差异）

### 2.1 场景字段变化

- 旧：`faults.sensor_stuck`
- 新：`component_faults[]`，并通过 `fault_kind` 指定异常类型（如 `stuck_on/stuck_off/stall` 等）

- 旧：`forces`
- 新：不再支持该字段；执行器/信号异常统一用 `component_faults[]` 表达

- 旧：输入通常按 DI/AI 号组织
- 新：`switch_events/sensor_events` 直接按 **组件实例 ID** 组织（如 `s_start`、`x_front`）

### 2.2 拓扑表达变化

- 旧：偏设备/IO地址驱动
- 新：以元件实例 + 端口连接为核心（`from: "s_start.state" -> to: "cyl_a.cmd_extend"`）

---

## 3. 迁移步骤（建议顺序）

1. 先写元件库与拓扑：`component_library + components + connections`
2. 将旧 `faults.sensor_stuck` 逐条改写成 `component_faults[]`
3. 删除旧 `forces`，改为等价的元件异常（如 `jammed/stall/stuck_off`）
4. 将旧输入脚本改为 `switch_events` / `sensor_events`
5. 分别执行：
   - `component-topology-validate`
   - `component-scenario-validate`
   - `component-sim`

---

## 4. 同 tick 多异常冲突规则（确定性）

当前实现中，同一组件同一 tick 多异常同时生效时，按固定优先级处理（高到低）：

1. `jammed`
2. `motion_timeout`
3. `stall`
4. `direction_reversed`
5. `lost_step`
6. `stuck_off`
7. `stuck_on`
8. `chatter`

说明：

- 布尔类状态（传感器/开关）优先使用 `stuck_off/stuck_on/chatter` 决策
- 步进位置演化优先受 `stall/direction_reversed/lost_step` 影响
- 所有异常启停都会写入 machine-readable 审计文件（`fault_audit.jsonl`）

---

## 5. 常见报错码与处理方式

| 报错码 | 含义 | 处理方式 |
|---|---|---|
| `CSCN-MIG-001` | 使用了旧 `faults` 字段 | 改为 `component_faults[]` |
| `CSCN-MIG-002` | 使用了旧 `forces` 字段 | 删除 `forces`，改为元件异常表达 |
| `CSCN-TIME-003` | `duration_ms` 与 `tick_ms` 不对齐 | 保证 `duration_ms % tick_ms == 0` |
| `CSIM-TGT-005` | 异常目标组件 ID 不存在 | 检查 `target_component_id` 是否在拓扑实例中 |
| `CSIM-TGT-006` | 异常类型与组件类型不匹配 | 改成该组件支持的 `fault_kind` |

---

## 6. 完整示例（可直接运行）

### 6.1 正常场景（无异常）

文件：

- `examples/component_model/topology.json`
- `examples/component_model/scenario_normal.json`

运行：

```bash
cargo run -- component-sim examples/component_model/topology.json \
  --scenario examples/component_model/scenario_normal.json \
  --out out/component_normal_trace.jsonl \
  --fault-audit-out out/component_normal_fault_audit.jsonl \
  --diagnosis-out out/component_normal_diagnosis.json \
  --output json
```

### 6.2 异常场景（含 stall/jammed/stuck_off 等）

文件：

- `examples/component_model/topology.json`
- `examples/component_model/scenario_faults.json`

运行：

```bash
cargo run -- component-sim examples/component_model/topology.json \
  --scenario examples/component_model/scenario_faults.json \
  --out out/component_fault_trace.jsonl \
  --fault-audit-out out/component_fault_audit.jsonl \
  --diagnosis-out out/component_fault_diagnosis.json \
  --output json
```

---

## 7. 输出工件变化（旧 -> 新）

### 7.1 旧路径（典型）

- `sim-plc` trace（IO导向）
- `faults.sensor_stuck` / `forces` 混合表达

### 7.2 新路径（本期）

- `component-sim` trace（组件状态导向）
  - 每 tick 输出每个组件的 `state/inputs/outputs/active_faults`
- `fault_audit.jsonl`
  - 记录每个异常的 `activated/expired` 生命周期
- `component_diagnosis.json`
  - 保留核心 `issue_code` 思路，并额外给出组件异常上下文（组件ID、异常类型、时间窗）

