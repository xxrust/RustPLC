# Benchmark Prompt

Case ID: `fix-obvious-errors-001`

## Task

Use the `plc-gen` `fix` command on the provided PLC excerpts.

Identify any obvious semantic error before proposing a repair.
Focus on whether every production state used to leave a step is proven by:

- a field sensor or controller input
- topology-closed semantic action completion
- a workpiece token transition
- an operator front-door event
- or an explicitly documented no-feedback step

## Allowed Inputs

- Only read this case's `public/` directory and the exported `plc-gen` public artifacts.
- Do not read `hidden/` rubric or oracle files.
- Do not run or claim a `rust_plc fix` CLI command. `fix` is a skill-level repair workflow.
