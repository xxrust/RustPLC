# RustPLC 拓扑语义与关系验证规范

| 字段 | 内容 |
| :--- | :--- |
| 版本 | 2.0 |
| 状态 | Final |
| 日期 | 2026-02-23 |

---

## 1. 核心原则

1. 拓扑语义正确性先于形式化验证。
2. 拓扑连线唯一来源是 `relation { from, to, via }`。
3. 设备属性写法 `driven_by/reports_to/detects` 已移除，不再兼容。
4. 非 IO 设备必须显式写 `Device.Port`；PLC IO 点位允许写简写 `Y0`/`X0`/`AI0`/`AO0`。

---

## 2. DSL 语法（可执行）

### 2.1 端口声明

```plc
[topology]
device valve_A: solenoid_valve {
    ports: [coil:digital:consumer, out:pneumatic:producer]
}
```

端口格式：`ports: [port_id:port_type:port_role, ...]`

- `port_type`: `digital | analog | pneumatic | logical | generic`
- `port_role`: `producer | consumer | bidirectional`

### 2.2 关系声明

```plc
relation { from: Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: sensor_A.out, to: X0, via: reports_to }
```

- `via`: `driven_by | reports_to | detects`
- `from/to` 支持：
  - `Device.Port`（标准写法）
  - `Device`（仅限 PLC IO 点位：`Y* / X* / AI* / AO*`）

---

## 3. 语义门禁规则

| 规则 | 说明 | 错误码 |
| :--- | :--- | :--- |
| 端口存在性 | 端口不存在、或非 IO 设备使用了端口简写 | `SEM-101` |
| 方向 | 必须满足 producer -> consumer | `SEM-102` |
| 类型兼容 | 关系类型与端口类型必须匹配 | `SEM-103` |
| 语义角色 | `detects` 要求 state -> detector | `SEM-104` |
| 悬空端口 | 显式声明端口必须参与 relation | `SEM-105` |
| subtype 兼容 | `device_type` 与 `subtype` 必须在矩阵内 | `SEM-106` |

---

## 4. 标准建模模板

### 4.1 双输入阀（标准）

```plc
[topology]
device Y5: digital_output
device Y6: digital_output
device X8: digital_input
device X9: digital_input

device valve_eject: solenoid_valve {
    ports: [coil_extend:digital:consumer, coil_retract:digital:consumer, out:pneumatic:producer]
}

device cyl_eject: cylinder

device sensor_eject_ext: sensor
device sensor_eject_ret: sensor

relation { from: Y5, to: valve_eject.coil_extend, via: driven_by }
relation { from: Y6, to: valve_eject.coil_retract, via: driven_by }
relation { from: valve_eject.out, to: cyl_eject.cmd, via: driven_by }
relation { from: cyl_eject.extended, to: sensor_eject_ext.sense, via: detects }
relation { from: sensor_eject_ext.out, to: X8, via: reports_to }
relation { from: cyl_eject.retracted, to: sensor_eject_ret.sense, via: detects }
relation { from: sensor_eject_ret.out, to: X9, via: reports_to }

[constraints]
causality: Y5 -> valve_eject -> cyl_eject -> sensor_eject_ext
causality: Y6 -> valve_eject -> cyl_eject -> sensor_eject_ret

[tasks]
task main:
    step idle:
```

### 4.2 可扩展多 IO 设备（motor 示例）

```plc
[topology]
device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device axis_feed: motor {
    ports: [cmd_fwd:digital:consumer, cmd_rev:digital:consumer, speed_ok:logical:producer, alarm:logical:producer]
}

device speed_ok_sensor: sensor
device alarm_sensor: sensor

relation { from: Y0, to: axis_feed.cmd_fwd, via: driven_by }
relation { from: Y1, to: axis_feed.cmd_rev, via: driven_by }
relation { from: axis_feed.speed_ok, to: speed_ok_sensor.sense, via: detects }
relation { from: speed_ok_sensor.out, to: X0, via: reports_to }
relation { from: axis_feed.alarm, to: alarm_sensor.sense, via: detects }
relation { from: alarm_sensor.out, to: X1, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
```

---

## 5. 反例（必须拦截）

| 反例 | 预期 |
| :--- | :--- |
| `relation { from: Y5, to: valve_eject.coil, via: driven_by }`（双输入阀端口名错误） | `SEM-101` |
| `relation { from: X0, to: valve_A.coil, via: driven_by }` | `SEM-102` |
| `relation { from: valve_A, to: cyl_A, via: driven_by }`（非 IO 设备省略端口） | `SEM-101` |

---

## 6. 迁移指南（旧 -> 新）

| 旧写法（已移除） | 新写法 |
| :--- | :--- |
| `device valve_A { driven_by: Y0 }` | `relation { from: Y0, to: valve_A.coil, via: driven_by }` |
| `device sensor_A { reports_to: X0 }` | `relation { from: sensor_A.out, to: X0, via: reports_to }` |
| `device sensor_A { detects: cyl_A.extended }` | `relation { from: cyl_A.extended, to: sensor_A.sense, via: detects }` |

迁移原则：

1. 先补全每个设备真实端口（必要时用 `ports: [...]` 显式声明）。
2. 再把所有拓扑语义改写为 `relation`。
3. 最后运行 `cargo test --workspace` 进行全量回归。
