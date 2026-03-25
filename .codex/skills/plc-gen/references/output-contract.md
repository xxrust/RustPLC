# plc-gen Output Contract

Use this file to shape the final response.

## Minimum Deliverables

Always return:

- the generated or repaired `.plc`
- a short assumptions list
- the exact launcher and commands used or recommended
- a validation result

## For Project-Level Requests

Return when relevant:

- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`
- the smallest command sequence needed to validate and deliver
- the current validation state

## Validation States

Use one of these explicitly:

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `failed validation`

Do not imply success without a real tool run.
