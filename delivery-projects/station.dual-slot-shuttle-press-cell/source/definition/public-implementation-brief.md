# Public Implementation Brief

Implement the station defined by `main.system.md` as a structured RustPLC project under the run's `specimen/implementation/` directory.

## Mandatory Order

1. Scaffold structured fragments.
2. Replace every starter placeholder.
3. Author `process_model/process_operation_model.toml` from `process-operation-intent.md`.
4. Author topology/workpiece/carrier/controller aliases.
5. Author startup, process, constraints, faults, supervision, manual, and HMI fragments.
6. Author required scenarios.
7. Run compile and targeted checks; repair concrete diagnostics.
8. Run nominal simulation to produce a real trace.
9. Use trace or `intent-doctor` to choose supported business anchors.
10. Author the sibling intent contract with a real digest.
11. Run `process-model-check` and `project-check` with explicit intent contract/evidence arguments if CLI help requires them.

## Implementation Constraints

- Use current public CLI help and `dsl-capabilities` when syntax is uncertain.
- Do not read definition-agent private logs.
- Do not copy an existing complete generated project wholesale.
- Do not use internal booleans initialized to true as physical proof.
- Do not hand-write cylinder endpoint waits for normal action completion.
- Do not use `allow_indefinite_wait` for local position, presence, or cylinder feedback.
- Keep the nominal flow finite at two seeded parts unless explicit replenishment is modeled.
- Add a meaningful Chinese comment immediately before every task and step.

## Required Authored Artifacts

- `rustplc.bundle.toml`
- fragments `00_topology` through `07_hmi`
- `process_model/process_operation_model.toml`
- `plc/main.system.md` or an equivalent bound authored copy
- station architecture and verification docs
- six scenario files named in `scenario-requirements.md`
- `rustplc.bundle.intent_alignment.contract.json`

## Required Execution Record

Maintain `logs/agent-b-execution.md` with timestamp, command/search, elapsed time, exit code, failure, retry count, route change, and missing-public-information notes. Stop repeating the same failing route after three attempts.
