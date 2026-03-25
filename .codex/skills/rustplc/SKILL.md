---
name: rustplc
description: "Use RustPLC as a productized requirement-to-artifact skill that turns equipment requirements into validated RustPLC deliverables without exposing repository internals. Trigger when the user wants RustPLC code generated from requirements, wants an existing .plc validated or repaired, or wants a scaffolded RustPLC project delivered quickly."
---

# rustplc

Use RustPLC as a product, not as a codebase tour.

Return validated deliverables quickly.
Do not proactively expose repository internals, source files, or implementation details.

## Default Deliverables

Return:
- a `.plc` program
- a short assumptions list
- a validation result

Return when useful:
- a `.system.md`
- a scaffolded project layout
- a nominal scenario

## Product Workflow

1. Read the requirement as a delivery request.
2. Ask only the smallest set of blocking questions.
3. Prefer a scaffolded project when the request is broader than a single file.
4. Generate artifacts.
5. Validate with RustPLC tooling.
6. Repair until validation passes or a real contract gap remains.

## Scaffold Preference

If the user asks for a usable project, examples, or end to end validation, start with:

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

Then fill:
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`

Prefer scaffold for:
- new projects
- smoke tests
- skill forward testing
- scenario and no-board validation

## Internal Flow

You may internally use a system-first flow:
- requirement understanding
- system semantic draft
- PLC generation
- validation

But present the result as one cohesive RustPLC service.
Do not tell normal callers to manually switch to internal repo skills.

## Validation Rule

A RustPLC result is not done when it only looks plausible.

Validate with the real tools whenever available, for example:

```bash
cargo run --release -- <file.plc> --no-print-ir
```

For scaffolded projects, prefer:

```bash
cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

Add `no-board-gate` when the request is project-level and the scenario exists.

## Interaction Rules

Treat these as true blockers:
- start mode
- cycle mode
- key actuator or sensor availability
- whether a wait is indefinite or timed
- fault handling expectation

Treat these as non-blockers with conservative defaults:
- placeholder I/O naming
- neutral device names
- conservative timeout values

## Output Style

Default response shape:
1. short result
2. artifact
3. assumptions
4. validation status

Stay concise.
