# RustPLC 多 Agent 项目框架分析报告

## 1. 问题陈述

当前 `project new --layout structured-fragments` 生成的目录结构声称支持多 agent 并行开发，但经过深入分析，发现三个根本性问题：

1. **阶段间是严格串行的**，不存在阶段级并行窗口
2. **阶段内的多站并行缺乏协议约束**——编译器无法验证两个 agent 写的站合在一起是否安全
3. **当前编译器没有设备所有权机制**——任何 task 可以写任何 device，没有隔离保证

## 2. 阶段间为什么是串行的

PLC 程序的依赖不仅是符号引用，更是**状态前提依赖**：

| 阶段 | 状态前提 | 为什么不能跳过 |
|------|---------|--------------|
| 00_topology | 无 | 所有后续阶段引用设备名 |
| 01_init | 00_topology 已声明设备 | 建立安全态，是所有流程的入口条件 |
| 02_process | 01_init 已建立安全态 | 自动流程假设设备从安全态出发 |
| 03_constraints | 02_process 已定义 step 名 | 约束引用具体的 step 和设备状态 |
| 04_faults | 02_process 已定义异常路径 | 故障处理必须知道流程可能造成什么异常状态 |

这是不可压缩的串行链。之前声称 01_init 和 02_process 可以并行是错误的。

## 3. 多站并行的真正窗口

并行窗口在**阶段内部**，不在阶段之间。以 `02_process/` 为例：

```
02_process/
├── st01_loading.plc      ← Agent A
├── st02_assembly.plc     ← Agent B
├── st03_inspection.plc   ← Agent C
```

三个 agent 同时写三个站文件。但这个并行有一个**隐含假设**：三个站的设备集合不重叠、握手信号对得上、工件流不断裂。

**当前编译器无法验证这个假设。**

## 4. 当前编译器的隔离能力（现状）

| 机制 | 能做什么 | 不能做什么 |
|------|---------|-----------|
| `conflicts_with` | 声明两个设备状态不能同时为真 | 不能自动检测两个 task 写同一设备 |
| `SemanticResource(Exclusive)` | 声明两个 claim 不能同时成立 | 需要手动声明，不自动推导 |
| 工件 capacity | 自动检测同一 site 超容量 | 只对工件流有效，纯设备操作无保护 |
| `functional_group` | 限制组件间连接方向 | 拓扑层约束，不影响 task 层 |
| 设备所有权 | **不存在** | 任何 task 可以写任何 device |

关键缺口：**没有"设备分区"概念**。基恩士通过 `gSt01Cylinder[1..16]` / `gSt02Cylinder[1..16]` 在数据结构层面就隔离了站与站的设备。我们的拓扑模型中，所有设备是全局的。

## 5. 多站并行需要的协议（提案）

要让多站并行从"文件不冲突"升级为"编译器可验证"，需要三层协议：

### 5.1 设备分区（Device Partition）

每个站声明自己拥有哪些设备。编译器验证：
- 同一设备不被两个站同时拥有
- task 只能写自己所属站的设备（除非通过握手协议）

**DSL 草案**：
```
station st01_loading {
    owns: [valve_push, cyl_push, sensor_push_ext, sensor_push_ret]
}

station st02_assembly {
    owns: [valve_press, cyl_press, sensor_press_ext, sensor_press_ret]
}
```

编译器检查：如果 `st01_loading` 的 task 中出现 `action: extend cyl_press`，报错——`cyl_press` 属于 `st02_assembly`。

### 5.2 握手协议（Handshake Protocol）

站间通信必须通过显式声明的握手信号。编译器验证：
- 每个握手有 request/allow/complete 三个信号
- 每个握手有超时处理
- 不存在死锁（A 等 B 允许，B 等 A 完成）

**DSL 草案**：
```
handshake st01_to_st02 {
    from: st01_loading
    to: st02_assembly
    request: st01_outflow_request     # st01 置位
    allow: st02_inflow_allow          # st02 置位
    complete: st01_outflow_done       # st01 置位
    timeout: 5000ms -> goto fault
}
```

