# Scaffold Day-1 Launchers

先只判断一件事：用户是在用已安装好的 `rust_plc`，还是仍在 RustPLC 源码仓里用 `cargo run`。

复杂项目默认路径不是单文件 `plc/main.plc`，而是 structured-fragments + delivery asset `main.bundle.toml` + `project-check`。

## 1. 已安装 `rust_plc`

对新 station / line / module 项目，优先：

```bash
rust_plc new my_station --layout structured-fragments --delivery-layer station
cd my_station
rust_plc project-check plc/deliveries/station/my_station/plc/main.bundle.toml --scenario plc/deliveries/station/my_station/scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
```

如果只是小范围单文件 repair，才走：

```bash
rust_plc project-check plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/project_check/normal --output human
```

## 2. RustPLC 源码仓 workspace

必须一直留在 RustPLC 仓库根目录执行：

```bash
cargo run --release --bin rust_plc -- new out/my_station --layout structured-fragments --delivery-layer station
cargo run --release --bin rust_plc -- project-check out/my_station/plc/deliveries/station/my_station/plc/main.bundle.toml --scenario out/my_station/plc/deliveries/station/my_station/scenarios/nominal/normal.yaml --out-dir out/my_station/out/project_check/normal --output human
```

如果只是现有单文件 source entry 的局部修复，才走：

```bash
cargo run --release --bin rust_plc -- project-check out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/project_check/normal --output human
```

## 3. 复杂项目的入口提醒

- root `plc/main.system.md` 与 `plc/main.target_semantics.bundle.toml` 可以作为项目级入口或索引
- 对新 scaffold 的 complex delivery，真正要 authoring 和验证的是 delivery asset 下的 `docs/`、`plc/main.bundle.toml`、`scenarios/nominal/normal.yaml`
- 不要把“scaffold 创建成功”误当成“confirmed `.system.md` 已经变成真实交付”

不要写成：

- `cargo run --release -- ...`
- 先 `cd` 进 scaffold 目录后再跑 `cargo run --release --bin rust_plc -- ...`

因为这个 workspace 有多个 binary，而 scaffold 本身不是 Cargo 项目。
