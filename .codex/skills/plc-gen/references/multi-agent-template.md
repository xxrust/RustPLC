# plc-gen Multi-Agent Template

Use multiple agents only when the project is large enough to justify disjoint write scopes.

## Default Roles

1. `request-architect`
2. `senior-dsl-implementer` for source delivery
3. `senior-dsl-implementer` for scenario and gate wiring when that scope is separate
4. `senior-dsl-implementer` for intent sidecar when that scope is separate
5. `reviewer-validator`

## When To Use This Template

Use the multi-agent split when the task involves several of:
- `.system.md` interpretation
- structured fragments or bundle repair
- scenario repair
- project-level validation
- authored `*.intent_alignment.contract.json`

## Required Write Scopes

Each implementer brief must specify:
- owned files
- forbidden files
- required proof obligation
- exact validation they must run locally before handoff

## Intent Sidecar Ownership

For complex projects, one implementer must explicitly own:
- the sibling `*.intent_alignment.contract.json`
- any contract-specific authored fixtures
- the proof that `project-check` really ran `intent_alignment`

Do not leave intent alignment as shared background responsibility.

## Reviewer Checks

The reviewer must verify:
- the source boundary is still coherent
- authored files and tool artifacts are clearly separated
- `project-check` really appended `intent_alignment`
- the reported verdict matches the generated artifacts
- the sidecar no longer contains scaffold placeholders such as `replace_me_after_authoring`
- the sidecar source binding resolves from the delivery root
- milestone anchors are comparator-supported and trace-backed, not synthetic starter labels
- docs do not promise recovery or branch behavior that the PLC fragments do not implement
