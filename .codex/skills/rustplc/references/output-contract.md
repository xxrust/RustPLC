# RustPLC Output Contract

Use this file to shape the final answer.

## Default Promise

Return one coherent RustPLC delivery, not a repo tour.

## Minimum Deliverables

Always return:

- the requested artifact or file contents
- a short assumptions list
- the exact launcher and commands used or recommended
- a validation result

## When the Request Is Project-Level

Return:

- `plc/main.system.md` when the system contract matters
- `plc/main.plc`
- `scenarios/nominal/normal.yaml` when scenario-driven validation matters
- the smallest command sequence needed to validate and deliver
- the current validation status

If you scaffolded a project, say which files were created or filled.

## When the Request Is Single-Artifact

Return:

- the repaired or generated `.plc`
- any required paired scenario path
- the validation command
- the validation result

## Validation Language

Use one of these states explicitly:

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `failed validation`

Do not imply success without a real tool run.

## Failure Contract

If the result is blocked, report:

- the exact missing input or contract gap
- the command that would be run next once the gap is resolved
- any conservative assumption you refused to invent

## Style

Stay concise.
Prefer short sections in this order:

1. result
2. artifacts
3. assumptions
4. validation
