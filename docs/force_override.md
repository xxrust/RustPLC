# FORCE / Override（仿真层）设计与用法

日期：2026-02-19

本文件描述 RustPLC 在 **SIL（仿真）层**对 OpenPLC “FORCE / override” 能力的最小实现：

- 用于**复现现场故障**、**验证互锁**、**构造可回归场景**；
- 目标是**确定性（deterministic）**与**可复现（replayable）**；
- 仅影响 SIL 仿真（`SimIo` / scenario 驱动），不直接等价于真实硬件上的强制写入能力。

---

## 1. 概念与边界

### 1.1 什么是 FORCE

FORCE 是一种“控制面”能力：在 PLC 运行时，把某个 IO 通道的最终可见值**固定为指定值**（覆盖 plant / scheduled / 程序写入），直到显式解除。

在 RustPLC 里，FORCE 的落点是：

- `crates/sim` 的 `SimIo`：实现强制覆盖语义；
- `scenario.yaml` 的 `forces`：实现**可回归**的 force/clear 脚本化注入。

### 1.2 开发态在线 force 控制面（`sim-plc`）

`sim-plc` 支持开发态在线 force 控制面，入口参数如下：

- `--enable-online-force-dev`：显式启用（默认关闭）；
- `--online-force-script <script.jsonl>`：按时间注入 force set/clear；
- `--online-force-audit-out <audit.jsonl>`：输出审计日志（未指定时默认写到 `online_force_audit.jsonl`）。

控制面仅在 **SIL/开发态仿真** 中工作，不改变板级固件 IO 控制行为。

### 1.3 语义边界

- 不在 DSL/runtime 核心语义中引入 IEC retain 变量在线写入；
- 不承诺与 OpenPLC force 协议一一兼容；
- 面向调试与回归证据链，优先可复现与可审计。

---

## 2. 语义与优先级（非常关键）

### 2.1 输入通道（DI/AI）

DI/AI 的读取优先级：

1. `force input`
2. `plant update`
3. `scheduled inputs`

实现要点：

- FORCE 输入在 **read-time** 生效：`read_digital_input` / `read_analog_input` 会先查 forced map。
- 这样做的好处是：底层 plant/scheduled 仍然会继续推进；当清除 force 时，读取会立即“显露”最新的底层状态。

### 2.2 输出通道（DO/AO）

DO/AO 的最终输出优先级：

1. `force output`
2. `program writes`

实现要点：

- 当输出被 force 时，程序对该通道的写入会被覆盖；
- `digital_edges` / `analog_edges` 记录的是**最终可观察到的输出值**（而不是“程序尝试写入值”），避免误导调试。

### 2.3 清除 force

force 的清除是显式的（并且在 YAML 中需要可表达）。

- YAML：使用 `null` 清除（例如 `0: null`）。
- `SimIo` API：传 `None` 清除。

注意：清除输出 force 不会“自动回到程序想要的值”，只有当程序下一次写该输出时，最终输出才会随之变化（这是可解释、可验证、也更符合实际控制系统的行为）。

---

## 3. Scenario YAML：forces 格式

`scenario.yaml` 新增字段：

```yaml
forces:
  - at_ms: 0
    set:
      digital_inputs:
        0: true
      analog_inputs:
        0: 7.5
      digital_outputs:
        0: true
      analog_outputs:
        0: 1.0
  - at_ms: 100
    set:
      digital_inputs:
        0: null
      analog_inputs:
        0: null
      digital_outputs:
        0: null
      analog_outputs:
        0: null
```

规则：

- `at_ms` 必须满足：
  - `at_ms < duration_ms`（当 `duration_ms != 0`）
  - `at_ms % tick_ms == 0`（tick 对齐）
- `set.*` 的 key 为通道 id（0 表示 DI0/AI0/DO0/AO0）
- value：
  - `true/false` 或浮点数：设置 force
  - `null`：清除该通道的 force

---

## 4. 在线 force 脚本与审计

在线脚本为 JSONL，每行一条命令：

```json
{"at_ms":0,"actor":"commissioning","source":"panel","channel":"DI0","value":true}
{"at_ms":20,"actor":"commissioning","source":"panel","channel":"DI0","value":null}
{"at_ms":30,"actor":"commissioning","source":"panel","channel":"AO0","value":1.25}
```

字段约束：

- `at_ms`：必须与 `tick_ms` 对齐；
- `actor/source`：审计来源标识；
- `channel`：`DI<n>/AI<n>/DO<n>/AO<n>`；
- `value`：bool/number 为 set，`null` 为 clear。

审计输出为 JSONL，包含：

- `at_ms`/`tick`（时间戳）；
- `actor`/`source`（操作者与来源）；
- `channel`/`channel_kind`/`channel_id`；
- `operation`（`set`/`clear`）；
- `from`/`to`（值变更）。

与 `scenario.yaml -> forces` 的交互：

- 运行前先加载 `scenario.yaml` 的 `forces`；
- 再注入在线脚本命令（同一 tick 允许多通道并发变更）；
- 最终统一按 `at_ms` 排序执行，保持确定性回放。

## 5. 最小可运行示例

示例文件：

- PLC：`examples/force_override_demo.plc`
- Scenario：`scenarios/force_override_demo/force.yaml`

运行（输出 trace 仅用于确认流程跑通；更直观的效果可查看输出边沿工件或 VCD/CSV 工具链）：

```bash
cargo run --release -- sim-plc examples/force_override_demo.plc \
  --scenario scenarios/force_override_demo/force.yaml \
  --out out/force_override_demo_trace.jsonl
```

预期现象（概念层面）：

- `X0` 初始为 false，但在 `forces` 中被强制为 true，从而流程能启动；
- `Y0/AO0` 在一段时间内被强制为固定值，程序写入不会改变最终输出；
- 清除输出 force 后，后续程序写入会重新反映到最终输出，并在 edges 中可观察到变化。
