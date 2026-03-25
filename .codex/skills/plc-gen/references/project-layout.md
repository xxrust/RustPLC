# plc-gen Project Layout

Use this file when the caller asks which files they should edit after scaffolding.

## Scaffold Command

```bash
<run> new my_plc_project
```

## Files the Caller Should Care About

- `plc/main.system.md`
  confirmed system contract
- `plc/main.plc`
  executable RustPLC DSL
- `scenarios/nominal/normal.yaml`
  nominal validation scenario already created by the scaffold
- `config/io_map.toml`
  deployment I/O mapping
- `config/retain.toml`
  retain baseline
- `rustplc.project.toml`
  manifest and default path contract

## Edit Order

For a fresh project:

1. confirm `plc/main.system.md`
2. write or repair `plc/main.plc`
3. update `scenarios/nominal/normal.yaml`
4. run validation commands
5. only then produce codegen or deployment artifacts

## Output Folders

Treat these as generated artifacts:

- `out/ir/`
- `out/sim/`
- `out/gate/`
- `out/codegen/`
- `out/rp2040/`
- `out/release/`
