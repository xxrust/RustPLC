# CI Runbook

本地复现 CI 门禁的完整步骤，以及 CI 失败时的排查指南。

---

## CI 运行的任务

工作流：`.github/workflows/rp2040_regression.yml`

| Job | 命令 | 说明 |
|-----|------|------|
| `workspace-test` | `cargo test --workspace` | 全量测试（831 个用例） |
| `topology-perf-gate` | `python3 scripts/topology_perf_gate.py --output human` | 500 节点/2000 边性能门禁 |
| `rp2040-cross-build` | `cargo build -p board-rp2040 --target thumbv6m-none-eabi --release` | RP2040 交叉编译 |
| `trace-gate` | trace-diff + PIL gates | 轨迹对比门禁 |
| `pil-renode-runner` | PIL baseline suite + Renode | Renode 仿真回归 |

---

## 本地复现

```bash
# 1. 全量测试
cargo test --workspace

# 2. 拓扑性能门禁
python3 scripts/topology_perf_gate.py --output human

# 3. RP2040 交叉编译
rustup target add thumbv6m-none-eabi
cargo build -p board-rp2040 --target thumbv6m-none-eabi --release

# 4. 轨迹对比门禁
cargo run --release -- trace-diff \
  --sil examples/trace_golden/sil_trace.jsonl \
  --board examples/trace_golden/board_trace_match.jsonl \
  --out out/ci_trace_match_report.json \
  --fail-on-mismatch

# 5. PIL 门禁
scripts/pil_trace_gate.sh \
  --sil examples/trace_golden/sil_trace.jsonl \
  --out-dir out/ci_pil_gate \
  --board-log examples/trace_golden/board_log_match.log

# 6. PIL baseline suite（cat runner，无需硬件）
scripts/pil_trace_baseline_suite.sh \
  --runner cat --out-root out/ci_pil_baselines

# 7. PIL 语义 baseline
scripts/pil_semantic_baseline.sh \
  --cases-dir examples/pil_baselines \
  --out-root out/ci_pil_semantic_baselines

# 8. Renode runner（需要 Renode）
scripts/pil_trace_baseline_suite.sh \
  --runner renode --out-root out/ci_pil_baselines_renode

# 9. Renode STM32 固件构建
rustup target add thumbv7em-none-eabi
cargo run --release -- build-renode-stm32 \
  examples/pil_baselines/case_timeout/case.plc \
  --scenario examples/pil_baselines/case_timeout/scenarios/base.yaml \
  --out out/ci_renode_f4

# 10. Renode 固件运行
scripts/renode/run_firmware_trace.sh \
  --elf out/ci_renode_f4/board-renode-stm32.elf
```

---

## 常见失败排查

### workspace-test 失败

```bash
# 只跑失败的测试
cargo test <test_name> -- --nocapture
```

### 运动回归失败

```bash
# 快速复现
cargo test -p rust_plc --test rp2040_motion_minimal_scenarios

# 检查场景一致性
cargo run --release -- scenario-validate examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/normal.yaml
```

检查点：
- .plc 和场景 YAML 是否同步
- 故障场景的 trace 中是否有 `reason == "timeout"` 的转换

### rp2040-cross-build 失败

| 症状 | 解决 |
|------|------|
| Missing target | `rustup target add thumbv6m-none-eabi` |
| Linker script errors | 检查 `.cargo/config.toml` 和 `link.x`/`defmt.x` |
| `-D warnings` 触发 | 保持 `board-rp2040` 无警告 |

### topology-perf-gate 失败

阈值配置：`scripts/perf/topology_perf_thresholds.json`

| 路径 | p95 阈值 |
|------|----------|
| `parse_validate` | 250 ms |
| `compile_simulate` | 400 ms |
| `render_transform` | 80 ms |

### Renode runner 失败

- 删除 `out/tools/renode/` 强制重新下载
- 确认 `python3`、`tar` 和 HTTPS 出站可用

### Renode 固件 trace 检查

UART 输出中应包含：
- `TICK ...` 行（tick 执行）
- `TRACE ...` 行（步骤转换）
- `TIMING ...` 行（每 tick 时序）
- 正常场景不应出现 `ERROR stage=...` 行

---

## 其他 CI 工作流

| 工作流 | 说明 |
|--------|------|
| `openplc_trace_phase2.yml` | OpenPLC trace 测试 |
| `rp2040_hil_nightly.yml` | RP2040 硬件在环夜间测试 |
| `st_codegen_matiec.yml` | ST 代码生成 + matiec 验证 |
