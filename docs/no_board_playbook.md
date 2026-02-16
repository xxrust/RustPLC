# No-Board Playbook（无开发板交付流程）

本 Playbook 目标：在没有 RP2040 实体板的情况下，仍可从 `.plc` 产出可交付工件，并通过虚拟板级门禁把回归风险显式化。

最小流程：

1. compile/verify（生成结构化验证报告）
2. sim（SIL 生成 trace）
3. virtual-board（host 方式生成 board.log + board_trace.jsonl）
4. trace-diff（SIL vs virtual-board 对比门禁）
5. release-bundle（打包可追溯交付物）

## 0. 前置

- 假设当前工作目录为仓库根目录
- 示例使用 `examples/two_cylinder.plc`
- 示例场景文件使用你自己的 `scenario.yaml`（或参考 `examples/rp2040_end_to_end/scenarios/normal.yaml`）

以下命令均可用 `cargo run --release -- ...` 或直接运行已构建的二进制 `target/release/rust_plc`。

## 1. compile/verify

```bash
cargo run --release -- examples/two_cylinder.plc \
  --report out/two_cylinder.verification_report.json \
  --deny-warnings
```

输入：
- `.plc` 源文件

输出：
- `out/two_cylinder.verification_report.json`
  - 结构化验证报告（safety/liveness/timing/causality 的 level/warnings/checked_rules 等）
- stdout：编译后的 IR JSON（用于调试/工具链对接）

## 2. sim（SIL）

```bash
cargo run --release -- sim-plc examples/two_cylinder.plc \
  --scenario examples/rp2040_end_to_end/scenarios/normal.yaml \
  --out out/no_board/sil_trace.jsonl
```

输入：
- `.plc` 源文件
- `scenario.yaml`（tick_ms/duration_ms/inputs/faults）

输出：
- `out/no_board/sil_trace.jsonl`
  - SIL JSONL 轨迹（每行一个 transition 事件）

## 3. virtual-board（host 虚拟板）

```bash
cargo run --release -- virtual-board examples/two_cylinder.plc \
  --scenario examples/rp2040_end_to_end/scenarios/normal.yaml \
  --out-dir out/no_board/virtual_board
```

输入：
- `.plc` 源文件
- `scenario.yaml`

输出目录（`--out-dir`）：
- `board.log`
  - 虚拟板日志（含 TRACE 行，格式与板端一致）
- `board_trace.jsonl`
  - 结构化板级轨迹（JSONL）
- `virtual_board_meta.json`
  - 运行元信息（tick_ms/duration 等）

可选：若你有真实板日志，可用 `trace-parse` 把 `board.log` 转成 JSONL。

## 4. trace-diff（门禁）

```bash
cargo run --release -- trace-diff \
  --sil out/no_board/sil_trace.jsonl \
  --board out/no_board/virtual_board/board_trace.jsonl \
  --out out/no_board/diff_report.json \
  --context 2 \
  --fail-on-mismatch
```

输入：
- `--sil`：SIL 轨迹 JSONL
- `--board`：板级/虚拟板轨迹 JSONL

输出：
- `diff_report.json`
  - `is_match` 是否一致
  - 首个 mismatch 的位置（tick/index/reason）
  - 上下文事件窗口（context）

提示：若你希望一条命令跑完整个门禁链路，可直接用：

```bash
cargo run --release -- no-board-gate examples/two_cylinder.plc \
  --scenario examples/rp2040_end_to_end/scenarios/normal.yaml \
  --out-dir out/no_board/gate
```

## 5. release-bundle（交付包）

```bash
cargo run --release -- release-bundle examples/two_cylinder.plc \
  --scenario examples/rp2040_end_to_end/scenarios/normal.yaml \
  --out-dir out/no_board/release
```

输入：
- `.plc` 源文件
- `scenario.yaml`

输出目录（`--out-dir`）：
- `manifest.json`
  - 每个工件的 `sha256` 与 `size_bytes`（可审计/可复现）
- `verification_report.json`
  - 结构化验证报告
- `sim_report.json` / `sil_trace.jsonl`
  - 仿真报告与轨迹
- `board.log` / `board_trace.jsonl`
  - 虚拟板日志与轨迹
- `diff_report.json`
  - 对比差异报告
- `build_meta.json`
  - build 元信息（git commit/dirty/generated_at/tool_version 等）

