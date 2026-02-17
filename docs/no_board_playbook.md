# No-RTOS Real-Time Playbook（无开发板）

目标：在**不引入 RTOS**、且没有实体开发板的情况下，依然完成实时性证据链与发布门禁。

最小命令链（必须按序执行）：

1. `compile/verify`（结构与静态预算）
2. `virtual-board`（生成板侧等价日志与 tick 时序）
3. `timing-report`（统计 p50/p95/p99/max/mean）
4. `no-board-gate`（轨迹一致性 + 实时阈值）
5. `release-bundle`（打包可审计工件）

## 0) 前置

- 假设当前目录为仓库根目录。
- 示例 PLC：`examples/realtime_stress/stress_case.plc`
- 示例场景：
  - 安全：`examples/realtime_stress/scenarios/safe.yaml`
  - 高负载：`examples/realtime_stress/scenarios/overload.yaml`

## 1) compile/verify

```bash
cargo run --release -- examples/realtime_stress/stress_case.plc \
  --report out/realtime/verification_report.json \
  --budget-max-time-estimate-us 2000 \
  --deny-warnings
```

输入：`.plc`

输出：
- `out/realtime/verification_report.json`
  - 包含 `runtime_budget.budget_time_estimate`
  - 若超过预算阈值会产生 `timing.warn`

建议阈值：
- 初始可用 `--budget-max-time-estimate-us 2000`（1ms tick 系统建议逐步收紧到 1000~1500）。

## 2) virtual-board

```bash
cargo run --release -- virtual-board examples/realtime_stress/stress_case.plc \
  --scenario examples/realtime_stress/scenarios/safe.yaml \
  --out-dir out/realtime/virtual_board
```

输入：`.plc` + `scenario.yaml`

输出目录：
- `board.log`
- `board_trace.jsonl`
- `tick_timing.jsonl`
- `virtual_board_meta.json`

## 3) timing-report

```bash
cargo run --release -- timing-report \
  --in out/realtime/virtual_board/tick_timing.jsonl \
  --out out/realtime/virtual_board/timing_report.json
```

输入：`tick_timing.jsonl`

输出：
- `timing_report.json`
  - 至少包含：`count/overrun_count/exec_us_min/exec_us_p50/exec_us_p95/exec_us_p99/exec_us_max/exec_us_mean`

建议阈值评审：
- 首轮先记录基线，再在 `no-board-gate` 使用 `p99` 和 `overrun_count` 门禁。

## 4) no-board-gate

```bash
cargo run --release -- no-board-gate examples/realtime_stress/stress_case.plc \
  --scenario examples/realtime_stress/scenarios/safe.yaml \
  --out-dir out/realtime/gate \
  --max-p99-exec-us 120 \
  --max-overrun-count 0
```

输入：`.plc` + `scenario.yaml` + 阈值

输出目录：
- `sil_trace.jsonl`
- `board.log`
- `board_trace.jsonl`
- `tick_timing.jsonl`
- `timing_report.json`
- `diff_report.json`

失败条件：
- 轨迹不一致（trace mismatch）
- `p99_exec_us` 超过 `--max-p99-exec-us`
- `overrun_count` 超过 `--max-overrun-count`

建议阈值：
- 先在 safe 场景测基线，再以“基线 + 10%~20%”设定 `--max-p99-exec-us`。
- `--max-overrun-count` 推荐从 `0` 开始。

## 5) release-bundle

```bash
cargo run --release -- release-bundle examples/realtime_stress/stress_case.plc \
  --scenario examples/realtime_stress/scenarios/safe.yaml \
  --out-dir out/realtime/release \
  --max-p99-exec-us 120 \
  --max-overrun-count 0
```

输入：`.plc` + `scenario.yaml` +（可选）实时阈值

输出目录：
- `manifest.json`（所有工件哈希 + 大小）
- `verification_report.json`
- `sim_report.json` / `sil_trace.jsonl`
- `tick_timing.jsonl` / `timing_report.json`
- `gate_summary.json` / `diff_report.json`
- `build_meta.json`（含 `realtime_profile.tick_ms/thresholds/overrun_count/p99_exec_us`）

