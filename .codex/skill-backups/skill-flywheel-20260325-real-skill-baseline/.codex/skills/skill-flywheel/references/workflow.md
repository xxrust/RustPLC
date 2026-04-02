# Workflow

## Goal

Improve a target project skill quickly without leaking source-heavy knowledge into the skill itself.

## Loop

1. Choose the target skill and a real task.
2. Create a cycle directory with `scripts/init_public_surface.py`.
3. Let Agent 2 try the task using only:
   - the target skill
   - the generated `public/` bundle
4. Capture all blockers in `logs/pain-points.md`.
5. Let Agent 3 inspect the repo and classify each blocker.
6. If the blocker is a skill gap, let Agent 1 patch the target skill.
7. If the blocker is a public-surface gap, export a better artifact instead of bloating the skill.
8. If the blocker is a code gap, add or improve an external contract:
   - help output
   - manifest
   - report
   - stable example
   - machine-readable diagnostic
9. Re-run the same task or a nearby task to verify the change.

## Sequencing

Use this order by default:

1. Agent 1 prepares the current target skill or reviews it briefly.
2. Agent 2 performs the task blind.
3. Agent 3 performs root-cause analysis.
4. Agent 1 applies the minimal skill delta when Agent 3 classifies an issue as `skill-gap`.

Do not ask Agent 3 to redesign the whole skill. Agent 3 should classify and recommend.

## Artifact Contract

Every cycle should leave behind:

- `manifest.json`
- `logs/pain-points.md`
- `logs/root-cause.md`
- optional `logs/agent1-feedback.md`

Keep findings concrete:

- task step
- observed failure
- missing artifact or capability
- proposed layer for the fix

## Anti-Bloat Rule

Before editing the target skill, ask:

1. Is this missing fact stable across many tasks?
2. Can it be exported from the repo or CLI instead?
3. Would adding it to the skill duplicate information that should live in a generated artifact?

If the answer to 2 is yes, prefer the exported artifact.
