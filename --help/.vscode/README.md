# VS Code Day-1 Support for RustPLC

## What this package provides

- `settings.json`: associates `*.plc` with INI highlighting (fallback strategy)
- `plc.code-snippets`: starter snippets for skeletons and wait/timeout patterns
- `tasks.json`: one-click commands for scenario-init/doctor/sim/gate/gen-st/build
- `extensions.json`: recommended extensions for Rust/YAML/TOML/spell-check

## Highlight strategy

RustPLC currently uses a lightweight no-extension strategy in scaffold projects:

- `*.plc` -> `ini` language mode
- snippets + tasks provide practical editing/iteration support

## Troubleshooting

1. If snippets do not appear:
   - confirm file is `*.plc`
   - run `Developer: Reload Window`
2. If tasks fail with "command not found":
   - ensure `cargo` is on PATH
   - run tasks from workspace root
3. If YAML/TOML diagnostics are missing:
   - install recommended extensions from `.vscode/extensions.json`
