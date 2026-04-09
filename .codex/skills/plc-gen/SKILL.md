---
name: plc-gen
description: Deliver, repair, and validate RustPLC DSL source sets and scaffolded projects. Use when Codex needs to generate or repair RustPLC DSL sources from a confirmed `plc/main.system.md` or equivalent system contract, whether as a single `.plc` file or a multi-file `.bundle.toml` plus fragments, scaffold a Day-1 RustPLC project, run project or scenario validation commands, or explain the current optimization surface of RustPLC.
---

# plc-gen

## Hard Guardrail: Closed-Loop Actuators

For topology-closed actuators such as cylinders, keep the action at the device-semantics layer.
Do not hand-write the normal endpoint confirmation with sensor waits.

Wrong:

```plc
step feed_forward:
    action: extend cyl_feed
    wait: sensor_feed_ext == true
    timeout: 800ms -> goto feed_warning.feed_cyl_warn
```

Wrong:

```plc
step orient_home:
    action: retract cyl_orient_rotate
    wait: sensor_orient_ret == true
```

Right:

```plc
step feed_forward:
    action: extend cyl_feed
        timeout: 800ms -> goto feed_warning.feed_cyl_warn
```

Right:

```plc
step orient_home:
    action: retract cyl_orient_rotate
        timeout: 600ms -> goto orient_warning.orient_cyl_warn
```

Checklist before writing a cylinder step:
- if the actuator is already modeled as a semantic device, do not recreate its closed loop in `task.step`
- if the topology is not closed enough to support the action semantically, report a blocker instead of silently downgrading to sensor choreography

## Hard Guardrail: Prefer Structured Source Sets

For complex projects, do not default to a monolithic `plc/main.plc`.

Prefer a structured source set or explicit target-semantics fragment layout that can be reviewed and validated in parallel.

Use `out/skill_flywheel/plc_gen_wafer_loader/plc/target_semantics_fragments` as the reference shape:
- `topology/`
- `constraints/`
- `architecture/`
- `auto/`
- `maintenance/`
- `manual/`
- `operator_interface/`

But do not treat that fragment tree as the whole authored architecture.

For real delivery, first classify the asset as:
- `module`
- `station`
- `line`

Then ensure that asset owns its own:
- `*.system.md`
- `*.architecture.md`
- `*.intent_alignment.contract.json`
- `*.verification.md`

The structured fragment tree is the compile surface.
The delivery asset and its document set are the architecture surface.

## Scaffold Rule

For a new multi-domain or long-lived station, start with:

```bash
rust_plc new <project_dir> --layout structured-fragments
```

or from the source workspace:

```bash
cargo run --release --bin rust_plc -- new <project_dir> --layout structured-fragments
```

Treat `plc/main.target_semantics.bundle.toml` as the source entry and fill the generated fragments under `plc/target_semantics_fragments/`.

## Hard Guardrail: Do Not Confuse The Bundle With The Whole Delivery

For complex projects, the compileable bundle is only one layer.

Preserve non-bundled authored sidecars when the contract needs them, such as:
- IO alias notes
- manual-mode sidecars
- operator-interface sidecars
- optimization notes
- maintenance self-check flows
- workpiece and business-intent sidecars

If you omit those sidecars, the result may compile but it is still structurally weaker than the reference target semantics.

## Hard Guardrail: Workpiece Semantics Are Mandatory When The Process Moves Real Parts

If the confirmed system contract describes real part flow, the PLC must model that flow with first-class RustPLC workpiece semantics.

Minimum required shape:
- declare `workpiece ...: workpiece_type`
- declare the participating `workpiece_location` / `workpiece_holder` / `workpiece_carrier`
- include the workpiece fragment in the compileable bundle when automatic tasks use the flow
- write `effect: acquire ...`, `effect: transfer ...`, `effect: finish ...` on the actual task steps

Do not leave workpiece semantics as a comment-only placeholder when the main production flow consumes or moves parts.

## Hard Guardrail: Intent Alignment Is Default For Complex Project Delivery

For any project-scale delivery such as:
- a scaffolded station
- a structured fragment source set
- a bundle-based PLC
- a canonical example
- a workpiece-carrying machine flow

intent alignment is not optional.

