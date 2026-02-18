# RP2040 HIL 回归（自托管 Runner）

这份文档把“真实 RP2040 板卡 + 自动对比门禁”固化为可重复流程：

- 固件：由同一份 `.plc` 生成并烧录
- 输入：按 `scenario.yaml` 驱动（SIL 与板级使用同一份场景）
- 观测：采集板级 `board.log`（RTT 或串口转存为文本）
- 对比：`board-parse` + `trace-diff --fail-on-mismatch`
- 产物：`diff_report.json` + `trace_diff_dashboard.html` + (可选) `hil_bundle.tgz`

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

## 2) 一条命令跑 HIL gate

仓库提供脚本：`scripts/rp2040_hil_gate.sh`

推荐用 end-to-end 示例（包含 `.plc / io_map / scenario`）：

```bash
scripts/rp2040_hil_gate.sh \
  --plc examples/rp2040_end_to_end/pressure_station.plc \
  --scenario examples/rp2040_end_to_end/scenarios/normal.yaml \
  --io-map examples/rp2040_end_to_end/io_map.toml \
  --mount /media/RPI-RP2 \
  --port /dev/ttyACM0 \
  --baud 115200 \
  --duration 20 \
  --out-dir out/rp2040_hil_gate \
  --bundle
```

输出目录包含：

- `sil_trace.jsonl`：SIL 轨迹
- `firmware.uf2` / `rp2040/`：构建产物
- `board.log`：板级日志
- `board_trace.jsonl`：板级 trace
- `diff_report.json`：对比报告（首个偏差 tick + 上下文）
- `trace_diff_dashboard.html`：可直接打开的 HTML 报告
- `hil_meta.json`：本次 gate 的元信息（commit/时间/输入参数）
- `hil_bundle.tgz`：可上传/留存的归档包（如果用了 `--bundle`）

## 3) 报告查看

优先看：

- `out/.../trace_diff_dashboard.html`
- 或用静态 Viewer：`tools/trace_viewer/index.html`（加载 `diff_report.json`）

## 4) GitHub Actions（可选）

如果你有一台长期在线且连着 Pico 的机器，可以安装 GitHub self-hosted runner，并在该机器上运行 HIL workflow：

- 工作流建议 `runs-on: [self-hosted, rp2040-hil]`
- 由 `workflow_dispatch` 手工触发或 schedule 定时触发

注意：公共 CI（`ubuntu-latest`）不具备真实硬件，因此只能跑 PIL/SIL 基线，不能替代 HIL。
