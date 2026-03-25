# RustPLC Project Layout

Use this file when the caller is working from a scaffolded project or asks where each artifact belongs.

## Scaffold Command

```bash
<run> new my_plc_project
```

The scaffold creates a fixed project contract.

## Files the Caller Should Care About

- `rustplc.project.toml`
  Project manifest and default path contract.
- `plc/main.system.md`
  Human or AI confirmed system intent.
- `plc/main.plc`
  Executable RustPLC DSL.
- `scenarios/nominal/normal.yaml`
  Nominal regression scenario.
- `config/io_map.toml`
  Deployment I/O mapping.
- `config/retain.toml`
  Retain and persistence baseline.
- `docs/project-layout.md`
  Local explanation of the generated layout.

## Generated Output Folders

These are build artifacts.
Do not hand-edit them unless the specific workflow calls for it.

- `out/ir/`
- `out/sim/`
- `out/gate/`
- `out/codegen/`
- `out/rp2040/`
- `out/release/`

## Edit Order

For a fresh customer project, work in this order:

1. confirm the system contract in `plc/main.system.md`
2. write or repair `plc/main.plc`
3. update `scenarios/nominal/normal.yaml`
4. run validation commands
5. only then produce codegen or deployment artifacts

## Day-1 Expectations

Tell the caller exactly which files to touch first:

- if the process description is still fuzzy, start in `plc/main.system.md`
- if the logic already exists, start in `plc/main.plc`
- if validation fails because of timing or inputs, inspect `scenarios/nominal/normal.yaml`
- if deployment fails, inspect `config/io_map.toml`

## VS Code Notes

The scaffold also generates:

- `.vscode/tasks.json`
- `.vscode/settings.json`
- `.vscode/extensions.json`
- `.vscode/plc.code-snippets`
- `.vscode/README.md`

Those files are convenience tooling.
They are not the source of truth for PLC logic.
