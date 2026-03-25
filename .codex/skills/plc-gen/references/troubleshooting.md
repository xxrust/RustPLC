# plc-gen Troubleshooting

Use this file when the caller gets stuck before actual PLC generation.

## Problem: `cargo run --release -- new ...` Fails

Cause:

- the workspace has multiple binaries

Fix:

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

## Problem: Top-Level `--help` Is Not Usable

Cause:

- the CLI currently does not expose a generic top-level help screen

Fix:

- do not ask the caller to discover the interface with top-level help
- give the exact subcommand syntax from `references/commands.md`

## Problem: The Caller Does Not Have Source Code

Fix:

- switch commands from `cargo run --release --bin rust_plc -- ...` to `rust_plc ...`
- keep the rest of the command line the same

## Problem: No Scenario Exists Yet

Fix:

```bash
<run> scenario-init plc/main.plc --out scenarios/nominal/normal.yaml --preset normal
```

Then run:

```bash
<run> scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

If the project came from `new`, first check whether `scenarios/nominal/normal.yaml` already exists before recommending regeneration.
