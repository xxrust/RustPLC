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

## Hard Guardrail: Scenario Must Explicitly Drive Non-Closed Inputs

Do not assume a nominal scenario can advance a real cycle by pulsing only the start button.

If a wait depends on ordinary PLC inputs or sensor devices that are not topology-closed actuator feedback, the scenario must explicitly drive those inputs.

Examples:
- homing / target sensors for a motion platform
- presence sensors, cooling sensors, or manual reset inputs
- any `plc_main.<alias>` or `plc_main.X*` mapped field signal that runtime will not synthesize for you

Only topology-closed semantic device actions, such as a cylinder action with built-in endpoint semantics, may advance without hand-authored nominal sensor events.

Before freezing intent anchors, run a real trace and confirm it covers the intended cycle boundary.
If the trace stalls because the scenario only drove operator start and never drove the dependent field inputs, repair the scenario first instead of guessing the contract.

## Hard Guardrail: Operator Boundary Is Not A Device

Do not model a human operator as a normal `device`, and do not invent reverse topology links from the PLC back into a push button.

For buttons, selectors, reset inputs, manual acknowledgements, and HMI commands:
- keep the physical input as a semantic field device plus `relation { from: <button>.out, to: plc_main.<input_alias>, via: reports_to }`
- define that alias in `controller_io plc_main { ... }` inside `controller.plc`; use raw `plc_main.X*` only for minimal fixtures or when no project alias exists yet
- record the operator front-door semantics in the system/project docs: actor, command name, trigger type, allowed state, reject behavior, and required visible feedback
- for complex projects, scenario input events should carry `actor` / `source` provenance when the event represents an operator action
- outputs back to the human, such as lamps, buzzers, HMI status, and alarm messages, are feedback obligations, not proof that the button has an input side

Use `docs/architecture/operator-boundary-front-door.md` as the design source for this boundary.

## Hard Guardrail: Prefer Structured Source Sets

For complex projects, do not default to a monolithic `plc/main.plc`.

Prefer a structured source set or explicit target-semantics fragment layout that can be reviewed and validated in parallel.

Use the current `rust_plc new --layout structured-fragments` scaffold as the reference shape:
- `rustplc.bundle.toml`
- `00_topology/`
- `01_init/`
- `02_process/`
- `03_constraints/`
- `04_faults/`
- `05_supervision/`
- `06_manual/`
- `07_hmi/`

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

Treat `rustplc.bundle.toml` as the scaffolded source entry and fill the generated phase directories.

## Hard Guardrail: Replace Scaffold Placeholders Before Calling The Project Generated

For scaffolded complex delivery, the generated tree is only a starter shell.

Before calling the result "generated", "ready", or "validated":
- replace scaffold placeholder intent sources such as `plc/main.system.md` and any layer-specific docs you add
- ensure the delivery-asset doc set, not only the root scaffold doc, carries the confirmed process facts
- decide whether the authoritative source entry is the scaffolded `rustplc.bundle.toml` or a deliberately added delivery-asset bundle
- either author a real `*.intent_alignment.contract.json` with resolved source binding, or report an explicit blocker

Do not stop at "scaffold succeeded" when the request was to generate a real project from a confirmed `.system.md`.
If delivery docs still contain scaffold markers such as `Default Starter Flow`, `starter`, or `replace_me_after_authoring`, the job is not complete.

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

Capacity rules:
- use `capacity: 1` for true single-part positions such as pickup positions, process stations, sleeve entries, handoff points, and robot holders
- use `capacity > 1` for finite containers such as storage boxes, bins, racks, magazines, cassettes, trays, buffers, hoppers, reject bins, and scrap boxes
- if the source contract names a box/bin/rack/buffer but does not provide a number, choose a conservative finite capacity and record that assumption in `main.system.md`
- never let the one-cycle nominal scenario alone justify collapsing a real storage container to `capacity: 1`

Do not leave workpiece semantics as a comment-only placeholder when the main production flow consumes or moves parts.

