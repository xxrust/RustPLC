# Developer Bootstrap Pack（`rust_plc new`）

日期：2026-02-19

## 1. 目标

通过一条命令生成可直接跑通的项目骨架（含 VS Code Day-1 支持包）：

- PLC 示例工程
- 场景文件
- `io_map.toml`
- CI baseline（no-board gate）
- VS Code Day-1 支持（语法高亮策略 + 代码片段 + 常用任务 + 推荐扩展）

## 2. 命令

```bash
cargo run --release -- new my_plc_project
```

可选：

```bash
cargo run --release -- new my_plc_project --force
```

## 3. 生成内容

- `README.md`（从 0 到 first gate pass 的 checklist）
- `plc/main.plc`
- `scenarios/normal.yaml`
- `io_map.toml`
- `.github/workflows/no_board_gate.yml`
- `.vscode/tasks.json`
- `.vscode/settings.json`
- `.vscode/extensions.json`
- `.vscode/plc.code-snippets`
- `.vscode/README.md`

## 4. VS Code 支持包契约（Day-1）

### 4.1 高亮策略

- `*.plc` 通过 `.vscode/settings.json` 关联到 `ini` 模式（轻量 fallback 策略）

### 4.2 命令任务

`.vscode/tasks.json` 默认包含：

- `RustPLC: scenario-init (normal)`
- `RustPLC: scenario-validate`
- `RustPLC: scenario-doctor`
- `RustPLC: sim-plc`
- `RustPLC: no-board-gate`
- `RustPLC: build-rp2040`

### 4.3 片段支持

`.vscode/plc.code-snippets` 默认包含：

- `plc-skeleton`（完整 PLC 文件骨架）
- `plc-wait-timeout`（wait + timeout 常用片段）

### 4.4 推荐扩展

`.vscode/extensions.json` 默认推荐：

- `rust-lang.rust-analyzer`
- `redhat.vscode-yaml`
- `tamasfe.even-better-toml`
- `streetsidesoftware.code-spell-checker`

## 5. Onboarding Checklist（零到首个 gate）

1. 场景校验：

```bash
cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output human
```

2. no-board gate：

```bash
cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output human
```

3. 诊断预检查（建议）：

```bash
cargo run --release -- scenario-doctor plc/main.plc --scenario scenarios/normal.yaml --output human
```

4. 可选 RP2040 构建：

```bash
cargo run --release -- build-rp2040 plc/main.plc --out out/rp2040 --io-map io_map.toml
```

## 6. 常见问题（Troubleshooting）

1. VS Code 里看不到 snippet：
   - 确认文件后缀为 `*.plc`
   - 执行 `Developer: Reload Window`
2. 任务执行报 `cargo` 不存在：
   - 确认终端 PATH 可找到 cargo
   - 从项目根目录打开 VS Code
3. YAML/TOML 没有诊断：
   - 安装 `.vscode/extensions.json` 中推荐扩展
