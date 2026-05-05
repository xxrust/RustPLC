# 结构化项目布局

RustPLC 的复杂项目用结构化目录表达完整工程意图。标准项目把组织思想拆成几个明确层次：

```text
plc/main.system.md
  -> 00_topology/
  -> process_model/process_operation_model.toml
  -> 01_init/
  -> 02_process/
  -> 03_constraints/
  -> 04_faults/
  -> 05_supervision/
  -> 06_manual/
  -> 07_hmi/
  -> rustplc.bundle.toml
```

## 每层做什么

- `00_topology/`：设备、连接、工件位置、容量、资源边界。
- `process_model/`：候选工艺操作的源侧调度意图，先于 task/step。
- `01_init/`：初始化和安全基线。
- `02_process/`：自动生产主流程。
- `03_constraints/`：安全与节拍约束。
- `04_faults/`：故障收敛与恢复。
- `05_supervision/`：模式仲裁、启动/停止、operator front-door。
- `06_manual/`：人工维护或手动操作。
- `07_hmi/`：HMI 展示和交互层。

## supervisor 的位置

`supervisor` 属于运行入口和模式管理层，职责是：

- 接收启动、停止、复位、模式选择等 front-door 命令。
- 锁存自动循环的使能状态。
- 管理任务启停与安全回退。
- 在需要时把系统拉回初始化基线。

所以，`05_supervision/` 默认禁用，并不表示“缺功能”，而是表示该层在当前项目中暂未启用。

## 为什么要这样组织

- 让 `process_model` 先表达“什么操作被允许”。
- 让 `02_process` 再表达“PLC 怎样执行这些操作”。
- 让 `04_faults` 专注异常收敛，不污染主流程。
- 让 `supervisor`、`manual`、`hmi` 从主工艺流里分离出去。

这样做的目标很直接：项目可以更容易审查、分工、并行开发和做形式化验证。
