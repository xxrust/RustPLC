# Tray Scan NG Sorting DSL Draft

## 1. 文档定位

这份文档只做一件事：

- 给出“32x24 tray 全检后统一吸取 NG”场景的语义 DSL 草案

它不是当前编译器契约，也不讨论“最优分析”。

这里的目标只有两个：

1. 先确认这个站的核心对象、状态和流程是否能被 RustPLC 的新工件语义完整表达。
2. 先把“站要做什么”写清楚，再决定后续哪些语法值得进入正式 spec。

---

## 2. 场景边界

本草案针对的场景是：

- 一个 `32 x 24` 的 tray 盘
- Y 轴带 tray 在“行”方向运动
- X 轴带相机和吸嘴在“列”方向运动
- 先扫描 tray 上全部工位
- 对每个工件写入检测结果
- 全部扫描结束后，再统一吸取 NG 工件到 NG 盒

这里特意采用“两阶段”流程，而不是“边扫边吸”，原因是：

- 更贴近很多现场设备的真实工艺
- 检测阶段动作单纯，节拍稳定
- 分拣阶段动作单纯，状态更清楚
- 不会把“检测结果产生”和“分拣动作执行”耦合成一个混乱循环

---

## 3. 语义目标

这个站至少要能正式表达下面这些事：

### 3.1 工件对象

- tray 里的单个物料是工件，而不是 tray 本身
- 每个工件都有自己的检测结果

### 3.2 载具与槽位

- tray 是 `carrier`
- tray 具有二维离散槽位
- 某个具体工位可写成 `tray_scan.slot[row, col]`

### 3.3 两阶段流程

阶段一：

- 扫描全部槽位
- 仅写入结果，不搬走工件

阶段二：

- 找出所有 `ng` 工件
- 再执行统一分拣

### 3.4 出入口与终态

- 工件从 `tray_scan.slot[*, *]` 进入本站流程
- OK 工件在本站结束后仍留在 `tray_scan.slot[*, *]`
- NG 工件通过 `ng_box` 离开本站流程
- `finished` 只能对应正常出口
- `rejected` 只能对应异常出口

---

## 4. DSL 草案

下面的 DSL 草案是“语义草案”，不是当前 parser 契约。

```plc
[topology]
variable tray_row_zero: float = 0
variable tray_row_pitch: float = 1
variable cam_col_zero: float = 0
variable cam_col_pitch: float = 1
variable ng_box_x: float = 500

workpiece die: workpiece_type {
    properties: [
        inspect_result: enum(unknown, ok, ng)
    ]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    ingress_sites: [tray_scan.slot[*, *]]
    normal_egress_sites: [tray_scan.slot[*, *]]
    abnormal_egress_sites: [ng_box]
}

carrier tray_scan: workpiece_carrier {
    layout: grid(rows: 32, cols: 24)
}

holder nozzle: workpiece_holder { capacity: 1 }
location ng_box: workpiece_location { capacity: 300 }

scan tray_scan by row_major

extern function vision_inspect(row: int, col: int) -> enum(ok, ng) {
    rust_module: "vision.tray_station",
    pure: false,
    time_bound_us: 20000
}

[tasks]
task scan_all:
    foreach slot(row, col) in tray_scan by row_major:
        step move_tray_to_row:
            compute y_target = tray_row_zero + row * tray_row_pitch
            action: axis.move_absolute(axis_y, position: y_target, speed: 80)

        step move_camera_to_col:
            compute x_target = cam_col_zero + col * cam_col_pitch
            action: axis.move_absolute(axis_x, position: x_target, speed: 80)

        step inspect_slot:
            action: call vision_inspect(row, col) -> inspect_result
            effect: set_property tray_scan.slot[row, col].inspect_result = inspect_result
            continue foreach

    on_complete: goto collect_ng

task collect_ng:
    foreach slot(row, col) in tray_scan by row_major where inspect_result == ng:
        step move_tray_to_row:
            compute y_target = tray_row_zero + row * tray_row_pitch
            action: axis.move_absolute(axis_y, position: y_target, speed: 80)

        step move_nozzle_to_col:
            compute x_target = cam_col_zero + row * 0 + col * cam_col_pitch
            action: axis.move_absolute(axis_x, position: x_target, speed: 80)

        step pick_ng:
            action: set nozzle_down = true
            wait: nozzle_down_fb == true
            timeout: 300ms -> goto fault
            action: set vacuum_on = true
            wait: vacuum_fb == true
            timeout: 200ms -> goto fault
            effect: acquire holder nozzle from tray_scan.slot[row, col]

        step move_to_ng_box:
            action: axis.move_absolute(axis_x, position: ng_box_x, speed: 100)

        step drop_ng:
            action: set vacuum_on = false
            effect: transfer from nozzle to ng_box
            effect: finish workpiece at ng_box as rejected
            continue foreach

    on_complete: goto done

task done:
    step idle:
        allow_indefinite_wait: true

task fault:
    step safe_stop:
        action: set vacuum_on = false
        action: set nozzle_down = false
        allow_indefinite_wait: true
```

---

## 5. 这个草案想表达的关键点

### 5.1 tray 是 carrier，不是工件

- `tray_scan` 本身是载具
- 真正被检测和分拣的是 `die`

### 5.2 槽位是二维离散地址

- `tray_scan.slot[row, col]` 是逻辑工位
- 它不是连续空间坐标
- 行列地址和轴位置之间的映射，由 `row_pitch` / `col_pitch` 负责

### 5.3 检测与分拣是两个 task

- `scan_all` 只负责得到结果
- `collect_ng` 只负责搬走 NG

这比“检测到一个 NG 就立刻吸走”更清楚，也更容易验证。

### 5.4 OK 与 NG 的流程收敛不同

- OK 工件没有离开 tray，本站结束后仍在原 tray 内
- NG 工件离开 tray，进入 `ng_box`

所以：

- `finished -> tray_scan.slot[*, *]`
- `rejected -> ng_box`

---

## 6. 这份草案隐含需要的新语义点

如果将来要把这份草案变成正式语法，至少还要冻结下面这些点：

1. `layout: grid(rows: m, cols: n)`
2. `slot[row, col]`
3. `scan <carrier> by <order>`
4. `foreach slot(row, col) in <carrier> by <order>`
5. `where inspect_result == ng`
6. `set_property <slot>.inspect_result = <value>`
7. `finish workpiece at <site> as <terminal_state>`
8. `continue foreach`

---

## 7. 暂不在本草案解决的问题

这份文档故意不讨论以下问题：

1. NG 槽位在第二阶段应按什么顺序吸取才最优
2. 是否要允许蛇形扫描、跳行扫描、分区扫描
3. 是否要支持多吸嘴并行分拣
4. 是否要支持检测结果不稳定后的复检策略
5. 是否要把“扫描策略”和“分拣策略”单独建模成可替换工艺方案

这些问题不属于“这个站能否被工件语义表达”，而属于更上层的规划、策略和最优分析问题。
