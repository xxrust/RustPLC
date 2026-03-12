# Developer Bootstrap Pack（`rust_plc new`）

日期：2026-03-12

## 1. 目标

通过一条命令生成一个像“完整 PLC 项目”而不是“单文件 demo”的工程骨架。

核心要求：

- `.system.md` 与 `.plc` 成对存在
- 项目名从 `new <project_dir>` 自动注入 `README` / `main.system.md` / `rustplc.project.toml`
- 场景、I/O 配置与 DSL 分层
- 所有派生产物统一进入 `out/`
- 提供 VS Code Day-1 支持与 CI baseline

## 2. 命令

```bash
cargo run --release -- new my_plc_project
```

可选：

```bash
cargo run --release -- new my_plc_project --force
```

## 3. 生成目录

```text
my_plc_project/
├── README.md
├── .gitignore
├── rustplc.project.toml
├── plc/
│   ├── main.system.md
│   └── main.plc
├── scenarios/
│   ├── nominal/normal.yaml
│   ├── faults/
│   └── generated/
├── config/
│   ├── io_map.toml
│   └── retain.toml
├── docs/
│   └── project-layout.md
├── out/
│   ├── ir/
│   ├── sim/
│   ├── gate/
│   ├── codegen/
│   ├── rp2040/
│   └── release/
├── .github/workflows/no_board_gate.yml
└── .vscode/
```

## 4. 目录职责

- `plc/`：项目语义源；`main.system.md` 与 `main.plc` 同目录、同 basename
- `rustplc.project.toml`：项目清单；固定项目名、主入口、默认 scenario 与输出路径
- `scenarios/`：版本化场景；按 `nominal / faults / generated` 分层
- `config/`：部署配置与运行配置
- `docs/`：项目自己的说明文档
- `out/`：所有可重建的中间物、仿真物、门禁产物、代码生成物、板级构建物、发布物

## 5. VS Code 支持包契约（Day-1）

### 5.1 高亮策略

- `*.plc` 通过 `.vscode/settings.json` 关联到 `ini` 模式

### 5.2 命令任务

`.vscode/tasks.json` 默认包含：

- `RustPLC: scenario-init (normal)`
- `RustPLC: scenario-validate`
- `RustPLC: scenario-doctor`
- `RustPLC: sim-plc`
- `RustPLC: no-board-gate`
- `RustPLC: gen-st`
- `RustPLC: build-rp2040`

### 5.3 片段支持

`.vscode/plc.code-snippets` 默认包含：

- `plc-skeleton`
- `plc-wait-timeout`

### 5.4 推荐扩展

`.vscode/extensions.json` 默认推荐：

- `rust-lang.rust-analyzer`
- `redhat.vscode-yaml`
- `tamasfe.even-better-toml`
- `streetsidesoftware.code-spell-checker`

## 6. Onboarding Checklist（零到首个 gate）

1. 场景校验：

```bash
cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

2. no-board gate：

```bash
cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human
```

3. 诊断预检查：

```bash
cargo run --release -- scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

4. 可选 ST 生成：

```bash
cargo run --release -- gen-st plc/main.plc --out out/codegen/st/main.st
```

5. 可选 RP2040 构建：

```bash
cargo run --release -- build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```

## 7. Troubleshooting

1. VS Code 里看不到 snippet：
   - 确认文件后缀为 `*.plc`
   - 执行 `Developer: Reload Window`
2. 任务执行报 `cargo` 不存在：
   - 确认终端 PATH 可找到 cargo
   - 从项目根目录打开 VS Code
3. `out/` 里产物越来越多：
   - 保留 `plc/`、`scenarios/`、`config/`、`docs/`
   - 清理 `out/` 不会破坏源码资产
