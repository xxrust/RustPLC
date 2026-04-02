# Project Layout

这个脚手架采用固定的 RustPLC 项目目录约定：

- `rustplc.project.toml`：项目清单，声明主入口与默认路径
- `plc/`：系统语义与 DSL 源码
- `scenarios/`：版本化场景输入
- `config/`：I/O 与运行配置
- `out/`：所有可重建产物

当前项目：`--help` / `Help`

推荐命令：

```bash
cargo run --release --bin rust_plc -- scenario-validate \
  plc/main.plc --scenario scenarios/nominal/normal.yaml --output human

cargo run --release --bin rust_plc -- sim-plc \
  plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl

cargo run --release --bin rust_plc -- no-board-gate \
  plc/main.plc --scenario scenarios/nominal/normal.yaml \
  --out-dir out/gate/no_board/normal --output human

cargo run --release --bin rust_plc -- gen-st \
  plc/main.plc --out out/codegen/st/main.st

cargo run --release --bin rust_plc -- build-rp2040 \
  plc/main.plc --out out/rp2040 --io-map config/io_map.toml
```
