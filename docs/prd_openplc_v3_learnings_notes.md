# PRD 补充说明：OpenPLC_v3 Learnings（输出/格式差异与兼容性）

日期：2026-02-18

本文是 `prd.json`（branch: `ralph/openplc-v3-learnings`）的补充说明，用于：

- 解释为什么这些故事与 `docs/openplc_v3_analysis.md` 对齐
- 明确“新增/改变的输入输出格式”及其与现有格式的差异
- 提前写清楚兼容性策略，避免后续误解为“要改掉现有格式”

注：fail-safe safe_state 已在当前仓库实现（不再纳入本 PRD）。

---

## 1. 现状回顾（与代码对应）

### 1.1 Tick timing 证据链（已存在，但缺少真实板日志转换）

仓库已存在统一的 tick timing 契约与工具链：

- `tick_timing.jsonl` 行格式（结构化 JSONL）由 `src/tick_timing.rs` 定义 `TickTimingSample`
- 统计报告 `timing-report` 基于 `tick_timing.jsonl` 生成 p50/p95/p99/max 与 overrun 计数
- `virtual-board` 生成的 `board.log`/`board_trace.jsonl`/`tick_timing.jsonl` 是“可重复”的仿真工件

缺口在于：**真实 RP2040 固件输出的是文本 TIMING 行**，但目前 CLI 只支持 `trace-parse` 导出 trace.jsonl，不支持从真实板日志导出 tick_timing.jsonl。

### 1.2 TRACE 解析（已存在）

当前 `trace-parse`：

- 输入：`board.log`（混合文本，包含 TRACE/TICK/LOG/TIMING 等行）
- 输出：`trace.jsonl`（结构化 JSONL）

---

## 2. 本 PRD 将新增/改变什么（格式层面）

本 PRD 的格式变化分两类：**新增导出工件** 与 **新增可接受的输入写法**。

### 2.1 从 board.log 导出 tick_timing.jsonl（新增导出工件，不改变固件输出）

#### 2.1.1 原始格式：固件 TIMING 行（文本）

RP2040 固件当前输出类似：

```
TIMING tick=7 ts_start_us=7000 ts_end_us=7120 exec_us=120 slack_us=880 overrun=false overrun_count=0
```

特点：

- 文本行，字段以 `k=v` 空格分隔
- 字段顺序可能不稳定（未来固件可能插入新字段）
- 可能包含 `overrun_count` 等 **TickTimingSample 不需要** 的字段

#### 2.1.2 目标格式：tick_timing.jsonl（结构化 JSONL）

每行是一个 `TickTimingSample`：

```json
{"tick":7,"ts_start_us":7000,"ts_end_us":7120,"exec_us":120,"slack_us":880,"overrun":false}
```

特点：

- 字段顺序保持稳定（由 Rust struct 序列化决定，便于 diff 与门禁）
- 一行一个 tick 的样本，天然适配 `timing-report` 与 `no-board-gate`

#### 2.1.3 兼容性策略

- **不要求修改固件日志格式**（仍输出 TIMING 文本行）
- CLI 新增/扩展“解析器”把 TIMING 文本转换为 tick_timing.jsonl
- 解析器对 TIMING 行采取“**只要必需字段齐全即可**”策略：
  - 允许字段乱序
  - 允许额外字段（例如 `overrun_count`）存在并忽略

### 2.2 trace-parse CLI 用法扩展（输出能力扩展，旧用法继续可用）

#### 2.2.1 原用法

```
rust_plc trace-parse --in board.log --out trace.jsonl
```

#### 2.2.2 新用法（扩展）

```
rust_plc trace-parse --in board.log --out trace.jsonl --timing-out tick_timing.jsonl
```

兼容性：

- `--timing-out` 为可选参数
- 不提供 `--timing-out` 时，行为与旧版本一致（只生成 trace.jsonl）

### 2.3 io_map.toml 支持 IEC 地址作为 key（新增输入写法，旧写法继续可用）

#### 2.3.1 原写法（现状）

```toml
[digital_outputs]
do0 = 16
do1 = 17
```

#### 2.3.2 新写法（别名输入）

IEC 风格地址由于包含 `%` 与 `.`，在 TOML 中必须使用 quoted key：

```toml
[digital_outputs]
"%QX0.0" = 16
"%QX0.1" = 17
```

兼容性与冲突规则：

- 可与 `do0/do1` 混用
- 若同一逻辑通道被两个 key 指向（例如 `do0` 与 `%QX0.0` 同时存在但数值不同），应报错并指出冲突来源

#### 2.3.3 映射规则（明确这是“别名映射”，不是完整 IEC 内存模型）

为了与 RustPLC 当前的“逻辑通道 ID”一致，本 PRD 的 IEC 支持仅作为 key 别名：

- `%QX<byte>.<bit>` -> `do_id = byte * 8 + bit`
- `%IX<byte>.<bit>` -> `di_id = byte * 8 + bit`
- `%QW<word>` -> `ao_id = word`（按“通道号”理解，不等同于 16-bit word 寄存器语义）
- `%IW<word>` -> `ai_id = word`（同上）

---

## 3. 为什么这些故事是“合理拆分”

对照 `docs/openplc_v3_analysis.md` 的可借鉴清单：

1. 运行时可观测性：优先补齐“真实板 -> 统一证据工件”的转换链路（US-001/US-002）
2. FORCE/override：先落在 SimIo（SIL/HIL 工具链），不碰运行时核心（US-003..US-005）
3. IEC 地址映射：先从 io_map alias 开始，降低迁移成本，不侵入 DSL 核心（US-006..US-008）
4. HAL 最小化：先对板端固件做结构重构，不改变行为，为多板卡扩展做准备（US-009）
5. 控制面协议：先定义可版本化的数据结构与文档，不立刻上服务端（US-010）

---

## 4. 非目标（避免误读）

- 不在本期把 RP2040 固件日志改为 JSON 日志（仅做 host 侧解析/转换）
- 不在本期实现完整的 IEC 61131-3 运行时（IEC 地址仅作为 io_map 输入别名）
- 不在本期引入 Web 管理层或远程 RPC 服务（仅定义协议与数据结构）

