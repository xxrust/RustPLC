# PLC Fix Obvious Errors

Task goal:

Using only the real `plc-gen` skill and the exported public artifacts, answer this task:

> Run the `fix` command mentally on the provided PLC source excerpts. Identify any obvious semantic error before proposing a repair. Focus on whether production states are proven by sensors, workpiece token transitions, operator events, topology-closed action completion, or explicit no-feedback semantics.

Observation points:

- Does the runner recognize that `fix` is a skill-level repair command, not a `rust_plc` binary subcommand?
- Does the runner inspect state proof before syntax or scenario cleanup?
- Does the runner flag internal `*_has_seed` / `*_ready` flags initialized to `true` when no field proof exists?
- Does the runner distinguish a test seed from cassette inventory or upstream replenishment?
- Does the runner avoid repairing the issue by simply setting the flag back to `true`?
- Does the runner propose a sensor-backed, workpiece-backed, operator-backed, topology-closed, or explicitly no-feedback repair direction?

If the blind runner must read repository source or private notes outside the exported public surface to answer, record `public-surface-gap`.
