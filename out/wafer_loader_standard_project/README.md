# 测角机上料器标准项目

- Project slug: `wafer_loader_standard_project`
- Manifest: `rustplc.project.toml`
- Layout: `phased bundle (v2)`
- Delivery layer: `station` (independent station)

## Purpose

本项目是从 `plc/main.system.md` 正向生成的标准 RustPLC scaffold，用于表达测角机上料器的单站控制语义。
核心链路是：

```text
plc/main.system.md
  -> process_model/process_operation_model.toml
  -> 00_topology/ + 01_init/ + 02_process/ + 03_constraints/ + 04_faults/
  -> rustplc.bundle.toml
```

## Structure

- `00_topology/`: device declarations, workpieces, connections
- `process_model/`: authored process operation scheduling intent
- `01_init/`: initialization and startup tasks
- `02_process/`: automatic production cycle
- `03_constraints/`: safety and timing rules
- `04_faults/`: fault handling tasks
- `05_supervision/`: reserved disabled mode-management layer for supervisor/front-door logic
- `06_manual/`: reserved disabled manual-maintenance layer
- `07_hmi/`: reserved disabled HMI layer
- `config/`: deployment configuration
- `scenarios/`: test scenarios
- `docs/`: project documentation

## Quick Start

```bash
cargo run --release --bin rust_plc -- project-check out/wafer_loader_standard_project/rustplc.bundle.toml --scenario out/wafer_loader_standard_project/scenarios/nominal/normal.yaml --out-dir out/wafer_loader_standard_project/out/check --require-process-model --output human
```
