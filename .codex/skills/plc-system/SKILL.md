---
name: plc-system
description: "Generate or repair a RustPLC system semantic description (.system.md) before PLC code generation. Use when the user wants to analyze PLC requirements, define project scope, create main.system.md, or turn process intent into a stable system contract."
---

# plc-system

Generate a confirmed `.system.md` that downstream PLC generation can trust.

Keep the skill narrow:
- define system identity, safety level, process intent, task boundaries, and key constraints
- do not generate `.plc` here
- do not dump a questionnaire

Keep this file lean.
Load only the reference file you need:

- `references/workflow.md`
  Use for the system-confirmation flow and blocking-question policy.
- `references/sections.md`
  Use when drafting or repairing `main.system.md`.
- `references/handoff.md`
  Use to produce a clean downstream handoff to `plc-gen`.

## Required Semantics

Treat `docs/architecture/signal-direction.md` as the source of truth for:
- concurrent tasks
- blocking steps
- blocking isolation

Model the system in terms that can later enter:
- semantic checks
- runtime
- safety / liveness / timing / causality verification

Do not describe the system as a single execution pointer jumping across `task.step`.

## Default Workflow

1. Read the requirement and propose a concrete system interpretation first.
2. Ask only 1 to 3 blocking questions if safety, task boundaries, or fault handling are still ambiguous.
3. Produce a `.system.md` with stable sections.
4. Get confirmation or note explicit assumptions.
5. Hand off to PLC generation.

Use this response shape when information is mostly clear:

```text
Current recommendation: ...
Reason: ...
Please confirm. If not, state the real constraint.
```

Use this response shape only when responsible advice is impossible:

```text
I cannot make a responsible recommendation yet because I still need: ...
This directly affects: ...
Please confirm: ...
```

## Preferred Output Sections

Always include:
- project identity
- system mission
- safety and reliability level
- operating environment
- normal process flow
- abnormal handling
- concurrent task partition
- blocking step expectations
- startup and stop flow
- testing and maintenance modes
- key constraints
- AI generation guidance

Add an axis section when motion axes exist:
- parameter layering (`model_ref` / `config_ref` / `motion_param_set`)
- homing / soft limits
- fault policy
- propagation scope

## Task and Blocking Rules

The `.system.md` must state:
- which activities should become separate tasks
- which waits are blocking steps
- which tasks must continue while another task is blocked
- which resources are shared or mutually exclusive

At minimum, call out:
- `wait`
- `delay`
- `timeout`
- `axis.move_*`
- human confirmation waits
- external feedback waits

## High-Impact Topics

Prioritize these questions or recommendations:
- system safety class and failure consequence
- start mode and cycle mode
- startup / reset / e-stop policy
- manual intervention points
- task partition and blocking isolation
- shared-resource conflicts
- timeout and fault routing expectations

Do not spend the first turn on low-impact details like exact I/O numbering.

## Scaffold Rule

If the request is for a full project rather than a standalone artifact, prefer the scaffold layout:

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

Then place the generated system file at:
- `plc/main.system.md`

If working without scaffold, keep `.system.md` next to the target `.plc`.

## Handoff Contract to plc-gen

The finished `.system.md` should let PLC generation decide:
- topology shape
- safety constraints
- task structure
- timeout strategy
- failure tasks
- scenario and validation baseline

End with a concise handoff note:

```text
The system contract is confirmed. Proceed to `.plc` generation.
```