Before validating a workpiece-carrying flow:
- decide whether the authored process is single-shot, finite-batch, or repeating
- if an ingress site represents a finite seed, do not loop back into a second cycle unless the system contract and scenario explicitly replenish another workpiece
- do not fake infinite supply by repeatedly acquiring or transferring from a source that only had one seeded workpiece
- ensure every reachable normal or fault terminal path finishes, rejects, or otherwise consumes the workpiece from the exact stage where it may reside
- do not collapse all fault paths into one generic handler when the active workpiece may be at different locations or holders

If the confirmed system is process-only and does not move discrete parts, do not invent fake workpiece flow just to satisfy project policy.
For scaffolded projects in that case, explicitly set `config/workpiece.toml` to a deliberate no-workpiece exception such as `required = false` before calling validation complete.

## Hard Guardrail: Raw AI/AO Process Control Is Out Of Current Generation Scope

Do not generate process-control examples that treat raw controller AI/AO channels as first-class devices.

If the request needs engineering-unit pressure, temperature, or PID behavior, model the real process equipment as a semantic device family where possible, or report a blocker for the missing process-device contract. Do not ship a "validated" project by falling back to raw AI/AO thresholds.

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
- use only schema-supported source kinds: `architecture_doc`, `canonical_example`, or `authored_asset`; patent excerpts and system contracts should normally be represented as `authored_asset` unless the Rust schema is extended
- write `source_digest.value` as the real lowercase SHA-256 hex of that authored source
- run `project-check` so the real `intent_alignment` step is appended after `no-board-gate`
- report the actual intent-alignment verdict instead of only reporting base-gate success
- replace scaffold placeholder digests such as `replace_me_after_authoring` before calling the delivery validated
- ensure `source_ref`, `authoritative_intent_source`, and every `review_basis[*].source` resolve from the workspace root used to launch `project-check`
- for generated projects under `out/...`, repo-root-relative paths are the safest default for those contract sources
- freeze `observation_bindings` against real comparator-supported evidence from trace or `intent-doctor`, not starter labels such as `replace_after_intent_doctor`

For complex projects, do not call the result `validated` if:
- the sidecar is missing without an explicit blocker
- `project-check` did not actually run the `intent_alignment` step
- the comparator failed or was blocked and that blocker was not reported
- the sidecar still contains scaffold placeholders or unresolved source binding

If `intent_alignment` returns `aligned` but also reports cross-cycle evidence warnings, report `validated with warnings` and name the warning.

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
- `docs/architecture/operator-boundary-front-door.md`
- `docs/architecture/intent_alignment_verification.md`
- the confirmed authored intent source such as `plc/main.system.md`

When the authoritative source contains non-ASCII text, read it with explicit UTF-8 handling before freezing lowering facts.
Do not proceed from mojibake or partially decoded system text.

## Core Rules

Controller / IO modeling guardrail:
- for scaffold delivery or any complex project that must survive real toolchain validation, prefer `device plc_main: plc { model_ref: ... }` backed by `devices/controllers/*.toml`
- for complex projects, define project-level names in `controller_io plc_main { input/output <alias>: X0/Y0/... { purpose: "...", safe_state: off } }`
- prefer using `plc_main.<alias>` in `connections.plc`; let semantic/preprocess lower aliases to canonical `X/Y/AI/AO` synthetic nodes
- do not invent inline controller `ports: [...]` in business DSL
- do not use raw `digital_input` / `digital_output` devices as the default topology backbone for complex projects when those names are only controller channels
- prefer semantic field devices plus explicit `relation { from, to, via }` mapping to `plc_main.<alias>`; raw `plc_main.<port>` is acceptable for small tests but should not be the complex-project default
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
   Keep `source_digest.value` in lowercase SHA-256 hex so source binding matches the toolchain verifier.
7. Run `project-check`.
8. Confirm that `intent_alignment` actually appeared as a step.
9. Report the real verdict, mismatch, or blocker.

## One-Shot Protocol

For complex work, use this default protocol:
1. Build a `public brief`.
2. Let `request-architect` freeze source shape, lowering, artifact list, and write scopes.
3. Let one or more `senior-dsl-implementer` agents execute disjoint scopes.
4. Let `reviewer-validator` do the independent validation pass.

This protocol is about parallel delivery of the RustPLC project itself.
Parallel blind runs used to optimize `plc-gen` belong to `skill-flywheel`, not to the generated PLC project.

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
