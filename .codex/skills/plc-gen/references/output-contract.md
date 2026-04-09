# plc-gen Output Contract

The final answer must always distinguish:
- authored files written by the skill
- toolchain artifacts produced by validation commands
- the actual validation verdict

## Minimum Output

Always report:
- what was generated or repaired
- what source entry is now authoritative
- what scenario was used
- what validation command actually ran
- whether the result is `validated`, `validated with warnings`, `failed validation`, or `blocked`

## Intent Alignment

For any complex project delivery, the final answer must explicitly state:
- whether a sibling `*.intent_alignment.contract.json` was created or repaired
- what its authoritative intent source is
- whether `project-check` actually ran the `intent_alignment` step
- the intent-alignment verdict
- the primary mismatch kind or blocker kind if the verdict was not aligned
- whether the sidecar still contains scaffold placeholders or unresolved source binding

Base-gate success without an executed `intent_alignment` step is not enough to call a complex project validated.
Scaffold placeholders such as `replace_me_after_authoring` or `replace_after_intent_doctor` also mean the project is not validated.

## Artifact Separation

Typical authored files:
- `plc/main.system.md`
- `plc/main.plc`
- `plc/main.target_semantics.bundle.toml`
- `plc/target_semantics_fragments/**`
- `scenarios/nominal/normal.yaml`
- `*.intent_alignment.contract.json`

Typical toolchain artifacts:
- `verification_report.json`
- `project_check_report.json`
- `sil_trace.jsonl`
- `intent_alignment/report.json`

Do not describe toolchain artifacts as if they were authored source files.
