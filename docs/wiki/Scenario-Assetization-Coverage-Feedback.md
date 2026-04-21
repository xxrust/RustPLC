# 场景资产化与覆盖反馈

场景不是一次性测试脚本，而是可版本化、可批量生成、可回归的工程资产。

---

## 场景工程命令

| 命令 | 用途 |
|------|------|
| `scenario-init` | 从 .plc 生成场景骨架 |
| `scenario-validate` | 校验场景与 .plc 的一致性 |
| `scenario-expand` | 展开 pulse/hold 语法糖为逐 tick 序列 |
| `scenario-gen` | 按覆盖策略批量生成场景 |
| `sim-plc` | 单场景 SIL 仿真 |
| `sim-regress` | 批量回归仿真 |

---

## 典型工作流

```bash
# 1. 生成场景骨架
cargo run --release -- scenario-init examples/assembly_station.plc \
  --out scenarios/normal.yaml --preset normal

# 2. 校验场景合法性
cargo run --release -- scenario-validate examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --output human

# 3. SIL 仿真
cargo run --release -- sim-plc examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out trace.jsonl

# 4. 批量回归
cargo run --release -- sim-regress \
  --plc-dir examples --scenario-dir scenarios \
  --artifacts-dir out/sim-regress --minimize-failure
```

---

## 覆盖策略

`scenario-gen` 支持三种覆盖模式：

| 模式 | 策略 |
|------|------|
| `pairwise` | 两两组合覆盖，平衡数量与覆盖度 |
| `boundary-first` | 优先覆盖边界值（超时临界、阈值边界） |
| `risk-first` | 优先覆盖高风险路径（故障注入、安全联锁） |

```bash
cargo run --release -- scenario-gen \
  --plc examples/rp2040_motion_minimal.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir out/scenario_gen \
  --coverage-mode risk-first \
  --dry-run
```

`--dry-run` 预览生成计划，不实际写入文件。

---

## 失败最小化与反馈

`sim-regress --minimize-failure` 在仿真失败时自动缩减场景，定位最小复现条件：

```bash
cargo run --release -- sim-regress \
  --plc-dir examples --scenario-dir scenarios \
  --artifacts-dir out/sim-regress --minimize-failure
```

输出 `out/sim-regress/feedback.json`，包含每个失败的模板和参数提示，可被 AI Agent 消费用于自动修复。

---

## 场景 YAML 结构

```yaml
tick_ms: 10
duration_ticks: 500

digital_inputs:
  - name: start_button
    ticks: [10, 11]

analog_inputs:
  - name: AI0
    values:
      - { tick: 0, value: 0.0 }
      - { tick: 100, value: 75.5 }

fault_injection:
  - type: sensor_stuck
    target: sensor_A
    at_tick: 200
```

---

## 模板库

场景模板存放在 `scenarios/templates/metadata.json`，`scenario-gen` 可引用模板库批量生成：

```bash
cargo run --release -- scenario-gen \
  --plc examples/assembly_station.plc \
  --template-library scenarios/templates/metadata.json \
  --out-dir out/scenario_gen
```

---

## 相关文件

| 文件 | 说明 |
|---|---|
| `src/cli/scenario.rs` | 场景命令实现 |
| `src/cli/sim.rs` | 仿真命令实现 |
| `scenarios/` | 场景文件目录 |
| `examples/scenario_gen/` | 生成配置示例 |
