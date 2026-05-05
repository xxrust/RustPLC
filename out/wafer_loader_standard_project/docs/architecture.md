# 测角机上料器架构

## 标准分层

```text
plc/main.system.md
      |
00_topology/
      |
process_model/process_operation_model.toml
      |
01_init/
      |
02_process/
      |
03_constraints/
      |
04_faults/
      |
rustplc.bundle.toml -> IR -> verification / runtime bridge / codegen
```

## 关键原则

- 拓扑描述设备、连接、工件位置、容量、资源边界。
- `process_model` 描述候选工艺操作、source/destination、admission 和共享资源。
- task/step 只表达 PLC 如何执行候选操作，不把调度意图反向藏在程序流里。
- `process-model-check` 负责验证 task/step 是否 refine 源侧模型。
- `supervisor` 负责 front-door 和模式仲裁，不是工艺设备，也不属于 `02_process/` 的生产主流程。
- `05_supervision/`、`06_manual/`、`07_hmi/` 是预留运行层，默认禁用不代表主流程缺失。

## 资源边界

- `slide_pick_zone`：出料气缸前进与旋臂进入滑轨取片区互斥。
- `arm_orient_zone`：摆缸下摆与旋臂进入辨相区互斥。
- `orient_inspection_site`：辨相台吸附面，是辨相与转运之间的工件交接点。

## 任务边界

- `startup_initializer` 建立初始化基线。
- `supervisor` 管理自动模式与 operator front-door。
- `feed_prep` 只负责把一片晶片准备到滑轨取料位。
- `orient_stage` 只负责旋臂取片与辨相结果分流。
- `transfer_to_measure` 只负责从辨相台取片到测片台或异常出口。
- fault task 只处理已定义异常路径，不发明新的工件位置。
