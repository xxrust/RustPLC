# RP2040 HIL 回归（自托管 Runner）

这份文档把“真实 RP2040 板卡 + 自动对比门禁”固化为可重复流程：

- 固件：由同一份 `.plc` 生成并烧录
- 输入：按 `scenario.yaml` 驱动（SIL 与板级使用同一份场景）
- 观测：采集板级 `board.log`（RTT 或串口转存为文本）
- 对比：`board-parse` + `trace-diff --fail-on-mismatch`
- 断言：按 case bundle 校验关键事件（axis/step/signal/tick）

## 1) 准备一台自托管 Runner

推荐：Linux 小主机 / 工控机 / 树莓派（只要能 USB 连接 Pico）。

需要：

- Rust toolchain（含 `thumbv6m-none-eabi` target）
- `elf2uf2-rs`（ELF -> UF2）
- Pico 可被识别为：
  - Mass Storage（`/media/RPI-RP2` 类挂载点），用于 UF2 拷贝烧录
  - 串口设备（如 `/dev/ttyACM0`），用于日志采集（如果你走串口）

串口权限（示例）：

```bash
sudo usermod -a -G dialout $USER
```

## 2) 本地一条命令（daily gate，含 motion + fail-safe）

仓库提供统一入口：`scripts/rp2040_hil_daily_gate.sh`

```bash
scripts/rp2040_hil_daily_gate.sh \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --baud 115200 \
  --duration 20 \
  --out-root out/rp2040_hil_daily_gate \
  --bundle
```

默认 case bundles（`scenarios/rp2040_hil_gate/cases.json`）：

- `motion_nominal`（motion-focused）
- `fail_safe_axis0_count_timeout`（fail-safe-focused）

输出目录结构：

- `out/.../<case-id>/hil_summary.json`：单 case gate 结果
- `out/.../<case-id>/diff_report.json`：SIL vs Board 对比
- `out/.../<case-id>/timing_report.json`：板级 tick 执行统计（p50/p95/p99/max/overrun）
- `out/.../<case-id>/timing_gate_verdict.json`：实时阈值门禁结论（阈值、观测值、违规项）
- `out/.../<case-id>/assertions_report.json`：断言检查，包含 `axis/signal/step/tick`
- `out/.../hil_daily_summary.json`：全量汇总（是否整体通过 + 各 case 详情）

当断言失败时，`assertions_report.json.first_failure_context` 会给出可定位字段：

- `axis`（例如 `axis0`）
- `signal`（例如 `axis0_count`）
- `expected.step`（`task/from_step/to_step/reason`）
- `observed.tick`（触发时间）

## 3) CI 复现命令（与 nightly workflow 一致）

`.github/workflows/rp2040_hil_nightly.yml` 中执行的命令如下，可在自托管机本地直接复现：

```bash
scripts/rp2040_hil_daily_gate.sh \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --duration 20 \
  --out-root out/ci_hil_daily_gate \
  --bundle
```

## 4) 单 case 调试（可选）

若只想调试某个 case，可直接调用 `scripts/rp2040_hil_gate.sh`：

```bash
scripts/rp2040_hil_gate.sh \
  --plc examples/rp2040_motion_minimal.plc \
  --scenario scenarios/rp2040_motion_minimal/count_stuck.yaml \
  --io-map examples/rp2040_motion_minimal.io_map.toml \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --out-dir out/rp2040_hil_single_case \
  --bundle
```

## 5) 实时阈值门禁（建议纳入 nightly）

可以在 daily gate 或单 case gate 上增加阈值：

```bash
scripts/rp2040_hil_daily_gate.sh \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --max-p99-exec-us 2000 \
  --max-overrun-count 0 \
  --out-root out/rp2040_hil_daily_gate \
  --bundle
```

判定规则：

- `exec_us_p99 <= max_p99_exec_us`
- `overrun_count <= max_overrun_count`
- 任一超限即 case 失败，并在 `timing_gate_verdict.json.violations` 给出超限指标

阈值调优建议（避免一开始就过严）：

1. 先连续采集 3~5 次无故障基线，记录每次 `timing_report.json.exec_us_p99`。
2. 初始阈值取“基线最大 p99 的 1.2~1.5 倍”。
3. `max_overrun_count` 建议先设 `0`，如现场噪声较高可短期放宽并记录原因。
4. 每次改阈值都要在评审记录中附上对应工件（`timing_report.json` + `timing_gate_verdict.json`）。

## 6) 异常退出分级矩阵（A/B/C/D）

异常退出矩阵与证据模板位于：

- `scenarios/rp2040_hil_gate/abnormal_exit/matrix.json`
- `scenarios/rp2040_hil_gate/abnormal_exit/evidence/*.json`

自动化验证（A/B/C）：

```bash
python3 scripts/abnormal_exit_matrix_verify.py \
  --matrix scenarios/rp2040_hil_gate/abnormal_exit/matrix.json \
  --evidence-dir scenarios/rp2040_hil_gate/abnormal_exit/evidence \
  --out out/rp2040_hil_daily_gate/abnormal_exit_report.json
```

说明：`D` 类（`kill9/power_loss/kernel_hang`）属于 `hardware_only`，
依赖独立硬件安全链电气实测，不纳入自动通过条件。详见：`docs/abnormal_exit_matrix.md`。

## 7) 报告查看

优先看：

- `out/.../<case-id>/trace_diff_dashboard.html`
- 或静态 Viewer：`tools/trace_viewer/index.html`（加载 `diff_report.json`）

## 8) GitHub Actions 说明

如果你有一台长期在线且连着 Pico 的机器，可以安装 GitHub self-hosted runner，并在该机器上运行 HIL workflow：

- 工作流建议 `runs-on: [self-hosted, linux]`
- 由 `workflow_dispatch` 手工触发或 schedule 定时触发

注意：公共 CI（`ubuntu-latest`）不具备真实硬件，因此只能跑 PIL/SIL 基线，不能替代 HIL。
