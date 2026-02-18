# Fail-Safe Safe State（异常退出安全态）

日期：2026-02-18

本页是对 `docs/fail_safe_safe_state.md` 的“可执行落地版”补充：把安全态策略如何进入 `io_map.toml`、如何被固化到固件、以及固件在运行时何时触发 Safe State 说明清楚。

## 1. 目标

- 默认策略：**输出置 0（de-energize）**
- 少量例外：通过 `profile` 为“抱闸/禁能/阀位”等点位配置 `safe_value` 与 `group` 顺序
- 该能力属于**软件 best-effort**：仅对“可控停止/可控错误路径”可靠；对掉电/硬死机等不可控终止仍依赖硬件安全链

## 2. 配置：io_map.toml 的 `[safe_state]`

当前实现读取 `io_map.toml` 的以下结构（示例）：

```toml
[digital_inputs]
di0 = 2

[digital_outputs]
do1 = 16  # step_en
do2 = 17  # brake_coil (NC brake, 0=brake)

[safe_state]
mode = "profile"  # all_zero | profile
on_exit_timeout_ms = 300  # 目前在 RP2040 固件侧未使用，保留字段

# 数字输出：key 支持 Y<id> 或 do<id>
[safe_state.do.Y2]
safe_value = 0
group = 10

[safe_state.do.do1]
safe_value = 0
group = 20

# 模拟输出：key 支持 AO<id> 或 ao<id>
[safe_state.ao.AO0]
safe_value = 0.0
group = 30
```

语义：

- `mode="all_zero"`：所有 DO/AO 直接写 0（默认）
- `mode="profile"`：仅对配置过的点位按 `group` 顺序写入 `safe_value`
- `group`：数值越小越先执行；同组点位同一轮写入（不保证组内顺序）

## 3. 固化：RP2040 固件如何拿到配置

`board-rp2040` 的 `build.rs` 会在编译期读取 `RUST_PLC_IO_MAP_TOML`，并把 safe_state 生成到固件内的 `io_map.rs` 常量里（与 DI/DO/AO GPIO 映射同源）。

产物常量（示意）：

- `SAFE_STATE_MODE`：`0=all_zero`，`1=profile`
- `SAFE_DO_DEFINED/SAFE_DO_VALUE/SAFE_DO_GROUP`
- `SAFE_AO_DEFINED/SAFE_AO_VALUE/SAFE_AO_GROUP`

## 4. 触发：固件何时进入 Safe State

当前 `board-rp2040` 固件在运行时遇到“Runtime tick 失败”（`tick_with_trace_and_logs` 返回 Err）时：

1. 输出错误日志
2. 调用 `io.enter_safe_state()` 写入 Safe State
3. 进入无限空转（停止继续执行 PLC tick）

说明：

- 这是“可控错误路径”的 best-effort 安全补丁，能覆盖“运行时返回错误”这类可检测故障
- 对 panic/HardFault/掉电等不可控终止，不能保证该逻辑一定执行；关键安全仍需依赖硬件安全链

## 5. 相关文档

- 设计口径：`docs/fail_safe_safe_state.md`
- RP2040 固件实现：`crates/board-rp2040/src/main.rs`
- io_map 解析与模板：`src/io_map.rs`、`src/main.rs`（io_map.template.toml 生成）

