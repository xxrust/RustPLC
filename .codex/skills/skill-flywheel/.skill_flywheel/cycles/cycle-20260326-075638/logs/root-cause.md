# Root Cause

## Hypothesis Judgment

Partial support.

## Findings

1. Pain point:
   A cycle could be semantically closed in `decision.md` while `decision.json` stayed placeholder.
   Classification:
   `public-surface-gap`
   Cause:
   The exported weak-blind closeout contract required writing the JSON files, but it did not make explicit that `decision.json` is the authoritative input for shell-runner state reconciliation and stop conditions.
   Minimal fix:
   Update `public/single-agent-closeout-checklist.md` to require synchronized `md/json` closeout and name the exact decision fields that must be carried into JSON.

2. Pain point:
   The session still lacks proof that the shell loop consumes the new structured decision correctly across fresh-process iterations.
   Classification:
   `code-gap`
   Cause:
   This round only repaired the public contract and test coverage; no post-fix shell replay has yet demonstrated `runner_state.json` updating from a substantive `decision.json`.
   Minimal fix:
   Run the outer shell runner again in the next cycle and verify that it reads a non-placeholder `decision.json`, advances `last_cycle`, and preserves progress across repeated invocations.
