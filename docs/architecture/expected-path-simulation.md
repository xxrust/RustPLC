# Expected-Path Simulation

## Status

This document is retired.

The phase-1 "expected-path plus trace comparison" idea has been absorbed by the implemented intent-alignment pipeline.

Authoritative source of truth:
- `docs/architecture/intent_alignment_verification.md`

Current product surface:
- authored `*.intent_alignment.contract.json`
- `project-check` auto-discovery of that sidecar
- comparator execution against `sil_trace.jsonl`

Expected-path is no longer the active contract model and should not be used as the default authoring target for new work.
