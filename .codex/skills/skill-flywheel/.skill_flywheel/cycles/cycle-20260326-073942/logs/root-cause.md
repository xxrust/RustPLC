# Root Cause

## Hypothesis Judgment

Partial support.

## Findings

1. Pain point:
   The active post-baseline cycle was left as placeholders after a timed-out run.
   Classification:
   `public-surface-gap`
   Cause:
   The task-specific public checklist covered startup and observation, but not the recovery rule for an already-open active cycle.
   Minimal fix:
   Export an explicit closeout rule in `public/autonomous-self-improve-checklist.md` that points to `public/single-agent-closeout-checklist.md`.

2. Pain point:
   A blind runner could misread "initialize a cycle if none exists" as permission to open a new cycle even when a post-baseline active cycle already exists.
   Classification:
   `public-surface-gap`
   Cause:
   The session-specific continuation path was implicit in the generic workflow but not surfaced in the task-specific checklist.
   Minimal fix:
   Make the active-cycle-first rule explicit and verify it with a public-contract test.
