# 拓扑信号方向重构

RustPLC 的拓扑语义从模糊的 `connected_to` 演进为显式的 `driven_by` / `reports_to` / `detects` 三种关系，所有边统一为 **producer → consumer** 方向。

---

## 为什么重构

原始 DSL 用 `connected_to` 表达设备关系，但这个关键字把驱动关系、信号上报和检测混为一谈。当拓扑复杂度上升（MIMO、多传感器、跨区域），模糊性导致 IR 构建、因果验证和前端渲染出现不一致。

---

## 新语义

### 三种关系类型

| 关系 | 含义 | 方向 |
|------|------|------|
| `driven_by` | 执行器被上游设备驱动 | producer → consumer |
| `reports_to` | 传感器向 I/O 点上报信号 | producer → consumer |
| `detects` | 传感器检测目标设备状态 | producer → consumer |

### DSL 语法

```plc
relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: plc_main.X0, via: reports_to }
```

每条 relation 显式声明源端口、目标端口和关系类型，消除歧义。

---

## 端口作为一等公民

设备声明类型化端口：

```json
{
  "id": "extend_out",
  "type": "pneumatic",
  "role": "producer"
}
```

| 属性 | 值 |
|------|------|
| role | `producer` / `consumer` / `bidirectional` |
| type | `digital` / `analog` / `pneumatic` / `logical` / `generic` |

端口级连接支持 MIMO 拓扑（一对多、多对一、多对多）。

---

## 多维标签

设备支持结构化标签，用于分组、风险分级和位置定位：

```plc
device cyl_press: cylinder {
    purpose: "冲压气缸",
    tags: {
        functional_group: "press_unit",
        danger_level: "high",
        location_group: "line_a/cell_2/station_7"
    }
}
```

| 标签维度 | 用途 |
|---------|------|
| `functional_group` | 逻辑功能分组 |
| `danger_level` | 风险等级（`low` / `medium` / `high`） |
| `location_group` | 层级物理位置（`line/cell/station`） |

### 标签规则引擎

```json
{
  "tag_rules": {
    "danger_level": {
      "dual_channel_levels": ["high"]
    },
    "functional_group": {
      "mode": "within_only"
    },
    "location_group": {
      "mode": "allow_any",
      "allowed_cross_zone_pairs": [["line_a/cell_1", "line_a/cell_2"]]
    }
  }
}
```

- `danger_level: high` 的设备自动要求双通道冗余
- `functional_group: within_only` 禁止跨组连接
- `location_group` 可配置允许的跨区域连接对

规则违反产生结构化错误（`code/path/message`）。

---

## 迁移

### 自动迁移工具

```bash
python3 scripts/migrate_connected_to.py --input examples/ --output examples/
```

无法自动迁移的项会标记为需人工确认。

### CI 回归守卫

CI 拒绝任何新的 `connected_to` 用法：

```bash
bash scripts/ci_no_connected_to_regression.sh
```

---

## 语义 Diff

`component-topology-diff` 模块计算两个拓扑快照之间的差异：

- 节点级 diff（新增/删除/修改设备）
- 端口级 diff（端口变更）
- 关系级 diff（连接变更）
- 标签级 diff（标签变更）
- 影响分析（受影响的规则、测试、模块）

输出适合审计记录。

---

## 性能门禁

500 节点 / 2000 边基准测试守护规模回归：

```bash
python3 scripts/topology_perf_gate.py --output human
```

| 路径 | p95 阈值 |
|------|----------|
| `parse_validate` | 250 ms |
| `compile_simulate` | 400 ms |
| `render_transform` | 80 ms |

---

## API 输出

`parse-plc` 和拓扑 API 返回完整的关系和端口元数据：

```json
{
  "relations": [{
    "from": "valve_A", "to": "cyl_A",
    "relation": "driven_by",
    "from_port": "output", "to_port": "drive_in",
    "signal": "pneumatic"
  }],
  "nodes": [{
    "id": "cyl_A",
    "ports": [...],
    "tags": { "functional_group": "press_unit", "danger_level": "high" }
  }]
}
```

---

## 前端

- 连接绑定到 `sourceHandle` / `targetHandle`（匹配端口 ID）
- 端口契约覆盖：`cylinder` / `sensor` / `switch` / `stepper` / `generic`
- 缺失端口元数据显示降级样式 + 警告
- 标签面板支持过滤、分组高亮、`location_group` 一键导航

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/ast/mod.rs` | `TopologyRelation`, `DevicePort`, `DeviceTags` |
| `src/component_topology.rs` | 标签规则验证 |
| `src/semantic/mod.rs` | producer → consumer 图构建 |
| `src/verification/causality.rs` | BFS 遍历（更新后） |
| `scripts/migrate_connected_to.py` | 迁移工具 |
| `tests/component_topology_diff.rs` | 语义 diff 测试 |
| `tests/component_topology_validate.rs` | 标签规则契约测试 |
