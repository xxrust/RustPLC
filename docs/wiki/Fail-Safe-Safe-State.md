# 故障安全状态（Fail-Safe Safe State）

当固件检测到运行时错误时，所有输出必须进入已知安全状态。这是软件层的 best-effort 安全补丁 — 对可控错误路径可靠，对掉电/硬死机等不可控终止仍依赖硬件安全链。

---

## 配置

在 `io_map.toml` 的 `[safe_state]` 段定义安全态策略：

```toml
[safe_state]
mode = "profile"          # all_zero | profile
on_exit_timeout_ms = 300  # 保留字段

# 数字输出（key 支持 Y<id> 或 do<id>）
[safe_state.do.Y2]
safe_value = 0
group = 10

[safe_state.do.do1]
safe_value = 0
group = 20

# 模拟输出（key 支持 AO<id> 或 ao<id>）
[safe_state.ao.AO0]
safe_value = 0.0
group = 30
```

### 两种模式

| 模式 | 行为 |
|------|------|
| `all_zero` | 所有 DO/AO 直接写 0（默认） |
| `profile` | 按 `group` 顺序写入指定 `safe_value` |

`group` 数值越小越先执行，同组点位同一轮写入。

典型用例：抱闸需要先断电（group=10），再释放使能（group=20），最后将非安全输出写入安全值（group=30）。

---

## 固化到固件

`board-rp2040` 的 `build.rs` 在编译期读取 `RUST_PLC_IO_MAP_TOML`，将 safe_state 配置生成为固件内的常量：

- `SAFE_STATE_MODE` — 0=all_zero, 1=profile
- `SAFE_DO_DEFINED` / `SAFE_DO_VALUE` / `SAFE_DO_GROUP`
- `SAFE_AO_DEFINED` / `SAFE_AO_VALUE` / `SAFE_AO_GROUP`

配置与 DI/DO/AO GPIO 映射同源，编译期固化，运行时零开销查表。

---

## 触发条件

RP2040 固件在运行时遇到 `tick_with_trace_and_logs` 返回 `Err` 时：

1. 输出错误日志
2. 调用 `io.enter_safe_state()` 写入安全态
3. 进入无限空转（停止执行 PLC tick）

---

## 边界

| 场景 | 覆盖 |
|------|------|
| 运行时返回错误 | 覆盖 — 可控错误路径 |
| panic / HardFault | 不保证 — 依赖硬件安全链 |
| 掉电 | 不保证 — 依赖硬件安全链 |

关键安全仍需依赖硬件安全链（急停继电器、安全 PLC、机械限位等）。软件安全态是补充，不是替代。

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/io_map.rs` | io_map 解析与模板生成 |
| `crates/board-rp2040/src/main.rs` | 固件安全态触发逻辑 |
| `crates/board-rp2040/build.rs` | 编译期常量生成 |
| `docs/已实现/fail_safe_safe_state.md` | 设计文档 |
