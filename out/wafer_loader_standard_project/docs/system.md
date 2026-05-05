# 测角机上料器系统说明

本项目是标准 scaffold 版本，顶层语义源为 `plc/main.system.md`。

## 工艺边界

- 从 `feed_cassette` 取出单片晶片。
- 出料机构将晶片交给 `slide_pick_site`。
- 小旋臂从滑轨取片并交给 `orient_inspection_site`。
- 辨相台完成初始方向判定和必要的 180 度翻转判定。
- 摆缸从辨相台取片并交付 `measure_stage_site`。
- 异常晶片进入 `reject_bin`，掉片路径以 `dropped` 关闭。

## 正向建模顺序

```text
system contract
  -> process_model/process_operation_model.toml
  -> task/step program flow
  -> process-model-check
```

`operation-model` 只用于迁移或审计已有 task/step，不是本项目的默认源头。

## 并发任务

- `feed_prep`：出料与滑轨准备。
- `orient_stage`：旋臂取片、辨相、翻转、拒收。
- `transfer_to_measure`：摆缸取片、测片台交付、掉片处理。
- `supervisor`：自动模式和启动/停止 front-door。
- `architecture_monitor`：掉片率等架构状态派生。
