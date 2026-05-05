# VS Code Day-1 Support for RustPLC

## What this package provides

- `settings.json`: associates `*.plc` with INI highlighting
- `plc.code-snippets`: starter snippets for PLC skeletons
- `tasks.json`: one-click project-check, sim, and gate commands
- `extensions.json`: recommended Rust/YAML/TOML extensions

## Troubleshooting

1. If snippets do not appear, confirm the file is `*.plc` and reload the window.
2. If tasks fail with `command not found`, run them from the workspace root with `cargo` on PATH.
3. If YAML/TOML diagnostics are missing, install the recommended extensions.