### 5.3 工件交接点（Workpiece Transfer Point）

站间工件流必须通过声明的交接点。编译器验证：
- 交接点 capacity = 1（同一时刻只有一个工件在交接）
- 上游站的 egress site = 下游站的 ingress site
- 交接顺序与握手协议一致

**DSL 草案**：
```
transfer_point st01_st02_handoff {
    from_station: st01_loading
    to_station: st02_assembly
    site: press_position          # workpiece_location, capacity: 1
    handshake: st01_to_st02       # 引用上面的握手协议
}
```

## 6. 有了协议之后的 Agent 协作模型

```
Phase 1 (Architect, serial):
    00_topology/
    ├── controller.plc          # PLC 声明
    ├── st01_devices.plc        # ST01 设备声明
    ├── st02_devices.plc        # ST02 设备声明
    ├── st03_devices.plc        # ST03 设备声明
    ├── workpieces.plc          # 工件类型和站点
    ├── connections.plc         # 设备连接关系
    └── station_protocol.plc    # station 分区 + handshake + transfer_point

Phase 2 (Architect, serial):
    01_init/
    └── defaults.plc            # 安全态初始化

Phase 3 (Station Agents, PARALLEL):
    02_process/
    ├── st01_loading.plc        ← Agent A（只能写 st01 拥有的设备）
    ├── st02_assembly.plc       ← Agent B（只能写 st02 拥有的设备）
    └── st03_inspection.plc     ← Agent C（只能写 st03 拥有的设备）

    编译器在此阶段验证：
    ✓ 每个 task 只写自己站的设备
    ✓ 站间通信只通过声明的握手信号
    ✓ 工件交接只通过声明的 transfer_point

Phase 4 (Safety Agent, serial):
    03_constraints/
    └── safety_rules.plc

Phase 5 (Fault Agent, parallel per station):
    04_faults/
    ├── st01_faults.plc         ← Agent A
    ├── st02_faults.plc         ← Agent B
    └── st03_faults.plc         ← Agent C
```

## 7. 与基恩士的对比

| 维度 | 基恩士 KVX | RustPLC 当前 | RustPLC 提案 |
|------|-----------|-------------|-------------|
| 设备隔离 | `gStXXCylinder[]` 数组天然隔离 | 无，全局设备 | `station { owns: [...] }` |
| 站间通信 | 手动握手变量 + 文档约定 | 无显式机制 | `handshake { request/allow/complete }` |
| 工件流 | 无（传统 PLC 不跟踪工件） | capacity 检查 | `transfer_point` + capacity |
| 模式管理 | `FB_McMode` 集中裁决 | 无 | `05_supervision/` (未来) |
| 编译期验证 | 无（靠人工 review） | 部分（safety/liveness） | 全量（分区+握手+工件流） |

## 8. 对当前框架的影响

### 不需要改的
- 目录结构 `00_topology/` 到 `07_hmi/` 的分层是对的
- 阶段间严格串行的 `depends_on` 是对的
- `rustplc.bundle.toml` 的 phase 声明机制是对的

### 需要改的
- `architecture.md` 中的 agent 并行描述需要加上协议前提
- `bundle.toml` 的注释需要说明"并行需要 station protocol"
- 长期：DSL 需要新增 `station`、`handshake`、`transfer_point` 语法
- 长期：验证引擎需要新增设备分区检查

### 建议的实施优先级
1. **立即**：修正 `architecture.md` 和 `bundle.toml`，诚实说明并行的前提条件
2. **短期**：在 `00_topology/` 的模板中加入 `station_protocol.plc` 占位，引导用户思考分区
3. **中期**：实现 `station { owns: [...] }` 语法和设备分区验证
4. **长期**：实现 `handshake` 和 `transfer_point` 语法及验证
