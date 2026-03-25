# RustPLC Troubleshooting

Use this file when the caller gets stuck before the real PLC work starts.

## Problem: `cargo run --release -- new ...` Fails

Cause:

- the workspace has multiple binaries

Fix:

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

Apply the same `--bin rust_plc` rule to every source-workspace command.

## Problem: Top-Level `--help` Does Not Work

Cause:

- the CLI currently expects a `.plc` path at the top level instead of providing a generic help screen

Fix:

- give the caller the exact subcommand syntax from `references/commands.md`
- do not tell the caller to discover the interface via top-level help

## Problem: The Caller Does Not Have Source Code

Cause:

- product users may have an installed `rust_plc` binary instead of the repository

Fix:

- switch commands from `cargo run --release --bin rust_plc -- ...` to `rust_plc ...`
- keep the rest of the command line the same

## Problem: No Scenario File Exists Yet

Fix:

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

Then run:

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

## Problem: Validation Fails Before Gate or Build

Fix order:

1. `scenario-validate`
2. `scenario-doctor`
3. `no-board-gate`
4. `gen-st`, `build-rp2040`, or `release-bundle`

Do not jump straight to deployment commands before the validation path is clean.

## Problem: The Project Directory Already Exists

Fix:

```bash
<run> new my_plc_project --force
```

Use `--force` only when overwrite is intentional.

## Problem: The User Is Overwhelmed by Internal Concepts

Fix:

- do not start with parser, IR, runtime, or verification internals
- start with scaffold, three editable files, and the validation loop
- introduce deeper architecture only when the caller asks
