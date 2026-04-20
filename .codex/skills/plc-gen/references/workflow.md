# plc-gen Workflow

## Default Policy

For a project-scale RustPLC delivery, the default path is:

1. Confirm the authored intent source, usually `plc/main.system.md`.
2. Choose the source shape.
3. Generate or repair the DSL source entry.
4. Generate or repair `scenarios/nominal/normal.yaml`.
5. Author or repair a sibling `*.intent_alignment.contract.json`.
6. Run `project-check`.
7. Confirm that `project-check` actually ran `intent_alignment`.
8. Report the real verdict and the real blocker or mismatch if it did not align.

For complex projects, this is the default, not an optional extra.

## Source Shape

Use a structured fragment layout by default when the system contract has multiple stable semantic domains such as topology, constraints, supervision, auto flow, maintenance, manual mode, and operator interface.

Prefer:
- `rust_plc new <project_dir> --layout structured-fragments`
- `plc/main.target_semantics.bundle.toml` as the source entry
- `plc/target_semantics_fragments/` as the authored source tree

Only stay single-file when the request is truly small or the existing boundary is intentionally single-file.

## Intent-Alignment Sidecar

For project-scale delivery, the sidecar is required by default.

Create a sibling file with the same stem as the source entry:
- `main.plc` -> `main.intent_alignment.contract.json`
- `main.target_semantics.bundle.toml` -> `main.target_semantics.bundle.intent_alignment.contract.json`

The sidecar must:
- be authored, not treated as a compiler artifact
- bind to an authoritative intent source such as `plc/main.system.md`
- describe business milestones rather than using raw `task.step` names as the semantic center
- bind observations to real evidence that the current comparator can consume
- for concurrent or pipelined stations, prefer unique workpiece-handoff anchors over repeating prep-loop transitions

Before calling the sidecar delivery-grade, confirm all of the following:
- `source_digest.value` is no longer a scaffold placeholder such as `replace_me_after_authoring`
- `source_digest.value` is the real lowercase SHA-256 hex of the bound authored source
- `source_ref`, `authoritative_intent_source`, and every `review_basis[*].source` resolve from the delivery root
- the frozen anchors came from real trace evidence or `intent-doctor`
- the sidecar no longer contains starter placeholders such as `replace_with_real_anchor` or `replace_after_intent_doctor`

It is acceptable to skip the sidecar only when:
- the task is a tiny local repair
- the user explicitly asks to skip intent alignment
- or there is a concrete blocker that you report as `blocked`

## Validation

`project-check` with auto-discovered sidecar is the default validation path.

Required outcome for a complex project:
- `project-check` ran `compile_verify`
- `project-check` ran `sequence_lint`
- `project-check` ran `scenario_doctor`
- `project-check` ran `no_board_gate`
- `project-check` also ran `intent_alignment`

If the sidecar exists but `intent_alignment` did not appear, the delivery is not validated.

If the comparator reports `mismatch` or `blocked`, keep the artifacts and report the exact finding. Do not downgrade back to "base gate passed".
If the sidecar is still scaffold-grade or its source binding is unresolved, report `blocked` or `failed validation`; do not report `validated`.

## Canary

When changing `plc-gen` prompts, workflow, or output policy, rerun the structured wafer-loader canary:

```bash
target\debug\rust_plc.exe project-check out/wafer_loader_project/plc/main.target_semantics.bundle.toml --scenario out/wafer_loader_project/scenarios/nominal/normal.yaml --out-dir out/wafer_loader_project/out/project_check_with_intent_alignment --output human
```

The point of this canary is to catch the difference between:
- what the generator claims the station should do
- and what the real runtime trace actually proves
