# Scenario Playbook（从 0 到回归门禁）

说明：
- 本页聚焦 scenario 资产的生成、校验与仿真。
- 业务意图与实际 trace 的对齐方法见 `docs/architecture/intent_alignment_verification.md`。

目标：让“场景（scenario.yaml）怎么写、怎么验证、怎么回归、怎么最小化失败”变成一条可复制的标准流程。

> 约定：以下命令默认在**仓库根目录**运行（确保 `examples/`、`scenarios/` 路径可用）。

相关建模规范/规则模板：
- 步进轴（Pulse/Dir）+ AB 编码器安全互锁建模：`docs/已实现/stepper_ab_encoder.md`

## 1) 从 .plc 初始化场景骨架（推荐起点）

```bash
cargo run --release -- scenario-init examples/assembly_station.plc \
  --out out/assembly_station.scenario.yaml --preset normal
```

常用 preset：
- `normal`：可运行的 happy-path 骨架（含 start 脉冲 + 传感器边沿示例）
- `timeout`：触发超时路径（通常不脚本传感器到位）
- `sensor_stuck`：注入传感器卡死 fault 示例
- `bounce`：按键抖动示例

## 2) 运行前校验（避免“跑起来才发现 YAML 写错/风险很高”）

```bash
cargo run --release -- scenario-validate examples/assembly_station.plc \
  --scenario out/assembly_station.scenario.yaml
```

校验会给出：
- YAML 路径定位（缺字段/类型/时间对齐/上界）
- PLC/场景不匹配提示（含可复制的修复命令）
- same-tick loop 等高风险提示（含修复片段）

## 3) 看清“语法糖展开后到底是什么输入脚本”（调试/评审用）

如果你在场景里写了 `pulse` / `hold`（或使用设备名写法），可以导出展开后的 canonical 输入：

```bash
cargo run --release -- scenario-expand examples/assembly_station.plc \
  --scenario examples/scenarios/pulse_hold.yaml --out out/pulse_hold.expanded.yaml
```

## 4) 单场景 SIL 仿真（生成 trace）

```bash
cargo run --release -- sim-plc examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out out/trace.jsonl
```

## 5) 批量生成场景（参数化覆盖边界）

```bash
cargo run --release -- scenario-gen --plc examples/assembly_station.plc \
  --config examples/scenario_gen/basic.yaml --out-dir out/scenario_gen
```

输出：
- `out/scenario_gen/scenario_0001.yaml` ...（可直接喂给 sim/no-board-gate）
- `out/scenario_gen/summary.json`（记录每个 case 的参数与文件名）

## 6) 批量回归 + 失败自动最小化（调试效率关键）

```bash
cargo run --release -- sim-regress \
  --plc-dir examples \
  --scenario-dir out/scenario_gen \
  --artifacts-dir out/sim-regress \
  --summary-out out/sim-regress/summary.json \
  --minimize-failure
```

失败用例会在 `out/sim-regress/case_XXXX/` 下生成：
- `minimized_scenario.yaml`：最小复现场景（含来源信息 + 下一步建议）
- `minimized_trace.jsonl` / `minimized_report.json`

更详细的回灌流程见：`docs/已实现/scenario_minimization.md`

## 7) 门禁（SIL vs virtual-board + 实时阈值）

无开发板门禁 playbook 见：`docs/已实现/no_board_playbook.md`

## 常见错误（以及怎么修）

1) 找不到场景文件（路径/工作目录不对）

如果你看到类似：

```text
Scenario YAML file not found: scenarios/normal.yaml
  cwd: ...
```

优先检查：
- 是否在仓库根目录运行
- `--scenario` 路径是否正确

或直接用 `scenario-init` 生成与你的 `.plc` 匹配的骨架：

```bash
cargo run --release -- scenario-init <file.plc> --out out/my.scenario.yaml --preset normal
```

2) 场景与 PLC 不匹配（用错了示例场景）

当你把 `scenarios/normal.yaml` 用在 `examples/two_cylinder.plc` 上时，CLI 会给出可复制的修复命令，按提示改为对应的场景文件即可（或先跑 `scenario-validate` 让它告诉你哪里不匹配）。
