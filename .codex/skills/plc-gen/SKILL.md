---
name: plc-gen
description: "Generate validated RustPLC DSL (.plc) from a confirmed system description or equivalent industrial control requirements. Use when the user wants a RustPLC program, wants main.plc filled inside a scaffolded project, or wants an existing .plc repaired to pass the current semantic and verification pipeline."
---

# plc-gen

Generate RustPLC DSL that survives the real pipeline.

The skill is successful only when the produced `.plc` is validated by current RustPLC tooling.

## Source of Truth

Use these project rules:
- `AGENTS.md`
- `docs/architecture/signal-direction.md`

Do not invent a second semantics model.
Generate code that matches the existing parser, semantic gate, runtime bridge, and verification chain.

## Input Contract

Preferred input:
- a confirmed `.system.md`

If the user skips `.system.md`, derive a minimal internal system model only when the remaining ambiguity is small.
If ambiguity changes safety, task partition, or fault handling, stop and ask only the missing blocking questions.

## Default Workflow

1. Read the confirmed system intent.
2. Build topology, constraints, tasks, and failure paths.
3. Prefer conservative task and timeout design.
4. Validate with RustPLC tooling.
5. Repair until the program passes or a real contract gap remains.

## Scaffold Rule

When the user wants a full project or asks to validate end to end, use the scaffold first:

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

Then write artifacts into:
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`

If not using scaffold, keep the generated `.plc` near its paired `.system.md`.

## Generation Rules

Always enforce:
- every device has `purpose`
- topology uses explicit `relation { from, to, via }`
- task semantics follow concurrent-task plus blocking-step rules
- human waits use `allow_indefinite_wait: true`
- non-human waits get explicit timeout routes
- failure routes are concrete tasks, not vague comments

Prefer this task skeleton unless the process clearly needs something else:
- `ready`
- `cycle`
- one or more `fail_*` tasks

## Concurrency and Blocking

Treat these as blocking by default:
- `wait`
- `delay`
- `timeout`
- `axis.move_relative`
- `axis.move_absolute`
- waits on external feedback

If an action must happen after an axis move completes, split it into a later step.

When independent workstations can progress while another waits, model them as separate tasks instead of flattening everything into one cycle task.

## Device and Constraint Heuristics

Prefer:
- `plc_main: plc { ports: [...] }`
- cylinders with paired `_ext` / `_ret` feedback when realistic
- explicit `requires` for dependency constraints
- `conflicts_with` only for true state coexistence conflicts

Do not use `conflicts_with` to encode mere execution order.

## Analog, PID, and Axis Rules

For analog signals:
- use `analog_input` / `analog_output`
- always declare `range` and `unit`
- avoid exact `==` thresholds when a range is safer

For PID:
- keep `pv` and `out` naming aligned with actual analog device names

For axis motion:
- prefer `axis.move_relative` / `axis.move_absolute`
- include `timeout`
- include `on_reject`
- include `on_motion_fault`
- include `on_safety_fault`

## Validation Loop

Validate generated code with the real toolchain whenever available:

```bash
cargo run --release -- <file.plc> --no-print-ir
```

If you are inside a scaffold project, also prefer:

```bash
cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

Use `no-board-gate` when the request is project-level and the scenario is ready.

## Fixture Discipline

This skill has fixture-backed regression coverage under:
- `.codex/skills/plc-gen/fixtures/valid/*.plc`

When changing the skill rules materially:
- update or add a representative fixture
- run `cargo test --test plc_gen_skill_fixtures`

## Output Style

Default response:
1. short result
2. generated `.plc`
3. assumptions
4. validation status

Keep explanations short unless the user asks for the reasoning.
