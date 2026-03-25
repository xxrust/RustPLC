# Pain Points

Task:
Explain how to scaffold a RustPLC project named demo_line and list the first three validation commands, using only public artifacts.

## Result

The blind operator completed the task from the public bundle alone.

- Scaffold command derived from public artifacts: `rust_plc new demo_line`
- Source-workspace variant also derived: `cargo run --release --bin rust_plc -- new out/demo_line`
- Validation commands returned by the blind operator:
  - `rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human`
  - `rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human`
  - `rust_plc no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human`

## Pain Points

1. Step:
   Observed blocker: No explicit blocker reported by the blind operator.
   Missing artifact or instruction: None reported.
   Impact: The task was solvable, but this alone did not prove the public surface was internally consistent.

2. Step:
   Observed blocker: Public artifacts were sufficient for a strong agent to assemble an answer, but the cycle still required a source-aware review to detect conflicting public guidance.
   Missing artifact or instruction: A single canonical public command matrix for scaffold creation and first validation checks.
   Impact: Future blind passes may succeed inconsistently or produce divergent answers depending on which public artifact they reach first.
