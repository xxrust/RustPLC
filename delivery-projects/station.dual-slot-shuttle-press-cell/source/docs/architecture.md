# DualSlot Shuttle Press Cell Architecture

## Compile Surface

`rustplc.bundle.toml` 按 `00_topology` 到 `07_hmi` 组织语义职责；`process_model/process_operation_model.toml` 是 task/step 之前的源侧调度契约。

## Ownership

- `00_topology`: controller aliases、现场设备、双槽 carrier 与 workpiece endpoint。
- `01_init`: 轴、气缸、可见输出和残件门禁的启动自检。
- `02_process`: front-door、装卸、穿梭运动和压装并发任务。
- `03_constraints`: `shuttle_envelope`、`load_station_access` 与时序约束。
- `04_faults`: recoverable 和 maintenance fault route。
- `05_supervision`: ready/running/fault 状态发布。
- `06_manual`: reset 与 operator-assist 边界。
- `07_hmi`: 灯、蜂鸣器、拒绝原因和故障码义务。

## Execution Model

root tasks 各自拥有运行上下文。blocking action 只阻塞当前 task。共享资源和显式 handoff state 协调任务，系统没有全局 program counter。

## Semantic Boundary

workpiece slot、axis route、cylinder outcome 和 operator front-door 都必须进入统一编译语义；runtime 不猜 token 或物理结果，verification 检查并发、资源、工件、时序和因果关系。
