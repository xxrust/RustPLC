# Pain Points

Task: make `skill-flywheel` self-iterate like Ralph via a shell-driven fresh-process loop, and avoid converging before 5 outer iterations unless there is a hard blocker.

## Result

This weak-blind round verified that the exported closeout checklist now tells a blind runner to keep `decision.md` and `decision.json` synchronized, so the cycle can land machine-readable state instead of Markdown-only conclusions.

## Hypothesis Signal

Partial support.

## Pain Points

1. Step:
   Read `public/single-agent-closeout-checklist.md` from the newly initialized cycle.
   Observed blocker:
   Before this round, the public closeout path allowed a real conclusion to exist only in `decision.md`, while `decision.json` could remain placeholder content.
   Missing artifact or instruction:
   An explicit rule that `decision.json` is authoritative for shell-runner state and must be updated with the same conclusion.
   Impact:
   The outer runner could keep treating a closed cycle as unfinished, leaving `last_cycle`, `continue_next_iteration`, and stop condition out of sync with the actual research decision.

2. Step:
   Check whether the new public rule alone proves the shell loop can now advance five fresh-process iterations.
   Observed blocker:
   This round only validated the public closeout contract and test coverage; it did not yet rerun the outer shell loop end-to-end after the contract fix.
   Missing artifact or instruction:
   A fresh shell-driven replay that consumes a non-placeholder `decision.json` and shows the next iteration reading the updated disk state.
   Impact:
   The disk-state contract is clearer, but the session still lacks post-fix evidence that the outer runner will close and read cycles correctly across repeated invocations.