Default requirements:
- author a sibling `*.intent_alignment.contract.json` sidecar next to the source entry
- bind it to an authored intent source such as `plc/main.system.md`
- run `project-check` so the real `intent_alignment` step is appended after `no-board-gate`
- report the actual intent-alignment verdict instead of only reporting base-gate success

For complex projects, do not call the result `validated` if:
- the sidecar is missing without an explicit blocker
- `project-check` did not actually run the `intent_alignment` step
- the comparator failed or was blocked and that blocker was not reported

If the contract exists but anchor choice or cycle boundaries are still uncertain, run:

```bash
rust_plc intent-doctor <source.plc|source.bundle.toml> --trace <trace.jsonl> [--intent-contract <contract.json>] --output human
```

Use it to rank real anchor candidates from the compiled semantics plus observed trace before freezing milestone bindings.

When editing the `plc-gen` skill, prompts, or workflow references themselves, rerun the wafer-loader canary at `out/wafer_loader_project/`.

## Source Of Truth

Prefer these stable sources:
- `AGENTS.md`
- `docs/architecture/signal-direction.md`
- `docs/architecture/intent_alignment_verification.md`
- the confirmed authored intent source such as `plc/main.system.md`

## Core Rules

Controller / IO modeling guardrail:
- for scaffold delivery or any complex project that must survive real toolchain validation, prefer `device plc_main: plc { model_ref: ... }` backed by `devices/controllers/*.toml`
- do not invent inline controller `ports: [...]` in business DSL
- do not use raw `digital_input` / `digital_output` devices as the default topology backbone for complex projects when those names are only controller channels
- prefer semantic field devices plus explicit `relation { from, to, via }` mapping to `plc_main.<port>`
- if validation reports `SEM-108` or `SCN-MAP-010`, rewrite the topology first

General rules:
1. Only a real RustPLC toolchain-validated result counts as complete.
2. For project-scale requests, default to delivering a scaffolded project or coherent source set, not an isolated snippet.
3. Keep existing source boundaries unless there is a clear reason to restructure them.
4. If the intent source is not frozen, first converge it with `plc-system`.
5. Keep topology-closed device actions at the high semantic layer.
6. Treat `axis.move_*` as blocking long-running actions.
7. For complex delivery, the sibling `*.intent_alignment.contract.json` sidecar is required by default.
8. Milestones in that sidecar must be business milestones, not raw `task.step` names copied from code.
9. Clearly separate authored files from toolchain artifacts.
10. If you say intent alignment ran, it must have run on real evidence such as `sil_trace.jsonl`.

## References

Read these as needed:
- `references/workflow.md`
- `references/project-layout.md`
- `references/commands.md`
- `references/generation-rules.md`
- `references/multi-agent-template.md`
- `references/public-brief-template.md`
- `references/output-contract.md`
- `references/optimization.md`
- `references/troubleshooting.md`

Use these role briefs as needed:
- `agents/request-architect.md`
- `agents/senior-dsl-implementer.md`
- `agents/reviewer-validator.md`

## Default Workflow

1. Classify the request as single-file repair, bundle repair, project delivery, or optimization discussion.
2. Confirm the authoritative intent source.
3. Choose the source shape.
4. Generate or repair the DSL source entry.
5. Generate or repair `scenarios/nominal/normal.yaml`.
6. For complex delivery, generate or repair the sibling `*.intent_alignment.contract.json`.
7. Run `project-check`.
8. Confirm that `intent_alignment` actually appeared as a step.
9. Report the real verdict, mismatch, or blocker.

## One-Shot Protocol

For complex work, use this default protocol:
1. Build a `public brief`.
2. Let `request-architect` freeze source shape, lowering, artifact list, and write scopes.
3. Let one or more `senior-dsl-implementer` agents execute disjoint scopes.
4. Let `reviewer-validator` do the independent validation pass.

## Launcher Discipline

Decide first whether the environment is:
- an installed `rust_plc` binary
- or the RustPLC source workspace

Rules:
- `rust_plc ...` can run inside a scaffolded project directory
- `cargo run --release --bin rust_plc -- ...` must run from the RustPLC workspace root
- do not `cd` into a scaffolded project and then try to drive it with `cargo run` from there

## Completion Standard

A complex project is complete only when at least one of these is true:
- a validated source entry and scenario are delivered, and `project-check` actually ran `intent_alignment`
- a real blocker is reported with the failing command and artifact path
- a toolchain limitation is reported with exact evidence

Do not return a "theoretically should work" answer.
