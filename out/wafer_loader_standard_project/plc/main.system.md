# 测角机上料器系统语义描述

本文件从 `docs/wafer_loader.system.md` 收敛而来，是本标准 scaffold 项目的顶层 system contract。
生成顺序固定为：

```text
main.system.md -> process_model/process_operation_model.toml -> 00/01/02/03/04 分层 PLC -> verification/runtime/codegen
```

## 项目身份

- 项目名称：测角机上料器
- 行业：半导体
- 控制器：Keyence KV-5500
- 介质：气动 + 步进轴
- 主要风险：晶片损失、节拍中断、机构干涉

## 工艺使命

系统负责从出料盒取出单片晶片，经滑轨取料位、小旋臂、辨相台、摆缸转运后交付到测片台。
辨相台通过同一个数字探测信号在旋转前后两次采样来判定方向：晶片放到辨相台并吸附后先记录旋转前探测值，再将辨相台旋转 180 度并记录旋转后探测值。
两次采样必须发生可判别变化；若两次相同，视为辨相传感器/旋光判定异常，当前晶片进入 IR/reject 盒。
联调临时约定：`before=true, after=false` 表示旋转前 +X 已朝滑轨方向，应回到原向；`before=false, after=true` 表示旋转后 +X 朝滑轨方向，应保持 180 度翻转状态。该映射需随正式光学标定冻结。
连续 3 片辨相异常时停机告警。

## 工艺操作调度模型

本项目不是“一个工件从头跑到尾后再放下一个工件”的固定大循环。调度策略为 opportunistic admission：

- source 有晶片
- destination 有容量
- 共享资源空闲
- 前序业务条件已满足

满足上述条件时，候选工艺操作即可被对应 task 执行。task/step 只是该调度模型的可执行投影。

源侧模型文件为：

```text
process_model/process_operation_model.toml
```

该文件必须先于 task/step 存在。task/step 生成或修改后，必须运行 `process-model-check` 验证是否 refine 该模型。

## 候选操作

- `feed_pick_cassette`：出料真空确认后从 `feed_cassette` 取片到 `feed_ejector`。
- `feed_release_to_slide`：出料吸嘴释放晶片到 `slide_pick_site`。
- `arm_pick_slide`：小旋臂吸嘴从 `slide_pick_site` 取片到 `arm_nozzle`。
- `arm_place_orient`：小旋臂将晶片交给 `orient_inspection_site`。
- `orient_reject`：辨相失败时从 `orient_inspection_site` 转入 `reject_bin` 并以 `rejected` 关闭。
- `transfer_pick_orient`：摆缸吸嘴从 `orient_inspection_site` 取片到 `transfer_nozzle`。
- `transfer_place_measure`：摆缸将晶片转移到 `measure_stage_site`。
- `finish_measure`：测片台成功接收后以 `handed_to_measure` 关闭。
- `transfer_pick_failure`：摆缸取片失败时从辨相台转入 `reject_bin` 并以 `rejected` 关闭。
- `transfer_drop_after_pick`：已取起后掉片时从转运吸嘴转入 `reject_bin` 并以 `dropped` 关闭。

## 并发 task

- `feed_prep`：出料机构出片与滑轨准备。
- `orient_stage`：旋臂取片、放片、辨相测量、翻转选择。
- `transfer_to_measure`：摆缸从辨相台取片并交付测片台。
- `supervisor`：自动模式 front-door、启动/停止命令、运行闩锁。
- `architecture_monitor`：派生掉片率等架构状态。

某 task 被 wait/delay/axis.move 阻塞时，不应阻塞其他无互锁冲突 task 推进。

## 共享资源与互锁

- `slide_pick_zone`：片盒前进与旋臂进入滑轨区互斥。
- `arm_orient_zone`：摆缸下摆与旋臂进入辨相区互斥。
- `orient_inspection_site`：辨相台吸附面是 `orient_stage` 与 `transfer_to_measure` 的工件交接点。

## 初始化

上电初始化必须关闭所有真空，收回出料/辨相/摆缸/升降/转运气缸，打开旋臂轴使能并等待原点确认。
`system_initialized` 是结构状态，应由初始化基线导出；当前实现以启动任务建立该基线。

## 维护自检

维护自检是 operator front-door 触发的独立任务，不属于自动生产节拍。`maintenance_self_check` 必须在维护模式下由操作员触发，先建立安全基线，再依次验证：

- 出料、旋臂、辨相台、转运四路真空的 on/off 反馈闭环。
- 出料、辨相翻转、摆缸、升降、转运五个气缸的伸出/缩回动作闭环。
- 旋臂轴小幅运动、回零命令与 home 反馈。

被 task 驱动的执行类设备必须有 maintenance/self-check 覆盖；若某设备不能由 PLC 自检，必须在机器可读配置中以 `self_check_exempt_devices` 写明 reason 与 proof_basis。缺失自检或豁免应由 `state-proof-check` 作为编译/项目检查失败暴露。

## 异常策略

- 真空取片窗口：1 秒，超时进入对应 fault task。
- 气缸不到位：警告挂起，等待操作员刷新。
- 旋臂轴异常：有限自动恢复，超过次数后停机告警。
- 辨相异常：当前片入 `reject_bin`，连续 3 片停机。
- 掉片：记录总数和连续次数，连续 2 次停机；长期掉片率超过 1/1000 停机。

## Operator Front-Door

启动、停止、刷新、模式选择、维护自检等人工入口属于 operator front-door，不把操作者建成普通设备。
瞬时按钮使用 `rising_edge(...)` 触发，不用“等待松手”步骤模拟边沿。

## 待联调冻结项

- 步进轴自动复位最大重试次数
- 气缸正式 timeout 标定值
- 旋臂轴 motion parameter set 最终命名
- 辨相数字探测值与 +X 方向的正式标定映射
