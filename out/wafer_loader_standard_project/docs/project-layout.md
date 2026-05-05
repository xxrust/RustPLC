# Project Layout

This scaffold uses the standard RustPLC project layout.

- `rustplc.project.toml`: project manifest and default artifact paths
- `plc/main.system.md`: human/AI confirmed system intent
- `process_model/process_operation_model.toml`: authored operation scheduling intent, before task/step
- `rustplc.bundle.toml`: executable RustPLC source entry
- `scenarios/nominal/normal.yaml`: nominal regression scenario
- `config/`: I/O, retain, and workpiece configuration
- `out/`: rebuildable generated artifacts

## Layer Semantics

- `00_topology/`: device declarations, workpieces, connections, station protocol.
- `process_model/`: source-side operation scheduling intent, written before task/step flow.
- `01_init/`: initialization baseline and safe state.
- `02_process/`: automatic production tasks that execute admitted process operations.
- `03_constraints/`: safety, timing, and resource rules.
- `04_faults/`: abnormal-path convergence and recovery.
- `05_supervision/`: reserved disabled supervisor/front-door layer.
- `06_manual/`: reserved disabled manual-maintenance layer.
- `07_hmi/`: reserved disabled HMI layer.

`supervisor` means operator command acceptance, auto-cycle latching, mode arbitration, and safe stop/return-to-baseline logic. It is not a process device and should not be mixed into the station production tasks.

Current project: `wafer_loader_standard_project` / `Wafer Loader Standard Project`

Recommended commands:

```bash
cargo run --release --bin rust_plc -- process-model-check \
  rustplc.bundle.toml --model process_model/process_operation_model.toml --output human

cargo run --release --bin rust_plc -- scenario-validate \
  rustplc.bundle.toml --scenario scenarios/nominal/normal.yaml --output human

cargo run --release --bin rust_plc -- sim-plc \
  rustplc.bundle.toml --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl

cargo run --release --bin rust_plc -- no-board-gate \
  rustplc.bundle.toml --scenario scenarios/nominal/normal.yaml \
  --out-dir out/gate/no_board/normal --output human

cargo run --release --bin rust_plc -- gen-st \
  rustplc.bundle.toml --out out/codegen/st/main.st
```
