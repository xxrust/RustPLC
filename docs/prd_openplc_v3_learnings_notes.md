# PRD 补充说明：OpenPLC_v3 Learnings（开发期：偏向“整洁方案”，允许破坏兼容）

日期：2026-02-18

本文是 `prd.json`（branch: `ralph/openplc-v3-learnings`）的补充说明，用于：

- 解释为什么这些故事与 `docs/openplc_v3_analysis.md` 对齐
- 明确“准备修改/新增的输出格式与命令接口”，并给出旧->新差异
- 明确开发期策略：**不强求兼容性**，优先采用更整洁的结构

注：fail-safe safe_state 已在当前仓库实现（不再纳入本 PRD）。

---

## 1. 总体取舍：为何不追求兼容

本 PRD 的核心目标是“把 OpenPLC 的工程范式沉淀到 RustPLC”，而不是维护 CLI/格式长期兼容。

因此我们选用更整洁的做法：

- 用一个 `board-parse` 产出“标准工件集合”（对齐 virtual-board 的输出文件名），而不是给旧命令不断加可选参数。
- 对“FORCE/override”优先落在可回归的 Scenario YAML，而不是只提供内部 API（否则使用门槛高、不可复现）。
- 对 IEC 地址支持采用 **工具链规范化（io-map-normalize）**，避免把多语法解析复杂度塞进核心 IoMap 解析器。

---

## 2. Board 日志：原格式 vs 目标工件格式

### 2.1 原格式：board.log（混合文本）

真实 RP2040 / pil-run 的日志是混合文本行，例如：

```
TICK tick=7 ts_ms=7
TRACE tick=7 task=0 from=1 to=2 reason=goto ts_ms=7
TIMING tick=7 ts_start_us=7000 ts_end_us=7120 exec_us=120 slack_us=880 overrun=false overrun_count=0
```

特点：

- 文本行，字段 `k=v` 空格分隔，顺序不保证稳定
- 可能夹杂其他行（boot/info/warn 等）
- TIMING 行可能带额外字段（如 `overrun_count`），但 timing-report 并不需要

### 2.2 目标工件 1：board_trace.jsonl

每行一个 trace 事件（结构化 JSONL），字段与现有 `TraceRow` 一致：

```json
{"tick":7,"task":0,"from_step":1,"to_step":2,"reason":"goto","timestamp_ms":7}
```

### 2.3 目标工件 2：tick_timing.jsonl

每行一个 `TickTimingSample`（结构化 JSONL）：

```json
{"tick":7,"ts_start_us":7000,"ts_end_us":7120,"exec_us":120,"slack_us":880,"overrun":false}
```

说明：

- 解析 TIMING 行时，只要求上述字段齐全；额外字段允许存在并忽略。
- JSON 字段顺序由 Rust struct 序列化确定，便于 diff 与门禁。

---

## 3. CLI 接口：旧 -> 新（破坏性变更）

### 3.1 旧：trace-parse（仅导出 trace.jsonl）

```
rust_plc trace-parse --in board.log --out trace.jsonl
```

### 3.2 新：board-parse（导出“标准工件集合”到目录）

```
rust_plc board-parse --in board.log --out-dir out/board_artifacts
```

输出文件：

- `out/board_artifacts/board_trace.jsonl`
- `out/board_artifacts/tick_timing.jsonl`

兼容策略：

- 开发期不保留 trace-parse（避免维护双入口与重复解析）。

---

## 4. FORCE/override：为何要进入 Scenario YAML

OpenPLC 的 FORCE 之所以工程上好用，关键是：

- 它是控制面能力（能“在运行时改值/锁值”）
- 它是可复现的（能在调试/回归中稳定重现）

RustPLC 当前优先在 **SIL 工具链** 做等价能力：把 force 配置写进 `scenario.yaml`（PRD: US-004..US-006）。

这样带来的收益：

- CI 可回归（scenario 就是证据与脚本）
- 语义可验证（force 与 plant/schedule/程序写入的优先级可写进测试）

---

## 5. IEC 地址：为何用 io-map-normalize，而不是污染核心 IoMap

在 RustPLC 里，IoMap 的职责应尽量简单：`di0/do0/ai0/ao0 -> gpio`。

IEC 地址（`%QX0.0` 等）是一种“工程师友好别名”，但直接让核心解析器支持多语法会带来：

- 解析路径分叉增多（长期维护成本上升）
- 错误信息与冲突规则更难写清楚

因此本 PRD 选择工具链方案：

- `io-map-normalize` 读取含 IEC key 的 io_map.toml
- 输出规范化后的 native key io_map.toml（只包含 di/do/ai/ao key）

核心 IoMap 解析器保持简洁、稳定。

