# Root Cause

Task:
Explain how to scaffold a RustPLC project named demo_line and list the first three validation commands, using only public artifacts.

## Findings

1. Pain point:
   Classification: public-surface-gap
   Why: Generated scaffold documentation teaches `cargo run --release --bin rust_plc -- ...` in a way that implies scaffold-local execution, while the public skill guidance correctly says the scaffold is not a Cargo project and source-workspace users must stay at the RustPLC repo root with full scaffold paths.
   Minimal fix: Normalize scaffold-facing docs and generated help output so repo-root execution context and full-path examples are the only public recommendation for source-workspace users.

2. Pain point:
   Classification: public-surface-gap
   Why: Public artifacts disagree on the first three validation commands and their order.
   Minimal fix: Publish one canonical "first three checks" sequence and update the scaffold README, help bundle, and top-level README to match it.

3. Pain point:
   Classification: public-surface-gap
   Why: Launcher guidance drifts between `cargo run --release -- ...` and `cargo run --release --bin rust_plc -- ...`, while the skill already contains the safer rule.
   Minimal fix: Expose one canonical launcher rule in outward-facing docs and generated scaffold materials.

4. Pain point:
   Classification: public-surface-gap
   Why: Creation guidance is fragmented across multiple public artifacts, forcing the blind operator to reconstruct a single workflow from several documents.
   Minimal fix: Export one scaffold quickstart artifact that covers create, edit, and first validation steps in one place.

5. Pain point:
   Classification: no skill-gap found in this cycle
   Why: `plc-gen` already carries the safer repo-root launcher rule and the `scenario-validate` -> `scenario-doctor` -> `no-board-gate` sequence; the inconsistencies live in outward-facing public docs instead.
   Minimal fix: Change public artifacts first. Re-run the cycle before modifying the skill.
