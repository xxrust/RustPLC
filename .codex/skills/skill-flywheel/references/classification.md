# Classification

Use these four labels. Do not invent more unless a project has a strong reason.

## `skill-gap`

Use when the blind operator had the right artifacts but the target skill failed to:

- choose the right workflow
- ask the right next question
- interpret the available artifact correctly
- apply the project conventions already exposed publicly

Fix:

- patch the target skill
- keep the delta minimal and procedural

## `public-surface-gap`

Use when the blind operator needed a stable fact that should be consumable without source, but the repo did not export it clearly enough.

Examples:

- command matrix
- supported subcommands
- example inputs and outputs
- file layout contract
- machine-readable manifest
- generated help or report

Fix:

- add or improve exported artifacts
- avoid pasting the same detail into the skill unless the skill also needs the selection rule

## `code-gap`

Use when the outward-facing behavior itself is missing or too weak, such that even a perfect skill and perfect public bundle would still fail.

Examples:

- missing CLI help
- missing diagnostics
- unstable or absent report format
- no way to inspect supported capabilities without reading source

Fix:

- patch the codebase, CLI, or generated reports

## `task-ambiguity`

Use when the task itself is under-specified and the blind operator lacked an answer that only the user can provide.

Fix:

- improve task framing
- ask a narrower blocking question

## Tie-Breaker

If a fact is stable, externally useful, and derivable from the repo, prefer `public-surface-gap` over `skill-gap`.

If a fact is not externally visible because the product does not expose it, prefer `code-gap` over `skill-gap`.
